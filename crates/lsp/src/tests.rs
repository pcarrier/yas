//! Engine tests against a scripted in-process fake LSP server — the
//! quirk harness (docs/design/lsp.md "Server implementation"): quirk
//! handling is tested deterministically, not against whatever
//! rust-analyzer does today.

use std::collections::HashMap;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::attach::Attachment;
use crate::backend::Backend;
use crate::discovery::{MarkerGroup, RootPolicy, ServerSpec};
use crate::model::*;
use crate::native::{self, QueryKind, QueryRecord, Status, Stream};
use crate::rpc;
use crate::{Budgets, testutil};

const WATCH: u8 = 1;
const DIAGNOSTICS: u8 = 2;

#[derive(Clone, Debug)]
enum Output {
    Event(native::Event),
    Query(native::QueryResponse),
}

type Sink = Arc<dyn Fn(Output) -> bool + Send + Sync>;

fn event_sink(sink: &Sink) -> native::EventSink {
    let sink = sink.clone();
    Arc::new(move |event| sink(Output::Event(event)))
}

fn query_sink(sink: &Sink) -> native::QuerySink {
    let sink = sink.clone();
    Arc::new(move |response| sink(Output::Query(response)))
}

fn query_response(output: &Output) -> Option<native::QueryResponse> {
    match output {
        Output::Query(response) => Some(response.clone()),
        Output::Event(_) => None,
    }
}

fn query_records(records: &[QueryRecord]) -> impl Iterator<Item = &QueryRecord> {
    records.iter()
}

fn query_kind(kind: u8) -> QueryKind {
    match kind {
        LSP_QUERY_DEFINITION => QueryKind::Definition,
        LSP_QUERY_REFERENCES => QueryKind::References,
        LSP_QUERY_HOVER => QueryKind::Hover,
        LSP_QUERY_DOC_SYMBOLS => QueryKind::DocumentSymbols,
        LSP_QUERY_WS_SYMBOLS => QueryKind::WorkspaceSymbols,
        LSP_QUERY_RENAME => QueryKind::Rename,
        LSP_QUERY_COMPLETION => QueryKind::Completion,
        LSP_QUERY_SIGNATURE => QueryKind::SignatureHelp,
        LSP_QUERY_CODE_ACTIONS => QueryKind::CodeActions,
        LSP_QUERY_FORMATTING => QueryKind::Formatting,
        _ => panic!("unknown query kind {kind}"),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_query(
    attachment: &Attachment,
    nonce: u16,
    kind: u8,
    flags: u8,
    line: u32,
    column: u32,
    path: &str,
    argument: &str,
    sink: &Sink,
) {
    let path = (kind != LSP_QUERY_WS_SYMBOLS).then(|| attachment.root.join(path));
    attachment.query_native(
        native::QueryRequest {
            nonce,
            kind: query_kind(kind),
            flags,
            line,
            column,
            path: path.as_deref(),
            argument,
        },
        query_sink(sink),
    );
}

#[derive(Default)]
struct StateMirror {
    servers: HashMap<u16, native::Server>,
}

impl StateMirror {
    fn apply(&mut self, output: &Output) -> Option<u32> {
        let Output::Event(native::Event::State { update_id, servers }) = output else {
            return None;
        };
        self.servers = servers
            .iter()
            .cloned()
            .map(|server| (server.server_ref, server))
            .collect();
        Some(*update_id)
    }
}

#[derive(Default)]
struct DiagnosticsMirror {
    files: HashMap<PathBuf, native::FileDiagnostics>,
}

impl DiagnosticsMirror {
    fn apply(&mut self, output: &Output) -> Option<u32> {
        let Output::Event(native::Event::Diagnostics {
            update_id,
            full,
            files,
        }) = output
        else {
            return None;
        };
        if *full {
            self.files.clear();
        }
        for file in files {
            if file.diagnostics.is_empty() {
                self.files.remove(&file.path);
            } else {
                self.files.insert(file.path.clone(), file.clone());
            }
        }
        Some(*update_id)
    }
}

#[test]
fn prepare_classifies_empty_and_missing_paths() {
    assert_eq!(
        crate::prepare_native(Path::new("")).err().unwrap().status,
        Status::Invalid
    );
    assert_eq!(
        crate::prepare_native(Path::new("missing/path"))
            .err()
            .unwrap()
            .status,
        Status::NotFound
    );
}

#[test]
fn native_prepare_represents_no_backend_and_validates_explicit_selection() {
    let root = tmp_root("native-prepare");
    let (prepared, detail) = crate::prepare_native(&root).unwrap();
    assert_eq!(prepared.backend_count(), 0);
    assert!(!detail.is_empty());

    let error = crate::prepare_explicit(&root, "rust", "rust-analyzer", b"not-json")
        .err()
        .expect("invalid initialization options");
    assert_eq!(error.status, Status::Invalid);
}

fn test_spec() -> ServerSpec {
    ServerSpec {
        id: "fake".into(),
        command: vec!["fake".into()],
        groups: vec![MarkerGroup {
            markers: vec!["marker".into()],
            policy: RootPolicy::Nearest,
        }],
        extensions: vec!["rs".into()],
        needs_open_doc: false,
        init: None,
        settings: Some(json!({ "answer": 42 })),
    }
}

fn test_budgets() -> Budgets {
    Budgets {
        query_timeout: Duration::from_secs(5),
        init_timeout: Duration::from_secs(5),
        // Short quiescence grace so wait_ready tests stay fast.
        ready_grace: Duration::from_millis(80),
        ..Budgets::default()
    }
}

fn tmp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yas-lsp-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn collector() -> (Sink, Receiver<Output>) {
    let (tx, rx) = std::sync::mpsc::channel::<Output>();
    (Arc::new(move |msg| tx.send(msg).is_ok()), rx)
}

/// Wait for a message satisfying `pick`, discarding others.
fn wait_for<T>(rx: &Receiver<Output>, mut pick: impl FnMut(&Output) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for message");
        let msg = rx.recv_timeout(left).expect("channel closed or timed out");
        if let Some(t) = pick(&msg) {
            return t;
        }
    }
}

/// The scripted fake server: handles the lifecycle and a fixed set of
/// query methods; forwards a copy of every received method name to
/// `seen`; sends the notifications/requests in `extra` right after
/// `initialized` arrives.
#[derive(Clone)]
struct FakeCfg {
    encoding: &'static str,
    /// `(json payloads)` sent after the `initialized` notification.
    after_init: Vec<Value>,
    seen: Option<Sender<String>>,
}

fn fake_server(
    cfg: FakeCfg,
) -> impl FnMut(BufReader<Box<dyn Read + Send>>, Box<dyn Write + Send>) + Clone + Send + 'static {
    move |mut reader, mut writer| {
        let cfg = cfg.clone();
        let mut next_req_id = 1000i64;
        while let Some(msg) = rpc::read_msg(&mut reader) {
            match msg {
                rpc::RpcMsg::Request { id, method, params } => {
                    if let Some(seen) = &cfg.seen {
                        let _ = seen.send(method.clone());
                    }
                    let reply = match method.as_str() {
                        "initialize" => rpc::response(
                            &id,
                            json!({
                                "capabilities": {
                                    "positionEncoding": cfg.encoding,
                                    "definitionProvider": true,
                                    "referencesProvider": true,
                                    "hoverProvider": true,
                                    "documentSymbolProvider": true,
                                    "workspaceSymbolProvider": true,
                                    "renameProvider": true,
                                    "completionProvider": { "triggerCharacters": ["."] },
                                    "signatureHelpProvider": { "triggerCharacters": ["("] },
                                    "codeActionProvider": true,
                                    "documentFormattingProvider": true,
                                    "documentRangeFormattingProvider": true,
                                },
                                "serverInfo": { "name": "fake" },
                            }),
                        ),
                        "shutdown" => rpc::response(&id, Value::Null),
                        "textDocument/definition" => {
                            let uri = params["textDocument"]["uri"].as_str().unwrap().to_string();
                            // One target on line 1 spanning the 'é' —
                            // characters 1..2 in UTF-16.
                            rpc::response(
                                &id,
                                json!([ { "uri": uri, "range": {
                                    "start": { "line": 1, "character": 1 },
                                    "end": { "line": 1, "character": 2 },
                                } } ]),
                            )
                        }
                        "textDocument/documentSymbol" => rpc::response(
                            &id,
                            json!([{
                                "name": "Outer",
                                "kind": 5,
                                "range": { "start": { "line": 0, "character": 0 },
                                           "end": { "line": 3, "character": 0 } },
                                "selectionRange": { "start": { "line": 0, "character": 0 },
                                                    "end": { "line": 0, "character": 5 } },
                                "children": [{
                                    "name": "inner",
                                    "kind": 12,
                                    "range": { "start": { "line": 1, "character": 0 },
                                               "end": { "line": 2, "character": 0 } },
                                    "selectionRange": { "start": { "line": 1, "character": 0 },
                                                        "end": { "line": 1, "character": 5 } },
                                }],
                            }]),
                        ),
                        "textDocument/rename" => {
                            let uri = params["textDocument"]["uri"].as_str().unwrap().to_string();
                            // UTF-16 units 2..4 are exactly the 𝄞
                            // character: bytes 3..7.
                            rpc::response(
                                &id,
                                json!({ "changes": { uri: [
                                    { "range": { "start": { "line": 1, "character": 2 },
                                                 "end": { "line": 1, "character": 4 } },
                                      "newText": "renamed" },
                                ] } }),
                            )
                        }
                        "textDocument/completion" => rpc::response(
                            &id,
                            // Out of sortText order on purpose (zz before
                            // aa), with a UTF-16 edit range over the é on
                            // line 1 (units 1..2 = bytes 1..3) and one
                            // snippet item without a textEdit.
                            json!({ "isIncomplete": true, "items": [
                                { "label": "zz_last", "kind": 6, "sortText": "b",
                                  "detail": "u32",
                                  "textEdit": { "range": {
                                      "start": { "line": 1, "character": 1 },
                                      "end": { "line": 1, "character": 2 } },
                                    "newText": "zz_last" } },
                                { "label": "aa_first", "kind": 3, "sortText": "a",
                                  "preselect": true,
                                  "insertText": "aa_first(${1:x})",
                                  "insertTextFormat": 2,
                                  "tags": [1] },
                            ] }),
                        ),
                        "textDocument/signatureHelp" => rpc::response(
                            &id,
                            // The active parameter's label is UTF-16
                            // offsets 5..8 into "f(a: 𝄞x)" — 𝄞 is two
                            // units / four bytes, so bytes 5..10.
                            json!({
                                "activeSignature": 1,
                                "activeParameter": 0,
                                "signatures": [
                                    { "label": "f()" },
                                    { "label": "f(a: 𝄞x)",
                                      "documentation": { "kind": "markdown",
                                                         "value": "docs" },
                                      "parameters": [
                                          { "label": [5, 8] },
                                      ] },
                                ],
                            }),
                        ),
                        "textDocument/codeAction" => {
                            let uri = params["textDocument"]["uri"].as_str().unwrap().to_string();
                            rpc::response(
                                &id,
                                json!([{
                                    "title": "Fix spelling",
                                    "kind": "quickfix",
                                    "isPreferred": true,
                                    "edit": { "changes": { uri: [{
                                        "range": {
                                            "start": { "line": 1, "character": 1 },
                                            "end": { "line": 1, "character": 2 }
                                        },
                                        "newText": "e"
                                    }] } }
                                }]),
                            )
                        }
                        "textDocument/formatting" | "textDocument/rangeFormatting" => {
                            rpc::response(
                                &id,
                                json!([{
                                    "range": {
                                        "start": { "line": 1, "character": 1 },
                                        "end": { "line": 1, "character": 2 }
                                    },
                                    "newText": "E"
                                }]),
                            )
                        }
                        _ => rpc::error_response(&id, -32601, "unhandled in fake"),
                    };
                    let _ = rpc::write_msg(writer.as_mut(), &reply);
                }
                rpc::RpcMsg::Notification { method, .. } => {
                    if let Some(seen) = &cfg.seen {
                        let _ = seen.send(method.clone());
                    }
                    if method == "initialized" {
                        for payload in &cfg.after_init {
                            let mut payload = payload.clone();
                            if payload.get("id") == Some(&json!("FRESH")) {
                                next_req_id += 1;
                                payload["id"] = json!(next_req_id);
                            }
                            let _ = rpc::write_msg(writer.as_mut(), &payload);
                        }
                    }
                    if method == "exit" {
                        return;
                    }
                }
                rpc::RpcMsg::Response { .. } => {}
            }
        }
    }
}

fn start(tag: &str, cfg: FakeCfg) -> (PathBuf, Arc<Backend>) {
    let root = tmp_root(tag);
    (
        root.clone(),
        testutil::pipe_backend(test_spec(), root, test_budgets(), fake_server(cfg)),
    )
}

fn attach(root: &Path, backend: &Arc<Backend>, _flags: u8, sink: Sink) -> Attachment {
    Attachment::start_native(
        root.to_path_buf(),
        vec![backend.clone()],
        vec![(test_spec(), root.to_path_buf())],
        1,
        event_sink(&sink),
        &test_budgets(),
    )
}

#[test]
fn diagnostics_cardinality_overflow_forces_full_resnapshot() {
    let root = tmp_root("diag-cardinality");
    let mut after_init = Vec::new();
    for name in ["a.rs", "b.rs", "c.rs"] {
        let path = root.join(name);
        std::fs::write(&path, "x\n").unwrap();
        after_init.push(json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": crate::text::path_to_uri(&path),
                "diagnostics": [{
                    "range": {
                        "start": { "line": 0, "character": 0 },
                        "end": { "line": 0, "character": 1 }
                    },
                    "severity": 1,
                    "source": "fake",
                    "message": name
                }]
            }
        }));
    }
    let budgets = Budgets {
        diag_files_max: 2,
        diag_entries_max: 8,
        diag_bytes_max: 4_096,
        ..test_budgets()
    };
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        budgets.clone(),
        fake_server(FakeCfg {
            encoding: "utf-16",
            after_init,
            seen: None,
        }),
    );
    let (sink, rx) = collector();
    let att = Attachment::start_native(
        root.clone(),
        vec![backend.clone()],
        vec![(test_spec(), root.clone())],
        1,
        event_sink(&sink),
        &budgets,
    );
    let mut mirror = DiagnosticsMirror::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            Instant::now() < deadline,
            "cache reset never reached subscriber"
        );
        let output = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let full = matches!(
            &output,
            Output::Event(native::Event::Diagnostics { full: true, .. })
        );
        let Some(update_id) = mirror.apply(&output) else {
            continue;
        };
        att.ack_native(Stream::Diagnostics, update_id);
        if full
            && backend
                .shared
                .diag_epoch
                .load(std::sync::atomic::Ordering::Relaxed)
                > 0
            && mirror.files.len() == 1
            && mirror.files.contains_key(&root.join("c.rs"))
        {
            break;
        }
    }
    let cache = backend.shared.diags.lock().unwrap();
    assert_eq!(cache.len(), 1);
    assert!(cache.contains_key(&root.join("c.rs")));
    assert!(
        backend
            .shared
            .diag_bytes
            .load(std::sync::atomic::Ordering::Relaxed)
            <= 4_096
    );
    assert!(
        backend
            .shared
            .diag_entries
            .load(std::sync::atomic::Ordering::Relaxed)
            <= 8
    );
}

#[test]
fn stalled_projection_worker_sheds_excess_query_with_budget() {
    let root = tmp_root("projection-pressure");
    std::fs::write(root.join("a.rs"), "x\n").unwrap();
    let budgets = Budgets {
        projection_queue_max: 1,
        ..test_budgets()
    };
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        budgets.clone(),
        fake_server(FakeCfg {
            encoding: "utf-16",
            after_init: Vec::new(),
            seen: None,
        }),
    );
    wait_ready(&backend);

    let (output_tx, output_rx) = std::sync::mpsc::channel();
    let (blocked_tx, blocked_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
    let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
    let sink: Sink = Arc::new(move |output| {
        let block = matches!(query_response(&output), Some(response) if response.nonce == 1);
        let accepted = output_tx.send(output).is_ok();
        if block {
            let _ = blocked_tx.send(());
            let _ = release_rx.lock().unwrap().recv();
        }
        accepted
    });
    let att = Attachment::start_native(
        root.clone(),
        vec![backend],
        vec![(test_spec(), root)],
        1,
        event_sink(&sink),
        &budgets,
    );
    run_query(&att, 1, LSP_QUERY_DEFINITION, 0, 0, 0, "a.rs", "", &sink);
    blocked_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    run_query(&att, 2, LSP_QUERY_DEFINITION, 0, 0, 0, "a.rs", "", &sink);
    run_query(&att, 3, LSP_QUERY_DEFINITION, 0, 0, 0, "a.rs", "", &sink);

    let exhausted = wait_for(&output_rx, |output| {
        query_response(output)
            .filter(|response| response.status == Status::ResourceExhausted)
            .map(|response| response.nonce)
    });
    assert_eq!(exhausted, 3);
    release_tx.send(()).unwrap();
    let second = wait_for(&output_rx, |output| {
        query_response(output)
            .filter(|response| response.nonce == 2)
            .map(|response| response.status)
    });
    assert_eq!(second, Status::Ok);
}

#[test]
fn nonreplying_server_cannot_grow_pending_queries_past_budget() {
    let root = tmp_root("pending-query-pressure");
    std::fs::write(root.join("a.rs"), "x\n").unwrap();
    let serve = move |mut reader: BufReader<Box<dyn Read + Send>>,
                      mut writer: Box<dyn Write + Send>| {
        while let Some(message) = rpc::read_msg(&mut reader) {
            match message {
                rpc::RpcMsg::Request { id, method, .. } if method == "initialize" => {
                    let response = rpc::response(
                        &id,
                        json!({
                            "capabilities": {
                                "positionEncoding": "utf-16",
                                "definitionProvider": true
                            }
                        }),
                    );
                    rpc::write_msg(writer.as_mut(), &response).unwrap();
                }
                // Deliberately drain but never answer semantic requests.
                rpc::RpcMsg::Request { .. } => {}
                rpc::RpcMsg::Notification { method, .. } if method == "exit" => return,
                _ => {}
            }
        }
    };
    let budgets = Budgets {
        pending_queries_max: 2,
        ..test_budgets()
    };
    let backend = testutil::pipe_backend(test_spec(), root.clone(), budgets.clone(), serve);
    wait_ready(&backend);
    let (sink, output_rx) = collector();
    let att = Attachment::start_native(
        root.clone(),
        vec![backend.clone()],
        vec![(test_spec(), root)],
        1,
        event_sink(&sink),
        &budgets,
    );
    for nonce in 1..=3 {
        run_query(
            &att,
            nonce,
            LSP_QUERY_DEFINITION,
            0,
            0,
            0,
            "a.rs",
            "",
            &sink,
        );
    }
    let response = wait_for(&output_rx, |output| {
        query_response(output).filter(|response| response.nonce == 3)
    });
    assert_eq!(response.status, Status::ResourceExhausted);
    backend.send(crate::backend::Cmd::Stop);
}

#[test]
fn stalled_child_writer_is_bounded_and_fails_the_session() {
    let root = tmp_root("writer-pressure");
    let path = root.join("a.rs");
    std::fs::write(&path, "disk\n").unwrap();
    let (stalled_tx, stalled_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = Arc::new(std::sync::Mutex::new(release_rx));
    let serve = move |mut reader: BufReader<Box<dyn Read + Send>>,
                      mut writer: Box<dyn Write + Send>| {
        while let Some(message) = rpc::read_msg(&mut reader) {
            match message {
                rpc::RpcMsg::Request { id, method, .. } if method == "initialize" => {
                    let response = rpc::response(
                        &id,
                        json!({
                            "capabilities": {
                                "positionEncoding": "utf-16",
                                "definitionProvider": true
                            }
                        }),
                    );
                    rpc::write_msg(writer.as_mut(), &response).unwrap();
                }
                rpc::RpcMsg::Notification { method, .. } if method == "initialized" => {
                    let _ = stalled_tx.send(());
                    let _ = release_rx.lock().unwrap().recv();
                    return;
                }
                _ => {}
            }
        }
    };
    let budgets = Budgets {
        writer_queue_max: 1,
        buffer_max: 1024 * 1024,
        max_restarts: 0,
        ..test_budgets()
    };
    let backend = testutil::pipe_backend(test_spec(), root.clone(), budgets.clone(), serve);
    let (sink, _rx) = collector();
    let att = Attachment::start_native(
        root.clone(),
        vec![backend.clone()],
        vec![(test_spec(), root)],
        1,
        event_sink(&sink),
        &budgets,
    );
    stalled_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    wait_ready(&backend);
    for marker in *b"abc" {
        let mut body = vec![b'x'; 512 * 1024];
        body[0] = marker;
        att.buffer(&path, Some(body));
        std::thread::sleep(Duration::from_millis(200));
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    while backend.phase() != LSP_PHASE_FAILED {
        assert!(
            Instant::now() < deadline,
            "writer pressure did not fail session"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        backend
            .shared
            .info
            .lock()
            .unwrap()
            .msg
            .contains("server exited")
    );
    release_tx.send(()).unwrap();
}

#[test]
fn state_reaches_ready_with_caps() {
    let (root, backend) = start(
        "ready",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![],
            seen: None,
        },
    );
    let (sink, rx) = collector();
    let att = attach(&root, &backend, WATCH, sink);
    let mut mirror = StateMirror::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "never reached READY");
        let msg = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        if let Some(state_id) = mirror.apply(&msg) {
            att.ack_native(Stream::State, state_id);
            let server = &mirror.servers[&1];
            assert_eq!(server.id, "fake");
            if server.phase == LSP_PHASE_READY {
                assert!(server.capabilities.definition());
                assert!(server.capabilities.rename());
                break;
            }
        }
    }
}

/// READY means quiescent, not merely initialized: an active
/// `$/progress` token holds the phase at INDEXING well past the grace
/// window, so `yas lsp wait` cannot return mid-warmup.
#[test]
fn active_progress_holds_off_ready() {
    let (_root, backend) = start(
        "hold",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![json!({
                "jsonrpc": "2.0",
                "method": "$/progress",
                "params": { "token": "warm", "value": {
                    "kind": "begin", "title": "indexing", "percentage": 5,
                } },
            })],
            seen: None,
        },
    );
    assert_holds_indexing(&backend);
}

/// The last progress `end` starts the grace clock; READY follows once
/// the session stays idle through it.
#[test]
fn progress_end_promotes_ready_after_grace() {
    let progress = |kind: Value| {
        json!({
            "jsonrpc": "2.0",
            "method": "$/progress",
            "params": { "token": "warm", "value": kind },
        })
    };
    let (_root, backend) = start(
        "grace",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![
                progress(json!({ "kind": "begin", "title": "indexing" })),
                progress(json!({ "kind": "end" })),
            ],
            seen: None,
        },
    );
    wait_ready(&backend);
}

/// A server that reports quiescence explicitly (rust-analyzer's
/// experimental serverStatus) overrides the grace heuristic in both
/// directions: `quiescent:false` pins INDEXING past any idle window…
#[test]
fn server_status_nonquiescent_holds_indexing() {
    let (_root, backend) = start(
        "status-busy",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![json!({
                "jsonrpc": "2.0",
                "method": "experimental/serverStatus",
                "params": { "health": "ok", "quiescent": false },
            })],
            seen: None,
        },
    );
    assert_holds_indexing(&backend);
}

/// Wait for the warmup signal to land (phase INDEXING), then outlast
/// the grace window several times over and check it stuck.
fn assert_holds_indexing(backend: &Arc<Backend>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "never reached INDEXING");
        if backend.shared.info.lock().unwrap().phase == LSP_PHASE_INDEXING {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(test_budgets().ready_grace * 6);
    assert_eq!(
        backend.shared.info.lock().unwrap().phase,
        LSP_PHASE_INDEXING
    );
}

/// …and `quiescent:true` promotes to READY without waiting out the
/// grace window.
#[test]
fn server_status_quiescent_promotes_ready() {
    let status = |quiescent: bool| {
        json!({
            "jsonrpc": "2.0",
            "method": "experimental/serverStatus",
            "params": { "health": "ok", "quiescent": quiescent },
        })
    };
    let (_root, backend) = start(
        "status-ready",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![status(false), status(true)],
            seen: None,
        },
    );
    wait_ready(&backend);
}

#[test]
fn query_before_ready_answers_warming() {
    // A server that never answers initialize.
    let silent = |mut reader: BufReader<Box<dyn Read + Send>>, _writer: Box<dyn Write + Send>| {
        while rpc::read_msg(&mut reader).is_some() {}
    };
    let root = tmp_root("warming");
    std::fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
    let backend = testutil::pipe_backend(test_spec(), root.clone(), test_budgets(), silent);
    let (sink, rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    run_query(&att, 7, LSP_QUERY_DEFINITION, 0, 0, 0, "a.rs", "", &sink);
    let (nonce, status) = wait_for(&rx, |msg| query_response(msg).map(|r| (r.nonce, r.status)));
    assert_eq!((nonce, status), (7, Status::Warming));
}

#[test]
fn definition_transcodes_utf16_to_bytes() {
    let (root, backend) = start(
        "def",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![],
            seen: None,
        },
    );
    // Line 1 is "aé𝄞b": UTF-16 char 1..2 covers é = bytes 1..3.
    std::fs::write(root.join("a.rs"), "x\naé𝄞b\n").unwrap();
    let (sink, rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    wait_ready(&backend);
    run_query(&att, 3, LSP_QUERY_DEFINITION, 0, 0, 0, "a.rs", "", &sink);
    let (status, records) = wait_for(&rx, |msg| {
        query_response(msg)
            .filter(|r| r.nonce == 3)
            .map(|r| (r.status, r.records))
    });
    assert_eq!(status, Status::Ok);
    let locations: Vec<_> = query_records(&records).collect();
    match &locations[..] {
        [
            QueryRecord::Location {
                line,
                column,
                end_column,
                path,
                hash,
                ..
            },
        ] => {
            assert_eq!((*line, *column, *end_column), (1, 1, 3));
            assert_eq!(path, &root.join("a.rs"));
            assert_ne!(*hash, LSP_HASH_NONE);
        }
        other => panic!("unexpected records: {other:?}"),
    }
}

/// yas advertises `definition.linkSupport`, so rust-analyzer and gopls
/// answer with `LocationLink[]` (`targetUri` + `targetSelectionRange`,
/// with `targetRange` the fallback) rather than plain `Location[]`. That
/// is the branch real servers hit, so cover both the selection-range
/// jump target and the fallback when it is absent.
#[test]
fn location_link_uses_selection_range_with_target_fallback() {
    let root = tmp_root("loclink");
    // Line 0 is "x"; line 1 is "aé𝄞b" (é = bytes 1..3 in UTF-8).
    std::fs::write(root.join("a.rs"), "x\naé𝄞b\n").unwrap();
    let serve = |mut reader: BufReader<Box<dyn Read + Send>>, mut writer: Box<dyn Write + Send>| {
        while let Some(msg) = rpc::read_msg(&mut reader) {
            match msg {
                rpc::RpcMsg::Request { id, method, params } => {
                    let reply = match method.as_str() {
                        "initialize" => rpc::response(
                            &id,
                            json!({ "capabilities": {
                                "positionEncoding": "utf-16",
                                "definitionProvider": true,
                                "referencesProvider": true,
                            } }),
                        ),
                        "shutdown" => rpc::response(&id, Value::Null),
                        // targetSelectionRange (UTF-16 1..2 = é) is the
                        // jump target; targetRange spans the whole item.
                        "textDocument/definition" => {
                            let uri = params["textDocument"]["uri"].as_str().unwrap().to_string();
                            rpc::response(
                                &id,
                                json!([ {
                                    "targetUri": uri,
                                    "targetRange": { "start": { "line": 0, "character": 0 },
                                                     "end": { "line": 3, "character": 0 } },
                                    "targetSelectionRange": { "start": { "line": 1, "character": 1 },
                                                              "end": { "line": 1, "character": 2 } },
                                } ]),
                            )
                        }
                        // No targetSelectionRange: targetRange (line 0
                        // "x", UTF-16 0..1) is the jump target.
                        "textDocument/references" => {
                            let uri = params["textDocument"]["uri"].as_str().unwrap().to_string();
                            rpc::response(
                                &id,
                                json!([ {
                                    "targetUri": uri,
                                    "targetRange": { "start": { "line": 0, "character": 0 },
                                                     "end": { "line": 0, "character": 1 } },
                                } ]),
                            )
                        }
                        _ => rpc::error_response(&id, -32601, "unhandled in fake"),
                    };
                    let _ = rpc::write_msg(writer.as_mut(), &reply);
                }
                rpc::RpcMsg::Notification { method, .. } => {
                    if method == "exit" {
                        return;
                    }
                }
                rpc::RpcMsg::Response { .. } => {}
            }
        }
    };
    let backend = testutil::pipe_backend(test_spec(), root.clone(), test_budgets(), serve);
    wait_ready(&backend);
    let att = attach(&root, &backend, 0, dummy_sink());

    // Definition: the selection range transcodes é to bytes 1..3.
    let (sink, rx) = collector();
    run_query(&att, 3, LSP_QUERY_DEFINITION, 0, 0, 0, "a.rs", "", &sink);
    let records = wait_for(&rx, |msg| {
        query_response(msg)
            .filter(|r| r.nonce == 3)
            .map(|r| r.records)
    });
    match &query_records(&records).collect::<Vec<_>>()[..] {
        [
            QueryRecord::Location {
                line,
                column,
                end_line,
                end_column,
                path,
                ..
            },
        ] => {
            assert_eq!((*line, *column, *end_line, *end_column), (1, 1, 1, 3));
            assert_eq!(path, &root.join("a.rs"));
        }
        other => panic!("unexpected definition records: {other:?}"),
    }

    // References: the targetRange fallback covers "x" = bytes 0..1.
    let (sink, rx) = collector();
    run_query(&att, 4, LSP_QUERY_REFERENCES, 0, 0, 0, "a.rs", "", &sink);
    let records = wait_for(&rx, |msg| {
        query_response(msg)
            .filter(|r| r.nonce == 4)
            .map(|r| r.records)
    });
    match &query_records(&records).collect::<Vec<_>>()[..] {
        [
            QueryRecord::Location {
                line,
                column,
                end_line,
                end_column,
                path,
                ..
            },
        ] => {
            assert_eq!((*line, *column, *end_line, *end_column), (0, 0, 0, 1));
            assert_eq!(path, &root.join("a.rs"));
        }
        other => panic!("unexpected references records: {other:?}"),
    }
}

#[test]
fn diagnostics_full_replay_reaches_late_joiner() {
    let root = tmp_root("diag");
    std::fs::write(root.join("a.rs"), "x\naé𝄞b\n").unwrap();
    let uri = crate::text::path_to_uri(&root.join("a.rs"));
    let (root2, backend) = {
        let cfg = FakeCfg {
            encoding: "utf-16",
            after_init: vec![json!({
                "jsonrpc": "2.0",
                "method": "textDocument/publishDiagnostics",
                "params": { "uri": uri, "diagnostics": [ {
                    "range": { "start": { "line": 1, "character": 1 },
                               "end": { "line": 1, "character": 2 } },
                    "severity": 1,
                    "code": "E1",
                    "message": "bad é",
                } ] },
            })],
            seen: None,
        };
        (
            root.clone(),
            testutil::pipe_backend(test_spec(), root.clone(), test_budgets(), fake_server(cfg)),
        )
    };
    let check = |att: &Attachment, rx: &Receiver<Output>| {
        let mut mirror = DiagnosticsMirror::default();
        loop {
            let msg = rx
                .recv_timeout(Duration::from_secs(10))
                .expect("no diag update");
            let Some(update_id) = mirror.apply(&msg) else {
                continue;
            };
            att.ack_native(Stream::Diagnostics, update_id);
            if let Some(file) = mirror.files.get(&root.join("a.rs")) {
                let d = &file.diagnostics[0];
                assert_eq!((d.line, d.column, d.end_column), (1, 1, 3));
                assert_eq!(d.message, "bad é");
                assert_ne!(file.hash, LSP_HASH_NONE);
                return;
            }
        }
    };
    let (sink1, rx1) = collector();
    let att1 = attach(&root2, &backend, DIAGNOSTICS, sink1);
    check(&att1, &rx1);
    // A late joiner gets the same state from the cache replay, without
    // the server republishing.
    let (sink2, rx2) = collector();
    let att2 = attach(&root2, &backend, DIAGNOSTICS, sink2);
    check(&att2, &rx2);
}

/// A frozen (lz4 cold) cache entry is subscriber-indistinguishable
/// from a live one: a late joiner's FULL replay decodes it, and the
/// next publish for the path lands as an ordinary live entry.
#[test]
fn frozen_diag_entry_replays_and_republishes() {
    let root = tmp_root("frozen");
    std::fs::write(root.join("a.rs"), "fn x() {}\n").unwrap();
    let uri = crate::text::path_to_uri(&root.join("a.rs"));
    let publish = move |msg: &str| {
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": { "uri": uri, "diagnostics": [ {
                "range": { "start": { "line": 0, "character": 0 },
                           "end": { "line": 0, "character": 1 } },
                "severity": 1,
                "message": msg,
            } ] },
        })
    };
    // A fake server that publishes "v1" after init and "v2" when a
    // hover query arrives.
    let serve = move |mut reader: BufReader<Box<dyn Read + Send>>,
                      mut writer: Box<dyn Write + Send>| {
        while let Some(msg) = rpc::read_msg(&mut reader) {
            match msg {
                rpc::RpcMsg::Request { id, method, .. } => {
                    let reply = match method.as_str() {
                        "initialize" => rpc::response(
                            &id,
                            json!({
                                "capabilities": {
                                    "positionEncoding": "utf-16",
                                    "hoverProvider": true,
                                },
                                "serverInfo": { "name": "fake" },
                            }),
                        ),
                        _ => rpc::response(&id, Value::Null),
                    };
                    let _ = rpc::write_msg(writer.as_mut(), &reply);
                    if method == "textDocument/hover" {
                        let _ = rpc::write_msg(writer.as_mut(), &publish("v2"));
                    }
                }
                rpc::RpcMsg::Notification { method, .. } => {
                    if method == "initialized" {
                        let _ = rpc::write_msg(writer.as_mut(), &publish("v1"));
                    }
                    if method == "exit" {
                        return;
                    }
                }
                rpc::RpcMsg::Response { .. } => {}
            }
        }
    };
    let backend = testutil::pipe_backend(test_spec(), root.clone(), test_budgets(), serve);
    // Wait for v1 to land, then freeze the entry in place.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "v1 publish never landed");
        if backend
            .shared
            .diags
            .lock()
            .unwrap()
            .contains_key(&root.join("a.rs"))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    {
        let mut diags = backend.shared.diags.lock().unwrap();
        crate::backend::freeze_cold_diags(&mut diags, Duration::ZERO);
        assert!(matches!(
            diags[&root.join("a.rs")].diags,
            crate::backend::Diags::Cold(_)
        ));
    }
    // A late joiner's FULL replay decodes the frozen entry.
    let (sink, rx) = collector();
    let att = attach(&root, &backend, DIAGNOSTICS, sink.clone());
    let mut mirror = DiagnosticsMirror::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(
            Instant::now() < deadline,
            "no FULL replay of the frozen entry"
        );
        let msg = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let Some(update_id) = mirror.apply(&msg) else {
            continue;
        };
        att.ack_native(Stream::Diagnostics, update_id);
        if let Some(file) = mirror.files.get(&root.join("a.rs")) {
            assert_eq!(file.diagnostics[0].message, "v1");
            break;
        }
    }
    // A publish against the frozen entry: hover makes the server
    // republish, the cache entry goes live again, and the subscriber
    // sees the incremental.
    run_query(&att, 1, LSP_QUERY_HOVER, 0, 0, 0, "a.rs", "", &sink);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "v2 publish never landed");
        let msg = rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let Some(update_id) = mirror.apply(&msg) else {
            continue;
        };
        att.ack_native(Stream::Diagnostics, update_id);
        if mirror.files[&root.join("a.rs")].diagnostics[0].message == "v2" {
            break;
        }
    }
    let diags = backend.shared.diags.lock().unwrap();
    let a = &diags[&root.join("a.rs")];
    assert!(matches!(a.diags, crate::backend::Diags::Live(_)));
    assert_eq!(a.diags()[0].msg, "v2");
}

#[test]
fn rename_returns_edit_plan_and_applyedit_is_refused() {
    let (seen_tx, _seen_rx) = std::sync::mpsc::channel();
    let (root, backend) = start(
        "rename",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![json!({
                "jsonrpc": "2.0",
                "id": "FRESH",
                "method": "workspace/applyEdit",
                "params": { "edit": { "changes": {} } },
            })],
            seen: Some(seen_tx),
        },
    );
    std::fs::write(root.join("a.rs"), "x\naé𝄞b\n").unwrap();
    let (sink, rx) = collector();
    let att = attach(&root, &backend, WATCH, sink.clone());
    wait_ready(&backend);
    run_query(&att, 9, LSP_QUERY_RENAME, 0, 1, 3, "a.rs", "renamed", &sink);
    let (status, records) = wait_for(&rx, |msg| {
        query_response(msg)
            .filter(|r| r.nonce == 9)
            .map(|r| (r.status, r.records))
    });
    assert_eq!(status, Status::Ok);
    let edits: Vec<_> = query_records(&records).collect();
    match &edits[..] {
        [
            QueryRecord::Edit {
                line,
                column,
                end_column,
                new_text,
                path,
                ..
            },
        ] => {
            assert_eq!((*line, *column, *end_column), (1, 3, 7));
            assert_eq!(*new_text, "renamed");
            assert_eq!(path, &root.join("a.rs"));
        }
        other => panic!("unexpected records: {other:?}"),
    }
    // The applyEdit sent after initialized was refused and counted.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "refused_edits never surfaced");
        if backend.shared.info.lock().unwrap().refused_edits >= 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn doc_symbols_flatten_with_depth() {
    let (root, backend) = start(
        "sym",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![],
            seen: None,
        },
    );
    std::fs::write(root.join("a.rs"), "struct O;\nfn i() {}\n\n\n").unwrap();
    let (sink, rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    wait_ready(&backend);
    run_query(&att, 5, LSP_QUERY_DOC_SYMBOLS, 0, 0, 0, "a.rs", "", &sink);
    let records = wait_for(&rx, |msg| {
        query_response(msg)
            .filter(|r| r.nonce == 5)
            .map(|r| r.records)
    });
    let symbols: Vec<_> = query_records(&records).collect();
    match &symbols[..] {
        [
            QueryRecord::Symbol {
                name: outer,
                depth: 0,
                symbol_kind: 5,
                ..
            },
            QueryRecord::Symbol {
                name: inner,
                depth: 1,
                symbol_kind: 12,
                ..
            },
        ] => {
            assert_eq!((outer.as_str(), inner.as_str()), ("Outer", "inner"));
        }
        other => panic!("unexpected records: {other:?}"),
    }
}

#[test]
fn child_exit_restarts_with_backoff() {
    // First session dies right after initialize; the respawned one
    // lives.
    let root = tmp_root("restart");
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let attempts2 = attempts.clone();
    let serve = move |mut reader: BufReader<Box<dyn Read + Send>>,
                      mut writer: Box<dyn Write + Send>| {
        let n = attempts2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        while let Some(msg) = rpc::read_msg(&mut reader) {
            if let rpc::RpcMsg::Request { id, method, .. } = msg
                && method == "initialize"
            {
                if n == 0 {
                    return; // die mid-handshake
                }
                let _ = rpc::write_msg(
                    writer.as_mut(),
                    &rpc::response(&id, json!({ "capabilities": {} })),
                );
            }
        }
    };
    let backend = testutil::pipe_backend(test_spec(), root, test_budgets(), serve);
    wait_ready(&backend);
    assert!(attempts.load(std::sync::atomic::Ordering::SeqCst) >= 2);
}

/// A document opened before a crash must be re-`didOpen`ed once the
/// backend comes back — even when the *first* respawn also dies during
/// its handshake, before the open set is ever repopulated. The reopen
/// list must survive that second respawn, not be clobbered by the
/// meanwhile-emptied open set.
#[test]
fn deferred_didopen_survives_a_respawn_that_dies_in_handshake() {
    use std::sync::atomic::{AtomicUsize, Ordering as O};
    let root = tmp_root("reopen");
    std::fs::write(root.join("a.rs"), "fn x() {}\n").unwrap();
    let uri = crate::text::path_to_uri(&root.join("a.rs"));

    let (open_tx, open_rx) = std::sync::mpsc::channel::<String>();
    let attempts = Arc::new(AtomicUsize::new(0));
    let serve = move |mut reader: BufReader<Box<dyn Read + Send>>,
                      mut writer: Box<dyn Write + Send>| {
        let n = attempts.fetch_add(1, O::SeqCst);
        // Session 2 dies mid-handshake: it never answers `initialize`,
        // so it never repopulates the open set.
        if n == 1 {
            return;
        }
        while let Some(msg) = rpc::read_msg(&mut reader) {
            match msg {
                rpc::RpcMsg::Request { id, method, .. } => {
                    match method.as_str() {
                        "initialize" => {
                            let _ = rpc::write_msg(
                                writer.as_mut(),
                                &rpc::response(
                                    &id,
                                    json!({ "capabilities": { "hoverProvider": true } }),
                                ),
                            );
                        }
                        "shutdown" => {
                            let _ =
                                rpc::write_msg(writer.as_mut(), &rpc::response(&id, Value::Null));
                        }
                        // Session 1 dies right after the query has opened
                        // the document, so a.rs is left in the open set.
                        "textDocument/hover" if n == 0 => return,
                        _ => {
                            let _ = rpc::write_msg(
                                writer.as_mut(),
                                &rpc::error_response(&id, -32601, "unhandled"),
                            );
                        }
                    }
                }
                rpc::RpcMsg::Notification { method, params } => {
                    // Only the recovered session's replay is observed.
                    if method == "textDocument/didOpen" && n >= 2 {
                        let sent = params["textDocument"]["uri"].as_str().unwrap_or_default();
                        let _ = open_tx.send(sent.to_string());
                    }
                    if method == "exit" {
                        return;
                    }
                }
                rpc::RpcMsg::Response { .. } => {}
            }
        }
    };
    let backend = testutil::pipe_backend(test_spec(), root.clone(), test_budgets(), serve);
    wait_ready(&backend);

    // Open a.rs via a query, then let session 1 die.
    let (sink, _rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    run_query(&att, 1, LSP_QUERY_HOVER, 0, 0, 0, "a.rs", "", &sink);

    // The third session (after the mid-handshake death of the second)
    // must re-open a.rs from the preserved reopen list.
    let reopened = open_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("a.rs was never re-opened after the double crash");
    assert_eq!(reopened, uri);
    drop(att);
}

/// A queued or in-flight query must always get its one response — even
/// when the backend is stopped underneath it — or the connection's
/// nonce would leak forever (docs/design/lsp.md: one response per
/// nonce in every outcome).
#[test]
fn stop_answers_pending_query() {
    let (root, backend) = start(
        "stopq",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![],
            seen: None,
        },
    );
    std::fs::write(root.join("a.rs"), "fn x() {}\n").unwrap();
    wait_ready(&backend);

    let (tx, rx) = std::sync::mpsc::channel::<native::QueryResponse>();
    let sink: native::QuerySink = Arc::new(move |response| tx.send(response).is_ok());
    backend.send(crate::backend::Cmd::Query {
        sub: 1,
        nonce: 7,
        kind: LSP_QUERY_HOVER,
        flags: 0,
        line: 0,
        col: 0,
        path: Some(root.join("a.rs")),
        arg: String::new(),
        sink,
    });
    backend.send(crate::backend::Cmd::Stop);

    let response = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("pending query answered on stop");
    assert_eq!(response.nonce, 7);

    // Once stopped the backend is terminally gone, and further sends are
    // refused so the attachment can respawn on a later query.
    let deadline = Instant::now() + Duration::from_secs(5);
    while !backend.is_gone() {
        assert!(Instant::now() < deadline, "backend never went gone");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!backend.send(crate::backend::Cmd::Stop));
}

/// A query for a capability the backend does not advertise must answer
/// NOT_FOUND, never a bare OTHER "error" — routing checks the capability
/// before dispatching, so an unsupported request is never sent (the
/// nixd-workspace-symbols case from the field).
#[test]
fn query_without_capability_is_not_found() {
    let root = tmp_root("nocap");
    std::fs::write(root.join("a.rs"), "fn x() {}\n").unwrap();
    // A server advertising only hover — no workspace/document symbols,
    // no definition.
    let serve = |mut reader: BufReader<Box<dyn Read + Send>>, mut writer: Box<dyn Write + Send>| {
        while let Some(msg) = rpc::read_msg(&mut reader) {
            if let rpc::RpcMsg::Request { id, method, .. } = msg
                && method == "initialize"
            {
                let _ = rpc::write_msg(
                    writer.as_mut(),
                    &rpc::response(&id, json!({ "capabilities": { "hoverProvider": true } })),
                );
            }
        }
    };
    let backend = testutil::pipe_backend(test_spec(), root.clone(), test_budgets(), serve);
    wait_ready(&backend);
    let att = attach(&root, &backend, 0, dummy_sink());

    for (nonce, kind, path) in [
        (7, LSP_QUERY_WS_SYMBOLS, ""),
        (8, LSP_QUERY_DEFINITION, "a.rs"),
    ] {
        let (sink, rx) = collector();
        run_query(&att, nonce, kind, 0, 0, 0, path, "", &sink);
        let (n, status) = wait_for(&rx, |m| query_response(m).map(|r| (r.nonce, r.status)));
        assert_eq!(
            (n, status),
            (nonce, Status::NotFound),
            "kind {kind} must be NOT_FOUND, not error"
        );
    }
}

fn dummy_sink() -> Sink {
    Arc::new(|_| true)
}

/// A `needs_open_doc` backend (tsserver's "No Project" quirk) must have
/// a document opened before `workspace/symbol`, or the query fails; yas
/// opens a representative file from the root first.
#[test]
fn ws_symbols_opens_a_project_doc_when_needed() {
    use std::sync::atomic::{AtomicBool, Ordering as O};
    let root = tmp_root("wsproj");
    std::fs::write(root.join("lib.rs"), "fn thing() {}\n").unwrap();
    let opened = Arc::new(AtomicBool::new(false));
    let opened2 = opened.clone();
    let serve = move |mut reader: BufReader<Box<dyn Read + Send>>,
                      mut writer: Box<dyn Write + Send>| {
        while let Some(msg) = rpc::read_msg(&mut reader) {
            match msg {
                rpc::RpcMsg::Request { id, method, .. } => {
                    let reply = match method.as_str() {
                        "initialize" => rpc::response(
                            &id,
                            json!({ "capabilities": { "workspaceSymbolProvider": true } }),
                        ),
                        "shutdown" => rpc::response(&id, Value::Null),
                        "workspace/symbol" if opened2.load(O::Relaxed) => rpc::response(
                            &id,
                            json!([{
                                "name": "thing", "kind": 12,
                                "location": { "uri": "file:///x/lib.rs", "range": {
                                    "start": { "line": 0, "character": 3 },
                                    "end": { "line": 0, "character": 8 } } }
                            }]),
                        ),
                        // No project until a document is open.
                        "workspace/symbol" => rpc::error_response(&id, -32000, "No Project"),
                        _ => rpc::error_response(&id, -32601, "unhandled"),
                    };
                    let _ = rpc::write_msg(writer.as_mut(), &reply);
                }
                rpc::RpcMsg::Notification { method, .. } => {
                    if method == "textDocument/didOpen" {
                        opened2.store(true, O::Relaxed);
                    }
                    if method == "exit" {
                        return;
                    }
                }
                rpc::RpcMsg::Response { .. } => {}
            }
        }
    };
    let mut spec = test_spec();
    spec.needs_open_doc = true;
    let backend = testutil::pipe_backend(spec, root.clone(), test_budgets(), serve);
    wait_ready(&backend);
    let (sink, rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    run_query(&att, 5, LSP_QUERY_WS_SYMBOLS, 0, 0, 0, "", "thing", &sink);
    let records = wait_for(&rx, |m| {
        query_response(m)
            .filter(|r| r.nonce == 5)
            .map(|r| r.records)
    });
    let syms: Vec<_> = query_records(&records).collect();
    assert_eq!(
        syms.len(),
        1,
        "ws-symbols should succeed once a project doc is opened"
    );
}

/// `didChangeWatchedFiles` must relay a creation as `Created` (type 1),
/// a modification as `Changed` (2), and a removal as `Deleted` (3) — not
/// collapse everything to Changed/Deleted by `exists()` alone, or a
/// server that adds files only on `Created` (gopls) misses new files.
#[test]
fn watched_files_carry_the_change_type() {
    let root = tmp_root("watched");
    // Two files that exist before the backend starts, so the real
    // watcher stays quiet and the injected hints drive the test
    // deterministically across platforms.
    std::fs::write(root.join("created.rs"), "fn a() {}\n").unwrap();
    std::fs::write(root.join("changed.rs"), "fn b() {}\n").unwrap();

    let (tx, rx) = std::sync::mpsc::channel::<Value>();
    let serve = move |mut reader: BufReader<Box<dyn Read + Send>>,
                      mut writer: Box<dyn Write + Send>| {
        while let Some(msg) = rpc::read_msg(&mut reader) {
            match msg {
                rpc::RpcMsg::Request { id, method, .. } => {
                    let reply = match method.as_str() {
                        "initialize" => rpc::response(&id, json!({ "capabilities": {} })),
                        "shutdown" => rpc::response(&id, Value::Null),
                        _ => rpc::error_response(&id, -32601, "unhandled"),
                    };
                    let _ = rpc::write_msg(writer.as_mut(), &reply);
                }
                rpc::RpcMsg::Notification { method, params } => {
                    if method == "workspace/didChangeWatchedFiles" {
                        let _ = tx.send(params);
                    }
                    if method == "exit" {
                        return;
                    }
                }
                rpc::RpcMsg::Response { .. } => {}
            }
        }
    };
    let backend = testutil::pipe_backend(test_spec(), root.clone(), test_budgets(), serve);
    wait_ready(&backend);

    let gone = root.join("gone.rs"); // never created → Deleted
    backend.send(crate::backend::Cmd::Dirty(vec![
        (root.join("created.rs"), true),
        (root.join("changed.rs"), false),
        (gone, false),
    ]));

    // Collect changes until all three files are seen.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut types: std::collections::HashMap<String, i64> = Default::default();
    while types.len() < 3 {
        assert!(
            Instant::now() < deadline,
            "watched-files event never arrived"
        );
        let params = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("no didChangeWatchedFiles");
        for change in params["changes"].as_array().into_iter().flatten() {
            let uri = change["uri"].as_str().unwrap_or_default();
            let name = uri.rsplit('/').next().unwrap_or_default().to_string();
            types.insert(name, change["type"].as_i64().unwrap_or(0));
        }
    }
    assert_eq!(types.get("created.rs"), Some(&1), "creation → Created");
    assert_eq!(types.get("changed.rs"), Some(&2), "modification → Changed");
    assert_eq!(types.get("gone.rs"), Some(&3), "missing path → Deleted");
}

fn wait_ready(backend: &Arc<Backend>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "backend never became ready");
        if backend.shared.info.lock().unwrap().phase == LSP_PHASE_READY {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The shared root watcher must drop VCS metadata and dependency/build
/// caches (`UNWATCHED_DIRS`) so a `cargo build` storm never reaches
/// `Cmd::Dirty` or `didChangeWatchedFiles` — but must NOT drop the wider
/// `SKIP_DIRS` set that `ensure_project_doc` avoids. Those two lists
/// answer different questions: skipping `dist/` when *picking* a
/// representative project file is right, skipping it when *watching* is a
/// correctness bug, because plenty of projects keep real sources there.
#[test]
fn watcher_filter_drops_only_never_source_subtrees() {
    let root = Path::new("/w");
    let keep = crate::backend::watched_path;
    assert!(keep(root, Path::new("/w/src/main.rs")));
    assert!(keep(root, Path::new("/w/Cargo.toml")));
    assert!(!keep(root, Path::new("/w/target/debug/build/x.d")));
    assert!(!keep(root, Path::new("/w/web/node_modules/p/index.js")));
    assert!(!keep(root, Path::new("/w/.git/index.lock")));
    assert!(!keep(root, Path::new("/w/.venv/lib/x.py")));
    assert!(!keep(root, Path::new("/w/.direnv/bin/x")));
    // Watched despite being in SKIP_DIRS: a picker avoids these, a
    // watcher must not, or an external edit there never refreshes.
    for build_ish in ["dist", "build", "out", "vendor"] {
        assert!(
            keep(root, &root.join(build_ish).join("app.ts")),
            "{build_ish}/ must still be watched"
        );
    }
    // Paths outside the root pass through (the old .git-only filter
    // kept them too).
    assert!(keep(root, Path::new("/elsewhere/x.rs")));
    // The stat-free filter drops files merely *named* like a skip dir.
    assert!(!keep(root, Path::new("/w/src/target")));
}

/// An empty publish for a path with no cached entry must be a complete
/// no-op — no disk read, no tombstone, no seq bump, no subscriber ping
/// — and a repeated clear with an unchanged hash must not re-insert.
#[test]
fn empty_publishes_skip_tombstones_and_pings() {
    let root = tmp_root("emptypub");
    std::fs::write(root.join("a.rs"), "fn x() {}\n").unwrap();
    std::fs::write(root.join("b.rs"), "fn y() {}\n").unwrap();
    let publish = |name: &str, diags: Value| {
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": crate::text::path_to_uri(&root.join(name)),
                "diagnostics": diags,
            },
        })
    };
    let diag = json!([{
        "range": { "start": { "line": 0, "character": 0 },
                   "end": { "line": 0, "character": 1 } },
        "severity": 1,
        "message": "bad",
    }]);
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        test_budgets(),
        fake_server(FakeCfg {
            encoding: "utf-16",
            after_init: vec![
                // Clear for a path never diagnosed: skipped entirely.
                publish("never.rs", json!([])),
                // Real entry, then its clear (tombstone), then a
                // duplicate clear: the duplicate is skipped.
                publish("a.rs", diag.clone()),
                publish("a.rs", json!([])),
                publish("a.rs", json!([])),
                // Sentinel proving everything above was processed.
                publish("b.rs", diag),
            ],
            seen: None,
        }),
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        assert!(Instant::now() < deadline, "sentinel publish never landed");
        if backend
            .shared
            .diags
            .lock()
            .unwrap()
            .contains_key(&root.join("b.rs"))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let diags = backend.shared.diags.lock().unwrap();
    // never.rs left no tombstone.
    assert!(!diags.contains_key(&root.join("never.rs")));
    // a.rs holds one tombstone (seq 2), not two.
    let a = &diags[&root.join("a.rs")];
    assert!(a.is_empty());
    assert_eq!(a.seq, 2);
    // Seqs: a.rs diag (1), a.rs clear (2), b.rs diag (3) — the skipped
    // publishes never bumped the counter.
    assert_eq!(
        backend
            .shared
            .diag_seq
            .load(std::sync::atomic::Ordering::Relaxed),
        3
    );
}

#[test]
fn completion_translates_sorts_and_flags() {
    let (root, backend) = start(
        "completion",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![],
            seen: None,
        },
    );
    std::fs::write(root.join("a.rs"), "x\naé𝄞b\n").unwrap();
    let (sink, rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    wait_ready(&backend);
    run_query(&att, 3, LSP_QUERY_COMPLETION, 0, 1, 1, "a.rs", "", &sink);
    let (status, incomplete, records) = wait_for(&rx, |msg| {
        query_response(msg)
            .filter(|r| r.nonce == 3)
            .map(|r| (r.status, r.incomplete, r.records))
    });
    assert_eq!(status, Status::Ok);
    assert!(incomplete, "isIncomplete must be preserved");
    let recs: Vec<_> = query_records(&records).collect();
    match &recs[..] {
        [
            QueryRecord::Completion {
                label: l1,
                deprecated,
                preselect,
                snippet,
                insert: i1,
                item_kind: k1,
                ..
            },
            QueryRecord::Completion {
                label: l2,
                insert: i2,
                detail,
                line,
                column,
                end_column,
                ..
            },
        ] => {
            // sortText order, not arrival order: "a" before "b".
            assert_eq!(*l1, "aa_first");
            assert!(*snippet);
            assert!(*preselect);
            assert!(*deprecated, "tag 1 → deprecated");
            assert_eq!(*i1, "aa_first(${1:x})");
            assert_eq!(*k1, 3);
            assert_eq!(*l2, "zz_last");
            // textEdit.newText == label → empty insert.
            assert_eq!(*i2, "");
            assert_eq!(*detail, "u32");
            // UTF-16 units 1..2 on "aé𝄞b" are the é: bytes 1..3.
            assert_eq!((*line, *column, *end_column), (1, 1, 3));
        }
        other => panic!("unexpected records: {other:?}"),
    }
}

#[test]
fn native_completion_context_code_actions_and_formatting_are_projected() {
    let (root, backend) = start(
        "native-actions",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![],
            seen: None,
        },
    );
    std::fs::write(root.join("a.rs"), "x\naé𝄞b\n").unwrap();
    let (sink, rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    wait_ready(&backend);

    // YAS CHARACTER maps to LSP TriggerCharacter (2) and preserves the
    // trigger itself. A successful response proves it was admitted/routed.
    run_query(&att, 30, LSP_QUERY_COMPLETION, 2, 1, 3, "a.rs", ".", &sink);
    let completion_status = wait_for(&rx, |message| {
        query_response(message)
            .filter(|response| response.nonce == 30)
            .map(|response| response.status)
    });
    assert_eq!(completion_status, Status::Ok);

    let action_arg = json!({
        "range": {
            "start": { "line": 1, "character": 1 },
            "end": { "line": 1, "character": 3 }
        },
        "diagnostics": []
    })
    .to_string();
    run_query(
        &att,
        31,
        LSP_QUERY_CODE_ACTIONS,
        0,
        1,
        1,
        "a.rs",
        &action_arg,
        &sink,
    );
    let action_records = wait_for(&rx, |message| {
        query_response(message)
            .filter(|response| response.nonce == 31)
            .map(|response| response.records)
    });
    let action_records = query_records(&action_records).collect::<Vec<_>>();
    assert!(matches!(
        action_records.as_slice(),
        [
            QueryRecord::Action {
                title,
                edit_count: 1,
                preferred,
                ..
            },
            QueryRecord::Edit { column: 1, end_column: 3, new_text, .. }
        ] if title == "Fix spelling" && *preferred && new_text == "e"
    ));

    let formatting_arg = json!({
        "range": Value::Null,
        "options": { "tabSize": 4, "insertSpaces": true }
    })
    .to_string();
    run_query(
        &att,
        32,
        LSP_QUERY_FORMATTING,
        0,
        0,
        0,
        "a.rs",
        &formatting_arg,
        &sink,
    );
    let formatting_records = wait_for(&rx, |message| {
        query_response(message)
            .filter(|response| response.nonce == 32)
            .map(|response| response.records)
    });
    assert!(matches!(
        query_records(&formatting_records)
            .collect::<Vec<_>>()
            .as_slice(),
        [QueryRecord::Edit {
            column: 1,
            end_column: 3,
            new_text,
            ..
        }] if new_text == "E"
    ));
}

#[test]
fn signature_help_active_first_with_param_bytes() {
    let (root, backend) = start(
        "sig",
        FakeCfg {
            encoding: "utf-16",
            after_init: vec![],
            seen: None,
        },
    );
    std::fs::write(root.join("a.rs"), "x\n").unwrap();
    let (sink, rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    wait_ready(&backend);
    run_query(&att, 4, LSP_QUERY_SIGNATURE, 0, 0, 0, "a.rs", "", &sink);
    let (status, records) = wait_for(&rx, |msg| {
        query_response(msg)
            .filter(|r| r.nonce == 4)
            .map(|r| (r.status, r.records))
    });
    assert_eq!(status, Status::Ok);
    let recs: Vec<_> = query_records(&records).collect();
    match &recs[..] {
        [
            QueryRecord::Signature {
                active: first_active,
                active_parameter,
                parameter_start,
                parameter_end,
                label: l1,
                documentation,
            },
            QueryRecord::Signature {
                active: second_active,
                label: l2,
                parameter_start: ps2,
                parameter_end: pe2,
                ..
            },
        ] => {
            // activeSignature 1 is emitted first, flagged ACTIVE.
            assert!(*first_active);
            assert_eq!(*l1, "f(a: 𝄞x)");
            assert_eq!(*active_parameter, Some(0));
            // UTF-16 offsets 5..8 into the label → bytes 5..10.
            assert_eq!((*parameter_start, *parameter_end), (5, 10));
            assert_eq!(*documentation, "docs");
            assert!(!*second_active);
            assert_eq!(*l2, "f()");
            assert_eq!((*ps2, *pe2), (0, 0));
        }
        other => panic!("unexpected records: {other:?}"),
    }
}

/// A minimal server that records every notification (method, params),
/// for observing document sync during buffer-overlay tests.
fn doc_recording_server(
    docs: Sender<(String, Value)>,
) -> impl FnMut(BufReader<Box<dyn Read + Send>>, Box<dyn Write + Send>) + Clone + Send + 'static {
    move |mut reader, mut writer| {
        let docs = docs.clone();
        while let Some(msg) = rpc::read_msg(&mut reader) {
            match msg {
                rpc::RpcMsg::Request { id, method, .. } => {
                    let reply = match method.as_str() {
                        "initialize" => rpc::response(
                            &id,
                            json!({ "capabilities": {
                                "positionEncoding": "utf-8",
                                "definitionProvider": true,
                            } }),
                        ),
                        "shutdown" => rpc::response(&id, Value::Null),
                        _ => rpc::error_response(&id, -32601, "unhandled in fake"),
                    };
                    let _ = rpc::write_msg(writer.as_mut(), &reply);
                }
                rpc::RpcMsg::Notification { method, params } => {
                    if method == "exit" {
                        return;
                    }
                    let _ = docs.send((method, params));
                }
                rpc::RpcMsg::Response { .. } => {}
            }
        }
    }
}

fn wait_doc<T>(
    rx: &Receiver<(String, Value)>,
    mut pick: impl FnMut(&str, &Value) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline
            .checked_duration_since(Instant::now())
            .expect("timed out waiting for doc event");
        let (method, params) = rx.recv_timeout(left).expect("channel closed or timed out");
        if let Some(t) = pick(&method, &params) {
            return t;
        }
    }
}

#[test]
fn buffer_overlay_overrides_disk_until_release() {
    let root = tmp_root("overlay");
    std::fs::write(root.join("a.rs"), "disk v1\n").unwrap();
    let (doc_tx, doc_rx) = std::sync::mpsc::channel();
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        test_budgets(),
        doc_recording_server(doc_tx),
    );
    let (sink, _rx) = collector();
    let att = attach(&root, &backend, 0, sink);
    wait_ready(&backend);
    // The overlay write opens the doc with buffer bytes, not disk.
    att.buffer(&root.join("a.rs"), Some(b"buffer v1\n".to_vec()));
    let text = wait_doc(&doc_rx, |m, p| {
        (m == "textDocument/didOpen")
            .then(|| p["textDocument"]["text"].as_str().unwrap().to_string())
    });
    assert_eq!(text, "buffer v1\n");
    // A disk change while overlaid: watched-files events still flow,
    // but no content didChange (the overlay is the byte source).
    std::fs::write(root.join("a.rs"), "disk v2\n").unwrap();
    backend.send(crate::backend::Cmd::Dirty(vec![(root.join("a.rs"), false)]));
    wait_doc(&doc_rx, |m, _| {
        (m == "workspace/didChangeWatchedFiles").then_some(())
    });
    // Versions are engine-minted and sequential, and a content didChange
    // is written before the watched-files notification — so the next
    // didChange being version 2 with the buffer text proves the disk
    // flush did not slip one in.
    att.buffer(&root.join("a.rs"), Some(b"buffer v2\n".to_vec()));
    let change = |m: &str, p: &Value| {
        (m == "textDocument/didChange").then(|| {
            (
                p["textDocument"]["version"].as_i64().unwrap(),
                p["contentChanges"][0]["text"].as_str().unwrap().to_string(),
            )
        })
    };
    let (version, text) = wait_doc(&doc_rx, change);
    assert_eq!((version, text.as_str()), (2, "buffer v2\n"));
    // Release reverts to disk truth with one didChange.
    att.buffer(&root.join("a.rs"), None);
    let (version, text) = wait_doc(&doc_rx, change);
    assert_eq!((version, text.as_str()), (3, "disk v2\n"));
}

#[test]
fn first_empty_overlay_write_still_syncs() {
    let root = tmp_root("overlay-empty");
    std::fs::write(root.join("a.rs"), "disk\n").unwrap();
    let (doc_tx, doc_rx) = std::sync::mpsc::channel();
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        test_budgets(),
        doc_recording_server(doc_tx),
    );
    let (sink, rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    wait_ready(&backend);
    // Open the doc from disk via a query first.
    run_query(&att, 9, LSP_QUERY_DEFINITION, 0, 0, 0, "a.rs", "", &sink);
    let text = wait_doc(&doc_rx, |m, p| {
        (m == "textDocument/didOpen")
            .then(|| p["textDocument"]["text"].as_str().unwrap().to_string())
    });
    assert_eq!(text, "disk\n");
    wait_for(&rx, |msg| {
        query_response(msg).filter(|r| r.nonce == 9).map(|_| ())
    });
    // The FIRST overlay write happens to be an empty buffer: it must
    // still sync — the open doc holds disk content, and a fresh
    // overlay is never "unchanged".
    att.buffer(&root.join("a.rs"), Some(Vec::new()));
    let (version, text) = wait_doc(&doc_rx, |m, p| {
        (m == "textDocument/didChange").then(|| {
            (
                p["textDocument"]["version"].as_i64().unwrap(),
                p["contentChanges"][0]["text"].as_str().unwrap().to_string(),
            )
        })
    });
    assert_eq!((version, text.as_str()), (2, ""));
}

/// A settled disk write to a handled file is a save. Check-on-save
/// servers (rust-analyzer's flycheck, gopls) rerun their external checker
/// only on didSave — `didChangeWatchedFiles` refreshes their VFS but
/// publishes nothing — so without this their diagnostics stay frozen at
/// whatever the startup check produced, for the life of the backend.
#[test]
fn disk_write_notifies_did_save() {
    let root = tmp_root("didsave");
    std::fs::write(root.join("a.rs"), "v1\n").unwrap();
    let (doc_tx, doc_rx) = std::sync::mpsc::channel();
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        test_budgets(),
        doc_recording_server(doc_tx),
    );
    let (sink, _rx) = collector();
    let _att = attach(&root, &backend, 0, sink);
    wait_ready(&backend);
    std::fs::write(root.join("a.rs"), "v2\n").unwrap();
    backend.send(crate::backend::Cmd::Dirty(vec![(root.join("a.rs"), false)]));
    let uri = wait_doc(&doc_rx, |m, p| {
        (m == "textDocument/didSave")
            .then(|| p["textDocument"]["uri"].as_str().unwrap().to_string())
    });
    assert!(uri.ends_with("/a.rs"), "unexpected didSave uri: {uri}");
}

/// The editor's case: an overlaid document still gets didSave when its
/// bytes land on disk. The overlay suppresses content sync (the buffer is
/// the byte source), but the external checker reads disk, and Ctrl+S is
/// precisely when it should rerun.
#[test]
fn overlaid_doc_still_notifies_did_save() {
    let root = tmp_root("didsave-overlay");
    std::fs::write(root.join("a.rs"), "v1\n").unwrap();
    let (doc_tx, doc_rx) = std::sync::mpsc::channel();
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        test_budgets(),
        doc_recording_server(doc_tx),
    );
    let (sink, _rx) = collector();
    let att = attach(&root, &backend, 0, sink);
    wait_ready(&backend);
    att.buffer(&root.join("a.rs"), Some(b"v2\n".to_vec()));
    wait_doc(&doc_rx, |m, _| (m == "textDocument/didOpen").then_some(()));
    std::fs::write(root.join("a.rs"), "v2\n").unwrap();
    backend.send(crate::backend::Cmd::Dirty(vec![(root.join("a.rs"), false)]));
    wait_doc(&doc_rx, |m, _| (m == "textDocument/didSave").then_some(()));
}

/// A deleted file is not a save — it is a didClose. Sending didSave for a
/// path that no longer exists would ask the checker to read missing bytes.
#[test]
fn deleted_file_does_not_notify_did_save() {
    let root = tmp_root("didsave-delete");
    std::fs::write(root.join("a.rs"), "v1\n").unwrap();
    std::fs::write(root.join("b.rs"), "v1\n").unwrap();
    let (doc_tx, doc_rx) = std::sync::mpsc::channel();
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        test_budgets(),
        doc_recording_server(doc_tx),
    );
    let (sink, _rx) = collector();
    let _att = attach(&root, &backend, 0, sink);
    wait_ready(&backend);
    std::fs::remove_file(root.join("a.rs")).unwrap();
    backend.send(crate::backend::Cmd::Dirty(vec![(root.join("a.rs"), false)]));
    // b.rs's save is the barrier: it is queued after a.rs's flush, so
    // once it arrives, any didSave for a.rs would already have been seen.
    std::fs::write(root.join("b.rs"), "v2\n").unwrap();
    backend.send(crate::backend::Cmd::Dirty(vec![(root.join("b.rs"), false)]));
    let uri = wait_doc(&doc_rx, |m, p| {
        (m == "textDocument/didSave")
            .then(|| p["textDocument"]["uri"].as_str().unwrap().to_string())
    });
    assert!(uri.ends_with("/b.rs"), "didSave for a deleted file: {uri}");
}

#[test]
fn detach_releases_overlays_to_disk() {
    let root = tmp_root("overlay-detach");
    std::fs::write(root.join("a.rs"), "disk v1\n").unwrap();
    let (doc_tx, doc_rx) = std::sync::mpsc::channel();
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        test_budgets(),
        doc_recording_server(doc_tx),
    );
    let (sink, _rx) = collector();
    let att = attach(&root, &backend, 0, sink);
    wait_ready(&backend);
    att.buffer(&root.join("a.rs"), Some(b"buffer\n".to_vec()));
    wait_doc(&doc_rx, |m, _| (m == "textDocument/didOpen").then_some(()));
    // Disconnect (Attachment drop) releases the overlay: the document
    // reverts to disk truth.
    drop(att);
    let text = wait_doc(&doc_rx, |m, p| {
        (m == "textDocument/didChange")
            .then(|| p["contentChanges"][0]["text"].as_str().unwrap().to_string())
    });
    assert_eq!(text, "disk v1\n");
}

#[test]
fn overlaid_doc_pinned_against_eviction() {
    let root = tmp_root("overlay-pin");
    std::fs::write(root.join("a.rs"), "one\n").unwrap();
    std::fs::write(root.join("b.rs"), "two\n").unwrap();
    let budgets = Budgets {
        max_docs: 1,
        ..test_budgets()
    };
    let (doc_tx, doc_rx) = std::sync::mpsc::channel();
    let backend = testutil::pipe_backend(
        test_spec(),
        root.clone(),
        budgets,
        doc_recording_server(doc_tx),
    );
    let (sink, rx) = collector();
    let att = attach(&root, &backend, 0, sink.clone());
    wait_ready(&backend);
    att.buffer(&root.join("a.rs"), Some(b"buffer\n".to_vec()));
    wait_doc(&doc_rx, |m, _| (m == "textDocument/didOpen").then_some(()));
    // Opening a second doc exceeds max_docs = 1, but neither the pinned
    // overlay nor the query's own document may be evicted — the cap
    // yields (bounded instead by max_overlays).
    run_query(&att, 5, LSP_QUERY_DEFINITION, 0, 0, 0, "b.rs", "", &sink);
    wait_for(&rx, |msg| {
        query_response(msg).filter(|r| r.nonce == 5).map(|_| ())
    });
    while let Ok((method, _)) = doc_rx.try_recv() {
        assert_ne!(method, "textDocument/didClose");
    }
}

/// A shell-side edit — `git checkout`, `sed -i`, a formatter — reaches a
/// server that only diagnoses open documents. For those,
/// `workspace/didChangeWatchedFiles` is a no-op, so the watcher hint has
/// to admit the file to the open set or the change is invisible forever.
/// Capable servers must NOT be handed the document: they re-read from disk
/// themselves, and an open doc would make yas authoritative for content
/// it does not own.
#[test]
fn watcher_dirty_opens_docs_only_for_open_doc_servers() {
    fn observed_opens(needs_open_doc: bool) -> Vec<String> {
        let root = tmp_root(if needs_open_doc {
            "dirty-open"
        } else {
            "dirty-capable"
        });
        // Exists before start, so the real watcher stays quiet and the
        // injected hint drives the test deterministically.
        std::fs::write(root.join("touched.rs"), "fn a() {}\n").unwrap();
        // A file this backend does not route for must never be admitted.
        std::fs::write(root.join("notes.md"), "hello\n").unwrap();

        let (tx, rx) = std::sync::mpsc::channel::<String>();
        let serve = move |mut reader: BufReader<Box<dyn Read + Send>>,
                          mut writer: Box<dyn Write + Send>| {
            while let Some(msg) = rpc::read_msg(&mut reader) {
                match msg {
                    rpc::RpcMsg::Request { id, method, .. } => {
                        let reply = match method.as_str() {
                            "initialize" => rpc::response(&id, json!({ "capabilities": {} })),
                            "shutdown" => rpc::response(&id, Value::Null),
                            _ => rpc::error_response(&id, -32601, "unhandled"),
                        };
                        let _ = rpc::write_msg(writer.as_mut(), &reply);
                    }
                    rpc::RpcMsg::Notification { method, params } => {
                        if method == "textDocument/didOpen" {
                            let uri = params["textDocument"]["uri"].as_str().unwrap_or("");
                            let _ = tx.send(uri.to_string());
                        }
                        // Ordering marker: this always follows the batch,
                        // so receiving it means any didOpen already landed.
                        if method == "workspace/didChangeWatchedFiles" {
                            let _ = tx.send("__watched__".into());
                        }
                        if method == "exit" {
                            return;
                        }
                    }
                    rpc::RpcMsg::Response { .. } => {}
                }
            }
        };
        let spec = ServerSpec {
            needs_open_doc,
            ..test_spec()
        };
        let backend = testutil::pipe_backend(spec, root.clone(), test_budgets(), serve);
        wait_ready(&backend);

        backend.send(crate::backend::Cmd::Dirty(vec![
            (root.join("touched.rs"), false),
            (root.join("notes.md"), false),
        ]));

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut opens = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(u) if u == "__watched__" => break,
                Ok(u) => opens.push(u),
                Err(_) => break,
            }
        }
        opens
    }

    let opened = observed_opens(true);
    assert!(
        opened.iter().any(|u| u.ends_with("touched.rs")),
        "an open-doc-only server must be handed the dirty file, got {opened:?}"
    );
    assert!(
        !opened.iter().any(|u| u.ends_with("notes.md")),
        "a file this backend does not route for must not be admitted, got {opened:?}"
    );

    assert!(
        observed_opens(false).is_empty(),
        "a capable server re-reads from disk and must not be handed an open document"
    );
}
