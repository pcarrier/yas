//! Native YAS language-server commands over typed LSP, State, and Transfer
//! families.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use yas_wire::{
    Decode, Encode, Extensions,
    core::Status,
    family,
    fs::Path as FsPath,
    lsp::{
        self, Close, DiagnosticRecord, DocumentTarget, EntityPatch, ListServers, Open, OpenResult,
        PageDelivery, Position, Query, QueryBody, QueryPage, QueryRecord, RemovedEntity,
        RemovedEntityKey, ServerList, ServerRecord, StateEntity, StopServer, Watch,
        WorkspaceSource,
    },
    state::{Phase, RecordKind, StateAck, StateEvent, Unwatch, WatchResult},
};

use crate::{cli::LspCommand, yas_native::NativeClient};

const STATE_CREDIT: u64 = yas_wire::schema::transport::RECOMMENDED_BUFFERED;
const QUERY_CREDIT: u64 = lsp::MAX_QUERY_BYTES as u64;
const MAX_COLLECTED_RECORDS: usize = 65_536;

struct Workspace {
    client: NativeClient,
    handle: u64,
    canonical_root: PathBuf,
}

impl Workspace {
    async fn open(on: Option<&str>, hub: &str, root: &str) -> Result<Self, String> {
        let canonical = client_abs(root);
        let mut client = NativeClient::connect(on, hub).await?;
        let result: OpenResult = client
            .request_typed(
                family::LSP,
                lsp::request_kind::OPEN,
                &Open {
                    source: WorkspaceSource::PlatformPath(path_bytes(&canonical)?),
                    open_mode: yas_wire::schema::lsp::OPEN_AUTO_DISCOVER as u8,
                    diagnostics_settle_ms: 0,
                    language: String::new(),
                    profile: String::new(),
                    initialization_options: Vec::new(),
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        if result.backend_count == 0 {
            let detail = result
                .no_backend_detail()
                .map_err(wire_error)?
                .unwrap_or_else(|| "no matching language server backend".to_string());
            return Err(detail);
        }
        Ok(Self {
            client,
            handle: result.workspace_handle,
            canonical_root: PathBuf::from(String::from_utf8_lossy(&result.canonical_root).as_ref()),
        })
    }

    async fn close(&mut self) -> Result<(), String> {
        self.client
            .request(
                family::LSP,
                lsp::request_kind::CLOSE,
                Close {
                    workspace_handle: self.handle,
                    extensions: Extensions::default(),
                }
                .encode()
                .map_err(wire_error)?,
                true,
            )
            .await
            .map(|_| ())
    }

    fn target(&self, path: &str) -> Result<DocumentTarget, String> {
        let absolute = PathBuf::from(client_abs(path));
        let relative = absolute.strip_prefix(&self.canonical_root).map_err(|_| {
            format!(
                "{} is outside LSP workspace {}",
                absolute.display(),
                self.canonical_root.display()
            )
        })?;
        Ok(DocumentTarget {
            path: fs_path(relative)?,
            document_revision: 0,
            content_hash: [0; 32],
        })
    }

    async fn query(&mut self, body: QueryBody) -> Result<Vec<QueryRecord>, String> {
        let mut cursor = Vec::new();
        let mut records = Vec::new();
        loop {
            let result: QueryPage = self
                .client
                .request_typed(
                    family::LSP,
                    lsp::request_kind::QUERY,
                    &Query {
                        workspace_handle: self.handle,
                        max_records: lsp::MAX_QUERY_RECORDS as u16,
                        cursor: cursor.clone(),
                        initial_receive_credit: QUERY_CREDIT,
                        body: body.clone(),
                        extensions: Extensions::default(),
                    },
                    true,
                )
                .await?;
            let status = Status::from_code(result.query_status);
            if status == Status::NotFound {
                return Ok(records);
            }
            if status != Status::Ok {
                return Err(format!(
                    "LSP query failed with {status:?}: {}",
                    result.detail
                ));
            }
            let typed = match result.delivery {
                PageDelivery::Inline(records) => records,
                PageDelivery::Transfer(descriptor) => self
                    .client
                    .receive_message_transfer(
                        &descriptor,
                        lsp::MAX_QUERY_BYTES as u64,
                        lsp::MAX_QUERY_RECORDS,
                    )
                    .await?
                    .into_iter()
                    .map(|message| lsp::TypedRecord::decode_message(&message).map_err(wire_error))
                    .collect::<Result<Vec<_>, _>>()?,
            };
            for record in typed {
                if let Some(record) = QueryRecord::decode_typed(&record).map_err(wire_error)? {
                    records.push(record);
                    if records.len() > MAX_COLLECTED_RECORDS {
                        return Err("LSP query exceeded the CLI record limit; narrow it".into());
                    }
                }
            }
            cursor = result.next_cursor;
            if cursor.is_empty() {
                return Ok(records);
            }
        }
    }
}

pub(crate) async fn dispatch(
    on: Option<&str>,
    hub: &str,
    command: LspCommand,
) -> Result<i32, String> {
    match command {
        LspCommand::Def { spec, root, json } => {
            query_position(on, hub, root, spec, PositionQuery::Definition, json).await
        }
        LspCommand::Refs {
            spec,
            declaration,
            root,
            json,
        } => {
            query_position(
                on,
                hub,
                root,
                spec,
                PositionQuery::References(declaration),
                json,
            )
            .await
        }
        LspCommand::Hover { spec, root, json } => {
            query_position(on, hub, root, spec, PositionQuery::Hover, json).await
        }
        LspCommand::Complete { spec, root, json } => {
            query_position(on, hub, root, spec, PositionQuery::Completion, json).await
        }
        LspCommand::Signature { spec, root, json } => {
            query_position(on, hub, root, spec, PositionQuery::Signature, json).await
        }
        LspCommand::Rename {
            spec,
            new_name,
            root,
            json,
        } => query_position(on, hub, root, spec, PositionQuery::Rename(new_name), json).await,
        LspCommand::Symbols {
            query,
            file,
            root,
            json,
        } => cmd_symbols(on, hub, root, query, file, json).await,
        LspCommand::Diagnostics {
            path,
            watch,
            wait,
            root,
            json,
        } => cmd_diagnostics(on, hub, root, path, watch, wait, json).await,
        LspCommand::Wait { root, timeout } => cmd_wait(on, hub, root, timeout).await,
        LspCommand::List { json } => cmd_list(on, hub, json).await,
        LspCommand::Stop { server_handle } => cmd_stop(on, hub, server_handle).await,
    }
}

enum PositionQuery {
    Definition,
    References(bool),
    Hover,
    Completion,
    Signature,
    Rename(String),
}

async fn query_position(
    on: Option<&str>,
    hub: &str,
    root: String,
    spec: String,
    query: PositionQuery,
    json: bool,
) -> Result<i32, String> {
    let (path, position) = parse_spec(&spec)?;
    let mut workspace = Workspace::open(on, hub, &root).await?;
    let target = workspace.target(&path)?;
    let body = match query {
        PositionQuery::Definition => QueryBody::Definition { target, position },
        PositionQuery::References(declaration) => QueryBody::References {
            target,
            position,
            flags: if declaration {
                yas_wire::schema::lsp::REFERENCES_INCLUDE_DECLARATION as u16
            } else {
                0
            },
        },
        PositionQuery::Hover => QueryBody::Hover { target, position },
        PositionQuery::Completion => QueryBody::Completion {
            target,
            position,
            trigger_kind: yas_wire::schema::lsp::COMPLETION_TRIGGER_INVOKED as u8,
            trigger: String::new(),
        },
        PositionQuery::Signature => QueryBody::SignatureHelp { target, position },
        PositionQuery::Rename(new_name) => QueryBody::Rename {
            target,
            position,
            new_name,
        },
    };
    let records = workspace.query(body).await?;
    for record in &records {
        print_query_record(record, json);
    }
    workspace.close().await?;
    Ok(if records.is_empty() { 1 } else { 0 })
}

async fn cmd_symbols(
    on: Option<&str>,
    hub: &str,
    root: String,
    query: Option<String>,
    file: Option<String>,
    json: bool,
) -> Result<i32, String> {
    let mut workspace = Workspace::open(on, hub, &root).await?;
    let body = if let Some(file) = file {
        QueryBody::DocumentSymbols {
            target: workspace.target(&file)?,
        }
    } else {
        let query = query.unwrap_or_default();
        QueryBody::WorkspaceSymbols { query }
    };
    let records = workspace.query(body).await?;
    let mut count = 0;
    for record in &records {
        if matches!(record, QueryRecord::Symbol(_)) {
            print_query_record(record, json);
            count += 1;
        }
    }
    workspace.close().await?;
    Ok(if count == 0 { 1 } else { 0 })
}

async fn cmd_list(on: Option<&str>, hub: &str, json: bool) -> Result<i32, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let servers: ServerList = client
        .request_typed(
            family::LSP,
            lsp::request_kind::LIST_SERVERS,
            &ListServers {
                workspace_handle: 0,
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    print_servers(&servers.servers, json);
    Ok(0)
}

async fn cmd_stop(on: Option<&str>, hub: &str, server_handle: u64) -> Result<i32, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let servers: ServerList = client
        .request_typed(
            family::LSP,
            lsp::request_kind::LIST_SERVERS,
            &ListServers {
                workspace_handle: 0,
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    let server = servers
        .servers
        .iter()
        .find(|server| server.server_handle == server_handle)
        .ok_or_else(|| format!("language server handle {server_handle} not found"))?;
    client
        .request(
            family::LSP,
            lsp::request_kind::STOP_SERVER,
            StopServer {
                server_handle: server.server_handle,
                generation: server.generation,
                operation_id: nonzero_operation_id(),
                extensions: Extensions::default(),
            }
            .encode()
            .map_err(wire_error)?,
            true,
        )
        .await?;
    Ok(0)
}

async fn cmd_wait(
    on: Option<&str>,
    hub: &str,
    root: String,
    timeout_seconds: u64,
) -> Result<i32, String> {
    let mut workspace = Workspace::open(on, hub, &root).await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_seconds);
    loop {
        let servers = list_workspace_servers(&mut workspace).await?;
        if let Some(failed) = servers
            .iter()
            .find(|server| server.phase == yas_wire::schema::lsp::SERVER_FAILED as u8)
        {
            return Err(format!(
                "{} failed: {}",
                failed.backend_id, failed.last_message
            ));
        }
        if !servers.is_empty()
            && servers
                .iter()
                .all(|server| server.phase == yas_wire::schema::lsp::SERVER_READY as u8)
        {
            workspace.close().await?;
            return Ok(0);
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(124);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn list_workspace_servers(workspace: &mut Workspace) -> Result<Vec<ServerRecord>, String> {
    let result: ServerList = workspace
        .client
        .request_typed(
            family::LSP,
            lsp::request_kind::LIST_SERVERS,
            &ListServers {
                workspace_handle: workspace.handle,
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    Ok(result.servers)
}

async fn cmd_diagnostics(
    on: Option<&str>,
    hub: &str,
    root: String,
    path: Option<String>,
    watch_forever: bool,
    wait: bool,
    json: bool,
) -> Result<i32, String> {
    let mut workspace = Workspace::open(on, hub, &root).await?;
    if wait {
        wait_workspace_ready(&mut workspace, Duration::from_secs(600)).await?;
    }
    let filter = path
        .as_deref()
        .map(|path| {
            workspace
                .target(path)
                .map(|target| display_path(&target.path))
        })
        .transpose()?;
    let result: WatchResult = workspace
        .client
        .request_typed(
            family::LSP,
            lsp::request_kind::WATCH,
            &Watch {
                workspace_handle: workspace.handle,
                datasets: yas_wire::schema::lsp::WATCH_DIAGNOSTICS as u16,
                state: yas_wire::state::Watch {
                    initial_credit: STATE_CREDIT,
                    resume: None,
                    extensions: Extensions::default(),
                },
            },
            true,
        )
        .await?;
    let mut diagnostics = BTreeMap::<String, DiagnosticRecord>::new();
    let mut snapshot_done = false;
    let mut cumulative_credit = STATE_CREDIT;
    loop {
        let frame = workspace
            .client
            .next_matching_event(family::LSP, lsp::event_kind::STATE)
            .await?;
        let event = StateEvent::decode(&frame.payload).map_err(wire_error)?;
        if event.subscription_id != result.subscription_id {
            continue;
        }
        if matches!(event.phase, Phase::SnapshotBegin | Phase::Reset) {
            diagnostics.clear();
            snapshot_done = false;
        }
        for record in &event.records {
            match record.kind {
                RecordKind::Add | RecordKind::Replace => {
                    if let StateEntity::Diagnostics(value) =
                        StateEntity::from_state_record(record).map_err(wire_error)?
                    {
                        diagnostics.insert(display_path(&value.path), value);
                    }
                }
                RecordKind::Patch => {
                    let patch = EntityPatch::decode(&record.body).map_err(wire_error)?;
                    if let StateEntity::Diagnostics(value) = patch.replacement {
                        diagnostics.insert(display_path(&value.path), value);
                    }
                }
                RecordKind::Remove => {
                    let removed = RemovedEntity::decode(&record.body).map_err(wire_error)?;
                    if let RemovedEntityKey::Diagnostics { path } = removed.key {
                        diagnostics.remove(&display_path(&path));
                    }
                }
                RecordKind::Family(_) if record.required => {
                    return Err("LSP sent an unsupported required State record".into());
                }
                _ => {}
            }
        }
        cumulative_credit = cumulative_credit.saturating_add(frame.payload.len() as u64);
        workspace
            .client
            .send_typed_event(
                family::LSP,
                lsp::event_kind::STATE_ACK,
                &StateAck {
                    subscription_id: result.subscription_id,
                    applied_revision: event.to_revision,
                    cumulative_byte_limit: cumulative_credit,
                },
                false,
            )
            .await?;
        if event.phase == Phase::Reset {
            continue;
        }
        if event.phase == Phase::SnapshotEnd || (snapshot_done && event.phase == Phase::Delta) {
            snapshot_done = true;
            let count = print_diagnostics(&diagnostics, filter.as_deref(), json);
            if !watch_forever {
                workspace
                    .client
                    .request(
                        family::LSP,
                        lsp::request_kind::UNWATCH,
                        Unwatch {
                            subscription_id: result.subscription_id,
                        }
                        .encode()
                        .map_err(wire_error)?,
                        true,
                    )
                    .await?;
                workspace.close().await?;
                return Ok(if count == 0 { 0 } else { 1 });
            }
        }
    }
}

async fn wait_workspace_ready(workspace: &mut Workspace, timeout: Duration) -> Result<(), String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let servers = list_workspace_servers(workspace).await?;
        if !servers.is_empty()
            && servers
                .iter()
                .all(|server| server.phase == yas_wire::schema::lsp::SERVER_READY as u8)
        {
            return Ok(());
        }
        if let Some(failed) = servers
            .iter()
            .find(|server| server.phase == yas_wire::schema::lsp::SERVER_FAILED as u8)
        {
            return Err(format!(
                "{} failed: {}",
                failed.backend_id, failed.last_message
            ));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("timed out waiting for language servers".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn print_query_record(record: &QueryRecord, json: bool) {
    match record {
        QueryRecord::Location(value) => {
            let path = display_path(&value.path);
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "location", "path": path,
                        "line": value.range.start.line + 1,
                        "col": value.range.start.byte_column + 1,
                        "endLine": value.range.end.line + 1,
                        "endCol": value.range.end.byte_column + 1,
                    })
                );
            } else {
                println!(
                    "{path}:{}:{}",
                    value.range.start.line + 1,
                    value.range.start.byte_column + 1
                );
            }
        }
        QueryRecord::Hover(value) => {
            let text = String::from_utf8_lossy(&value.content);
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "hover",
                        "format": if value.markup_kind == yas_wire::schema::lsp::MARKUP_MARKDOWN as u8 { "markdown" } else { "plaintext" },
                        "text": text,
                    })
                );
            } else {
                println!("{text}");
            }
        }
        QueryRecord::Symbol(value) => {
            let path = value.path.as_ref().map(display_path).unwrap_or_default();
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "symbol", "name": value.name,
                        "kind": symbol_kind(value.symbol_kind), "depth": value.depth,
                        "path": path, "line": value.range.start.line + 1,
                        "col": value.range.start.byte_column + 1,
                    })
                );
            } else {
                println!(
                    "{}{} {} — {path}:{}:{}",
                    "  ".repeat(value.depth as usize),
                    symbol_kind(value.symbol_kind),
                    value.name,
                    value.range.start.line + 1,
                    value.range.start.byte_column + 1
                );
            }
        }
        QueryRecord::Completion(value) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "completion", "label": value.label,
                        "kind": value.item_kind, "detail": value.detail,
                        "insert": String::from_utf8_lossy(&value.insert_text),
                    })
                );
            } else {
                println!("{}\t{}\t{}", value.label, value.item_kind, value.detail);
            }
        }
        QueryRecord::Edit(value) => {
            let path = display_path(&value.path);
            let replacement = String::from_utf8_lossy(&value.replacement);
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "edit", "path": path,
                        "line": value.range.start.line + 1,
                        "col": value.range.start.byte_column + 1,
                        "endLine": value.range.end.line + 1,
                        "endCol": value.range.end.byte_column + 1,
                        "newText": replacement,
                    })
                );
            } else {
                println!(
                    "{path}:{}:{}-{}:{} -> {replacement}",
                    value.range.start.line + 1,
                    value.range.start.byte_column + 1,
                    value.range.end.line + 1,
                    value.range.end.byte_column + 1
                );
            }
        }
        QueryRecord::Signature(value) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "signature", "label": value.label,
                        "activeParam": value.active_parameter,
                        "paramStart": value.parameter_start,
                        "paramEnd": value.parameter_end,
                        "doc": value.documentation,
                    })
                );
            } else {
                println!("{}", value.label);
                if !value.documentation.is_empty() {
                    println!("{}", value.documentation);
                }
            }
        }
        QueryRecord::Action(value) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "action", "title": value.title,
                        "kind": value.kind, "editCount": value.edits.len(),
                        "disabledReason": value.disabled_reason,
                    })
                );
            } else {
                println!(
                    "{}\t{}\t{} edits",
                    value.title,
                    value.kind,
                    value.edits.len()
                );
            }
        }
    }
}

fn print_servers(servers: &[ServerRecord], json: bool) {
    if !json {
        println!("HANDLE\tLANGUAGE\tPROFILE\tPHASE\tPROGRESS\tRSS\tBACKEND\tMESSAGE");
    }
    for server in servers {
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "handle": server.server_handle, "generation": server.generation,
                    "language": server.language, "profile": server.profile,
                    "phase": phase_name(server.phase), "progress": server.progress_pct,
                    "rss": server.rss_bytes, "backend": server.backend_id,
                    "message": server.last_message,
                })
            );
        } else {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                server.server_handle,
                server.language,
                server.profile,
                phase_name(server.phase),
                server.progress_pct,
                server.rss_bytes,
                server.backend_id,
                server.last_message
            );
        }
    }
}

fn nonzero_operation_id() -> [u8; 16] {
    loop {
        let value: [u8; 16] = rand::random();
        if value != [0; 16] {
            return value;
        }
    }
}

fn print_diagnostics(
    records: &BTreeMap<String, DiagnosticRecord>,
    filter: Option<&str>,
    json: bool,
) -> usize {
    let mut count = 0;
    for (path, record) in records {
        if filter.is_some_and(|filter| path != filter && !path.starts_with(&format!("{filter}/"))) {
            continue;
        }
        for diagnostic in &record.diagnostics {
            count += 1;
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "type": "diagnostic", "path": path,
                        "line": diagnostic.range.start.line + 1,
                        "col": diagnostic.range.start.byte_column + 1,
                        "severity": severity_name(diagnostic.severity),
                        "code": diagnostic.code, "source": diagnostic.source,
                        "message": diagnostic.message,
                    })
                );
            } else {
                println!(
                    "{path}:{}:{}: {}: {}{}",
                    diagnostic.range.start.line + 1,
                    diagnostic.range.start.byte_column + 1,
                    severity_name(diagnostic.severity),
                    if diagnostic.code.is_empty() {
                        String::new()
                    } else {
                        format!("[{}] ", diagnostic.code)
                    },
                    diagnostic.message
                );
            }
        }
    }
    count
}

fn parse_spec(spec: &str) -> Result<(String, Position), String> {
    let invalid = || format!("expected PATH:LINE:COL, got {spec}");
    let (rest, column) = spec.rsplit_once(':').ok_or_else(invalid)?;
    let (path, line) = rest.rsplit_once(':').ok_or_else(invalid)?;
    let line = line.parse::<u32>().map_err(|_| invalid())?;
    let column = column.parse::<u32>().map_err(|_| invalid())?;
    if path.is_empty() || line == 0 || column == 0 {
        return Err(invalid());
    }
    Ok((
        client_abs(path),
        Position {
            line: line - 1,
            byte_column: column - 1,
        },
    ))
}

fn client_abs(path: &str) -> String {
    let absolute = std::path::absolute(path).unwrap_or_else(|_| PathBuf::from(path));
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized.to_string_lossy().into_owned()
}

fn path_bytes(path: &str) -> Result<Vec<u8>, String> {
    if path.as_bytes().contains(&0) {
        Err("path contains NUL".into())
    } else {
        Ok(path.as_bytes().to_vec())
    }
}

fn fs_path(path: &Path) -> Result<FsPath, String> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_string_lossy().as_bytes().to_vec();
                if value.is_empty() || value.contains(&0) {
                    return Err("invalid YAS path component".into());
                }
                components.push(value);
            }
            Component::CurDir => {}
            _ => {
                return Err(format!(
                    "path must be workspace-relative: {}",
                    path.display()
                ));
            }
        }
    }
    if components.is_empty() {
        return Err("document path cannot name the workspace root".into());
    }
    Ok(FsPath { components })
}

fn display_path(path: &FsPath) -> String {
    path.components
        .iter()
        .map(|component| String::from_utf8_lossy(component))
        .collect::<Vec<_>>()
        .join("/")
}

fn phase_name(phase: u8) -> &'static str {
    match phase {
        value if value == yas_wire::schema::lsp::SERVER_SPAWNING as u8 => "spawning",
        value if value == yas_wire::schema::lsp::SERVER_INITIALIZING as u8 => "initializing",
        value if value == yas_wire::schema::lsp::SERVER_INDEXING as u8 => "indexing",
        value if value == yas_wire::schema::lsp::SERVER_READY as u8 => "ready",
        value if value == yas_wire::schema::lsp::SERVER_FAILED as u8 => "failed",
        _ => "unknown",
    }
}

fn severity_name(severity: u8) -> &'static str {
    match severity {
        value if value == yas_wire::schema::lsp::DIAGNOSTIC_ERROR as u8 => "error",
        value if value == yas_wire::schema::lsp::DIAGNOSTIC_WARNING as u8 => "warning",
        value if value == yas_wire::schema::lsp::DIAGNOSTIC_INFORMATION as u8 => "info",
        value if value == yas_wire::schema::lsp::DIAGNOSTIC_HINT as u8 => "hint",
        _ => "unknown",
    }
}

fn symbol_kind(kind: u16) -> &'static str {
    const NAMES: [&str; 26] = [
        "file",
        "module",
        "namespace",
        "package",
        "class",
        "method",
        "property",
        "field",
        "constructor",
        "enum",
        "interface",
        "function",
        "variable",
        "constant",
        "string",
        "number",
        "boolean",
        "array",
        "object",
        "key",
        "null",
        "enum-member",
        "struct",
        "event",
        "operator",
        "type-parameter",
    ];
    NAMES.get(kind as usize).copied().unwrap_or("unknown")
}

fn wire_error(error: impl std::fmt::Display) -> String {
    format!("invalid YAS LSP payload: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_specs_are_one_based_at_the_cli_boundary() {
        let (_, position) = parse_spec("src/main.rs:10:4").unwrap();
        assert_eq!(position.line, 9);
        assert_eq!(position.byte_column, 3);
        assert!(parse_spec("src/main.rs:0:1").is_err());
    }

    #[test]
    fn fs_paths_reject_escape_components() {
        assert_eq!(
            display_path(&fs_path(Path::new("src/main.rs")).unwrap()),
            "src/main.rs"
        );
        assert!(fs_path(Path::new("../outside.rs")).is_err());
    }
}
