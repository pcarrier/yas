//! Native YAS filesystem commands.
//!
//! Every entry point in this module speaks the typed FS, State, and Transfer
//! families over one negotiated YAS session.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};

use yas_wire::{
    Decode, Encode, Extensions,
    core::{ResultPrefix, Status},
    family,
    fs::{
        self, Apply, ApplyItem, ApplyResult, Close, Commit, CommitResult, ConflictDetail,
        ContentResult, EntryBody, EntryRecord, Fetch, Grep, Open, OpenResult, PageDelivery, Path,
        Precondition, QueryGrepFileRecord, QueryPage, QueryRecord, QueryRecordBatch, RootSource,
        Search, StageWrite, StageWriteResult, StateMutation, Watch,
    },
    state::{Phase, StateAck, StateEvent, Unwatch, WatchResult},
};

use crate::{cli::FsCommand, yas_native::NativeClient};

const STATE_CREDIT: u64 = yas_wire::schema::transport::RECOMMENDED_BUFFERED;
const QUERY_CREDIT: u64 = fs::MAX_QUERY_BYTES as u64;

struct Root {
    client: NativeClient,
    handle: u64,
}

impl Root {
    async fn open(on: Option<&str>, hub: &str, path: &str, writable: bool) -> Result<Self, String> {
        let mut client = NativeClient::connect(on, hub).await?;
        let opened: OpenResult = client
            .request_typed(
                family::FS,
                fs::request_kind::OPEN,
                &Open {
                    flags: if writable {
                        0
                    } else {
                        yas_wire::schema::fs::OPEN_READ_ONLY as u16
                    },
                    source: RootSource::PlatformPath(client_abs(path).into_bytes()),
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        Ok(Self {
            client,
            handle: opened.root_handle,
        })
    }

    async fn close(&mut self) -> Result<(), String> {
        self.client
            .request(
                family::FS,
                fs::request_kind::CLOSE,
                Close {
                    root_handle: self.handle,
                    extensions: Extensions::default(),
                }
                .encode()
                .map_err(wire_error)?,
                true,
            )
            .await
            .map(|_| ())
    }
}

pub(crate) async fn dispatch(
    on: Option<&str>,
    hub: &str,
    command: FsCommand,
) -> Result<i32, String> {
    match command {
        FsCommand::Sync {
            path,
            content,
            no_recursive,
            gitignore,
            dot_ignore,
            ignore,
            exclude_git,
            exclude,
            once,
            json,
        } => {
            cmd_sync(
                on,
                hub,
                path,
                SyncOptions {
                    content,
                    no_recursive,
                    gitignore,
                    dot_ignore,
                    ignore,
                    exclude_git,
                    exclude,
                    once,
                    json,
                },
            )
            .await
        }
        FsCommand::Write {
            path,
            root,
            if_hash,
            create,
            force,
            parents,
            durable,
            mode,
            json,
        } => {
            cmd_write(
                on, hub, path, root, if_hash, create, force, parents, durable, mode, json,
            )
            .await
        }
        FsCommand::Mkdir {
            path,
            root,
            parents,
            mode,
            json,
        } => cmd_mkdir(on, hub, path, root, parents, mode, json).await,
        FsCommand::Rm {
            path,
            root,
            if_hash,
            json,
        } => cmd_rm(on, hub, path, root, if_hash, json).await,
        FsCommand::Mv {
            from,
            to,
            root,
            parents,
            json,
        } => cmd_mv(on, hub, from, to, root, parents, json).await,
        FsCommand::Ln {
            target,
            link,
            symlink,
            root,
            if_hash,
            force,
            parents,
            json,
        } => {
            cmd_ln(
                on, hub, target, link, symlink, root, if_hash, force, parents, json,
            )
            .await
        }
        FsCommand::Grep {
            pattern,
            root,
            regex,
            case_sensitive,
            word,
            no_ignore,
            max_matches,
            files_with_matches,
            json,
        } => {
            cmd_grep(
                on,
                hub,
                pattern,
                root,
                regex,
                case_sensitive,
                word,
                no_ignore,
                max_matches,
                files_with_matches,
                json,
            )
            .await
        }
        FsCommand::Cat { path, root } => cmd_cat(on, hub, path, root).await,
        FsCommand::Find {
            query,
            root,
            limit,
            json,
        } => cmd_find(on, hub, query, root, limit, json).await,
    }
}

struct SyncOptions {
    content: bool,
    no_recursive: bool,
    gitignore: bool,
    dot_ignore: bool,
    ignore: bool,
    exclude_git: bool,
    exclude: Vec<String>,
    once: bool,
    json: bool,
}

async fn cmd_sync(
    on: Option<&str>,
    hub: &str,
    path: String,
    options: SyncOptions,
) -> Result<i32, String> {
    let mut root = Root::open(on, hub, &path, false).await?;
    let mut flags = yas_wire::schema::fs::WATCH_INCLUDE_HIDDEN as u16;
    if !options.no_recursive {
        flags |= yas_wire::schema::fs::WATCH_RECURSIVE as u16;
    }
    if options.content {
        flags |= yas_wire::schema::fs::WATCH_CONTENT as u16;
    }
    if options.gitignore || options.ignore {
        flags |= yas_wire::schema::fs::WATCH_GITIGNORE as u16;
    }
    if options.dot_ignore || options.ignore {
        flags |= yas_wire::schema::fs::WATCH_DOT_IGNORE as u16;
    }
    if options.exclude_git || options.ignore {
        flags |= yas_wire::schema::fs::WATCH_EXCLUDE_GIT as u16;
    }
    let watched: WatchResult = root
        .client
        .request_typed(
            family::FS,
            fs::request_kind::WATCH,
            &Watch {
                root_handle: root.handle,
                flags,
                settle_ms: 0,
                inline_max: if options.content {
                    fs::MAX_INLINE_BYTES as u32
                } else {
                    0
                },
                ignore_patterns: options.exclude.join("\n"),
                state: yas_wire::state::Watch {
                    initial_credit: STATE_CREDIT,
                    resume: None,
                    extensions: Extensions::default(),
                },
            },
            true,
        )
        .await?;

    if options.json {
        println!(
            "{}",
            serde_json::json!({
                "type": "synced",
                "subscription_id": watched.subscription_id,
                "root": client_abs(&path),
            })
        );
    } else {
        eprintln!("syncing {}", client_abs(&path));
    }

    let mut entries = BTreeMap::<Path, EntryRecord>::new();
    let mut ready = false;
    let mut cumulative_credit = STATE_CREDIT;
    loop {
        let frame = root
            .client
            .next_matching_event(family::FS, fs::event_kind::STATE)
            .await?;
        if !frame.header.sensitive {
            return Err("FS STATE event was not marked sensitive".into());
        }
        let event = StateEvent::decode_with(
            &frame.payload,
            0,
            &[yas_wire::schema::fs::RECORD_MOVE as u16],
        )
        .map_err(wire_error)?;
        if event.subscription_id != watched.subscription_id {
            continue;
        }
        if matches!(event.phase, Phase::SnapshotBegin | Phase::Reset) {
            entries.clear();
            ready = false;
            if options.json {
                println!("{}", serde_json::json!({"type": "reset"}));
            }
        }
        for record in &event.records {
            let mutation = StateMutation::decode_record(record).map_err(wire_error)?;
            apply_state_mutation(&mut entries, mutation, ready || options.json, options.json);
        }

        cumulative_credit = cumulative_credit.saturating_add(frame.payload.len() as u64);
        root.client
            .send_typed_event(
                family::FS,
                fs::event_kind::STATE_ACK,
                &StateAck {
                    subscription_id: watched.subscription_id,
                    applied_revision: event.to_revision,
                    cumulative_byte_limit: cumulative_credit,
                },
                false,
            )
            .await?;

        if event.phase == Phase::Reset {
            // The server follows a Reset with a fresh snapshot. Keep the
            // subscription and mirror alive so a transient restage does not
            // terminate a long-running sync.
            continue;
        }
        if event.phase == Phase::SnapshotEnd && !ready {
            ready = true;
            if options.json {
                println!(
                    "{}",
                    serde_json::json!({"type": "sync", "entries": entries.len()})
                );
            } else {
                print_snapshot(&entries);
            }
            if options.once {
                root.client
                    .request(
                        family::FS,
                        fs::request_kind::UNWATCH,
                        Unwatch {
                            subscription_id: watched.subscription_id,
                        }
                        .encode()
                        .map_err(wire_error)?,
                        true,
                    )
                    .await?;
                root.close().await?;
                return Ok(0);
            }
            if !options.json {
                eprintln!("watching for changes (ctrl-c to stop)…");
            }
        }
    }
}

fn apply_state_mutation(
    entries: &mut BTreeMap<Path, EntryRecord>,
    mutation: StateMutation,
    print: bool,
    json: bool,
) {
    match mutation {
        StateMutation::Complete(entry) => {
            let existed = entries.contains_key(&entry.path);
            if print {
                print_entry_change(&entry, existed, json);
            }
            entries.insert(entry.path.clone(), entry);
        }
        StateMutation::Patch(patch) => {
            if print {
                print_entry_change(&patch.replacement, true, json);
            }
            entries.insert(patch.path, patch.replacement);
        }
        StateMutation::Remove(removed) => {
            let keys = subtree_keys(entries, &removed.path);
            for key in keys {
                entries.remove(&key);
            }
            if print {
                print_remove(&removed.path, json);
            }
        }
        StateMutation::Move(moved) => {
            let keys = subtree_keys(entries, &moved.from);
            let mut replacements = Vec::with_capacity(keys.len());
            for key in keys {
                if let Some(mut entry) = entries.remove(&key) {
                    let suffix = &key.components[moved.from.components.len()..];
                    let mut components = moved.to.components.clone();
                    components.extend_from_slice(suffix);
                    entry.path = Path { components };
                    replacements.push(entry);
                }
            }
            for entry in replacements {
                entries.insert(entry.path.clone(), entry);
            }
            if print {
                print_move(&moved.from, &moved.to, json);
            }
        }
    }
}

fn subtree_keys(entries: &BTreeMap<Path, EntryRecord>, prefix: &Path) -> Vec<Path> {
    entries
        .keys()
        .filter(|path| path.components.starts_with(&prefix.components))
        .cloned()
        .collect()
}

#[allow(clippy::too_many_arguments)]
async fn cmd_write(
    on: Option<&str>,
    hub: &str,
    path: String,
    root_path: String,
    if_hash: Option<String>,
    create: bool,
    force: bool,
    parents: bool,
    durable: bool,
    mode: Option<String>,
    json: bool,
) -> Result<i32, String> {
    let mut content = Vec::new();
    std::io::stdin()
        .read_to_end(&mut content)
        .map_err(|error| format!("reading stdin: {error}"))?;
    if content.len() as u64 > yas_wire::schema::fs::MAX_STAGED_BYTES {
        return Err(format!(
            "file is {} bytes; native YAS limit is {}",
            content.len(),
            yas_wire::schema::fs::MAX_STAGED_BYTES
        ));
    }
    let path = wire_path(&path)?;
    let mode = parse_mode(mode.as_deref())?;
    let precondition = if create {
        Precondition::Absent
    } else if !force {
        if_hash
            .as_deref()
            .map(parse_hash)
            .transpose()?
            .map(Precondition::Hash)
            .unwrap_or(Precondition::Any)
    } else {
        Precondition::Any
    };
    let mut root = Root::open(on, hub, &root_path, true).await?;
    let result = if content.len() <= fs::MAX_INLINE_BYTES && !durable {
        apply_one(
            &mut root,
            ApplyItem::WriteInline {
                path,
                precondition,
                create_parents: parents,
                mode,
                content,
            },
            json,
        )
        .await
    } else {
        stage_write(
            &mut root,
            path,
            precondition,
            parents,
            durable,
            mode,
            content,
            json,
        )
        .await
    };
    if result.is_ok() {
        root.close().await?;
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn stage_write(
    root: &mut Root,
    path: Path,
    precondition: Precondition,
    parents: bool,
    durable: bool,
    mode: u32,
    content: Vec<u8>,
    json: bool,
) -> Result<i32, String> {
    let hash = *blake3::hash(&content).as_bytes();
    let prefix = root
        .client
        .request_result(
            family::FS,
            fs::request_kind::STAGE_WRITE,
            StageWrite {
                root_handle: root.handle,
                path,
                precondition,
                flags: if parents {
                    yas_wire::schema::fs::STAGE_CREATE_PARENTS as u16
                } else {
                    0
                },
                mode,
                byte_len: content.len() as u64,
                content_hash: hash,
                initial_receive_credit: content.len() as u64,
                extensions: Extensions::default(),
            }
            .encode()
            .map_err(wire_error)?,
            true,
        )
        .await?;
    if prefix.status == Status::Conflict {
        return report_conflict(&prefix, json);
    }
    require_ok(&prefix, "FS STAGE_WRITE")?;
    let staged = StageWriteResult::decode(&prefix.body).map_err(wire_error)?;
    root.client
        .send_byte_transfer(&staged.descriptor, &content)
        .await?;
    let commit = root
        .client
        .request_result(
            family::FS,
            fs::request_kind::COMMIT,
            Commit {
                staging_handle: staged.staging_handle,
                operation_id: nonzero_operation_id(),
                flags: if durable {
                    (yas_wire::schema::fs::COMMIT_SYNC_DATA
                        | yas_wire::schema::fs::COMMIT_SYNC_DIRECTORY) as u16
                } else {
                    0
                },
                extensions: Extensions::default(),
            }
            .encode()
            .map_err(wire_error)?,
            true,
        )
        .await?;
    if commit.status == Status::Conflict {
        return report_conflict(&commit, json);
    }
    require_ok(&commit, "FS COMMIT")?;
    let result = CommitResult::decode(&commit.body).map_err(wire_error)?;
    report_success(
        result.content_hash,
        result.modified_unix_ns,
        result.entry_revision,
        json,
    );
    Ok(0)
}

async fn cmd_mkdir(
    on: Option<&str>,
    hub: &str,
    path: String,
    root_path: String,
    parents: bool,
    mode: Option<String>,
    json: bool,
) -> Result<i32, String> {
    run_apply(
        on,
        hub,
        root_path,
        ApplyItem::Mkdir {
            path: wire_path(&path)?,
            precondition: Precondition::Absent,
            create_parents: parents,
            mode: parse_mode(mode.as_deref())?,
        },
        json,
    )
    .await
}

async fn cmd_rm(
    on: Option<&str>,
    hub: &str,
    path: String,
    root_path: String,
    if_hash: Option<String>,
    json: bool,
) -> Result<i32, String> {
    run_apply(
        on,
        hub,
        root_path,
        ApplyItem::Remove {
            path: wire_path(&path)?,
            precondition: if_hash
                .as_deref()
                .map(parse_hash)
                .transpose()?
                .map(Precondition::Hash)
                .unwrap_or(Precondition::Any),
            flags: yas_wire::schema::fs::REMOVE_RECURSIVE as u16,
        },
        json,
    )
    .await
}

async fn cmd_mv(
    on: Option<&str>,
    hub: &str,
    from: String,
    to: String,
    root_path: String,
    parents: bool,
    json: bool,
) -> Result<i32, String> {
    run_apply(
        on,
        hub,
        root_path,
        ApplyItem::Rename {
            from: wire_path(&from)?,
            to: wire_path(&to)?,
            precondition: Precondition::Any,
            create_parents: parents,
        },
        json,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn cmd_ln(
    on: Option<&str>,
    hub: &str,
    target: String,
    link: String,
    symlink: bool,
    root_path: String,
    if_hash: Option<String>,
    force: bool,
    parents: bool,
    json: bool,
) -> Result<i32, String> {
    let precondition = if force {
        Precondition::Any
    } else {
        if_hash
            .as_deref()
            .map(parse_hash)
            .transpose()?
            .map(Precondition::Hash)
            .unwrap_or(Precondition::Absent)
    };
    let link = wire_path(&link)?;
    let item = if symlink {
        ApplyItem::Symlink {
            path: link,
            target: target.into_bytes(),
            precondition,
            create_parents: parents,
        }
    } else {
        ApplyItem::Hardlink {
            source: wire_path(&target)?,
            target: link,
            precondition,
            create_parents: parents,
        }
    };
    run_apply(on, hub, root_path, item, json).await
}

async fn run_apply(
    on: Option<&str>,
    hub: &str,
    root_path: String,
    item: ApplyItem,
    json: bool,
) -> Result<i32, String> {
    let mut root = Root::open(on, hub, &root_path, true).await?;
    let result = apply_one(&mut root, item, json).await;
    if result.is_ok() {
        root.close().await?;
    }
    result
}

async fn apply_one(root: &mut Root, item: ApplyItem, json: bool) -> Result<i32, String> {
    let result: ApplyResult = root
        .client
        .request_typed(
            family::FS,
            fs::request_kind::APPLY,
            &Apply {
                root_handle: root.handle,
                operation_id: nonzero_operation_id(),
                flags: yas_wire::schema::fs::APPLY_ALL_OR_NONE as u16,
                items: vec![item],
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    let item = result
        .items
        .into_iter()
        .find(|item| item.index == 0)
        .ok_or_else(|| "FS APPLY omitted its only item Result".to_string())?;
    if item.status == Status::Ok.code() {
        report_success(
            item.content_hash.unwrap_or([0; 32]),
            item.modified_unix_ns,
            item.entry_revision,
            json,
        );
        return Ok(0);
    }
    if item.status == Status::Conflict.code() {
        report_conflict_values(item.content_hash, item.modified_unix_ns, json);
        return Ok(1);
    }
    Err(if item.detail.is_empty() {
        format!("FS mutation failed with status {}", item.status)
    } else {
        item.detail
    })
}

async fn cmd_cat(
    on: Option<&str>,
    hub: &str,
    path: String,
    root_path: String,
) -> Result<i32, String> {
    let mut root = Root::open(on, hub, &root_path, false).await?;
    let prefix = root
        .client
        .request_result(
            family::FS,
            fs::request_kind::FETCH,
            Fetch {
                root_handle: root.handle,
                path: wire_path(&path)?,
                expected_hash: None,
                initial_receive_credit: yas_wire::schema::fs::MAX_STAGED_BYTES,
                extensions: Extensions::default(),
            }
            .encode()
            .map_err(wire_error)?,
            true,
        )
        .await?;
    if prefix.status == Status::NotFound {
        return Err(format!("cannot read {path}: no such file"));
    }
    require_ok(&prefix, "FS FETCH")?;
    let result = ContentResult::decode(&prefix.body).map_err(wire_error)?;
    let bytes = root
        .client
        .receive_inline_or_transfer(result.content, yas_wire::schema::fs::MAX_STAGED_BYTES)
        .await?;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|error| format!("writing stdout: {error}"))?;
    root.close().await?;
    Ok(0)
}

async fn cmd_find(
    on: Option<&str>,
    hub: &str,
    query: String,
    root_path: String,
    limit: u16,
    json: bool,
) -> Result<i32, String> {
    let mut root = Root::open(on, hub, &root_path, false).await?;
    let mut cursor = Vec::new();
    let mut paths = Vec::new();
    loop {
        let page: QueryPage = root
            .client
            .request_typed(
                family::FS,
                fs::request_kind::SEARCH,
                &Search {
                    root_handle: root.handle,
                    flags: 0,
                    max_results: limit.saturating_sub(paths.len().min(u16::MAX as usize) as u16),
                    query: query.as_bytes().to_vec(),
                    cursor: cursor.clone(),
                    initial_receive_credit: QUERY_CREDIT,
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        let next = page.next_cursor.clone();
        for record in page_records(&mut root.client, page).await? {
            if let QueryRecord::Path(record) = record {
                paths.push(record.path);
                if limit != 0 && paths.len() >= usize::from(limit) {
                    break;
                }
            }
        }
        if next.is_empty() || next == cursor || (limit != 0 && paths.len() >= usize::from(limit)) {
            break;
        }
        cursor = next;
    }
    for path in &paths {
        let path = display_path(path);
        if json {
            println!("{}", serde_json::json!({"path": path}));
        } else {
            println!("{path}");
        }
    }
    root.close().await?;
    Ok(if paths.is_empty() { 1 } else { 0 })
}

#[allow(clippy::too_many_arguments)]
async fn cmd_grep(
    on: Option<&str>,
    hub: &str,
    pattern: String,
    root_path: String,
    regex: bool,
    case_sensitive: bool,
    word: bool,
    no_ignore: bool,
    max_matches: u16,
    files_with_matches: bool,
    json: bool,
) -> Result<i32, String> {
    let mut root = Root::open(on, hub, &root_path, false).await?;
    let mut flags = 0u16;
    if regex {
        flags |= yas_wire::schema::fs::GREP_REGEX as u16;
    }
    if case_sensitive {
        flags |= yas_wire::schema::fs::GREP_CASE_SENSITIVE as u16;
    }
    if word {
        flags |= yas_wire::schema::fs::GREP_WORD as u16;
    }
    if no_ignore {
        flags |= yas_wire::schema::fs::GREP_INCLUDE_IGNORED as u16;
    }

    let mut cursor = Vec::new();
    let mut matched_files = 0usize;
    let mut hits = 0usize;
    let mut truncated = false;
    loop {
        let page: QueryPage = root
            .client
            .request_typed(
                family::FS,
                fs::request_kind::GREP,
                &Grep {
                    root_handle: root.handle,
                    flags,
                    max_results: max_matches,
                    max_per_file: 0,
                    query: pattern.as_bytes().to_vec(),
                    cursor: cursor.clone(),
                    initial_receive_credit: QUERY_CREDIT,
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        truncated |= page.flags & yas_wire::schema::fs::PAGE_TRUNCATED as u16 != 0;
        let next = page.next_cursor.clone();
        let records = page_records(&mut root.client, page).await?;
        let files: BTreeMap<u32, QueryGrepFileRecord> = records
            .iter()
            .filter_map(|record| match record {
                QueryRecord::GrepFile(file) => Some((file.file_index, file.clone())),
                _ => None,
            })
            .collect();
        if files_with_matches {
            for file in files.values().filter(|file| file.match_count != 0) {
                matched_files += 1;
                print_grep_file(file, json);
            }
        } else {
            for record in records {
                let QueryRecord::GrepMatch(found) = record else {
                    continue;
                };
                let file = files
                    .get(&found.file_index)
                    .ok_or_else(|| "FS GREP match referenced a missing file".to_string())?;
                hits += 1;
                let path = display_path(&file.path);
                let ignored =
                    file.flags & yas_wire::schema::fs::QUERY_GREP_FILE_IGNORED as u16 != 0;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({
                            "path": path,
                            "ignored": ignored,
                            "line": found.line + 1,
                            "col": found.column,
                            "endLine": found.end_line + 1,
                            "endCol": found.end_column,
                            "text": found.text,
                        })
                    );
                } else {
                    println!("{path}:{}:{}", found.line + 1, found.text);
                }
            }
        }
        if next.is_empty() || next == cursor {
            break;
        }
        cursor = next;
    }
    if truncated {
        eprintln!("yas: results truncated — a budget was reached");
    }
    root.close().await?;
    let count = if files_with_matches {
        matched_files
    } else {
        hits
    };
    Ok(if count == 0 { 1 } else { 0 })
}

async fn page_records(
    client: &mut NativeClient,
    page: QueryPage,
) -> Result<Vec<QueryRecord>, String> {
    let typed = match page.delivery {
        PageDelivery::Inline(records) => records,
        PageDelivery::Transfer(descriptor) => {
            let messages = client
                .receive_message_transfer(
                    &descriptor,
                    fs::MAX_QUERY_BYTES as u64,
                    fs::MAX_QUERY_RECORDS,
                )
                .await?;
            let mut expected = 0u32;
            let mut records = Vec::new();
            for message in messages {
                let batch = QueryRecordBatch::decode(&message).map_err(wire_error)?;
                if batch.first_record_index != expected {
                    return Err("FS query Transfer batches were not contiguous".into());
                }
                expected = expected
                    .checked_add(batch.records.len() as u32)
                    .ok_or_else(|| "FS query record index overflow".to_string())?;
                records.extend(batch.records);
            }
            records
        }
    };
    typed
        .iter()
        .map(QueryRecord::from_typed_record)
        .map(|record| record.map_err(wire_error))
        .collect()
}

fn print_grep_file(file: &QueryGrepFileRecord, json: bool) {
    let path = display_path(&file.path);
    let ignored = file.flags & yas_wire::schema::fs::QUERY_GREP_FILE_IGNORED as u16 != 0;
    if json {
        println!("{}", serde_json::json!({"path": path, "ignored": ignored}));
    } else {
        println!("{path}");
    }
}

fn report_success(hash: [u8; 32], modified_unix_ns: i64, revision: u64, json: bool) {
    let hash = encode_hash(&hash);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "ok",
                "hash": hash,
                "mtimeNs": modified_unix_ns,
                "revision": revision,
            })
        );
    } else if hash.bytes().any(|byte| byte != b'0') {
        eprintln!("ok {hash}");
    } else {
        eprintln!("ok");
    }
}

fn report_conflict(prefix: &ResultPrefix, json: bool) -> Result<i32, String> {
    let detail = ConflictDetail::from_result_detail(&prefix.detail)
        .map_err(wire_error)?
        .ok_or_else(|| "FS conflict Result omitted conflict detail".to_string())?;
    report_conflict_values(detail.current_hash, detail.modified_unix_ns, json);
    Ok(1)
}

fn report_conflict_values(hash: Option<[u8; 32]>, modified_unix_ns: i64, json: bool) {
    let hash = hash.map(|hash| encode_hash(&hash));
    if json {
        println!(
            "{}",
            serde_json::json!({
                "status": "conflict",
                "hash": hash,
                "mtimeNs": modified_unix_ns,
            })
        );
    } else if let Some(hash) = hash {
        eprintln!("conflict: on-disk hash is {hash} (rebase and retry, or --force)");
    } else {
        eprintln!("conflict: the path no longer exists");
    }
}

fn require_ok(prefix: &ResultPrefix, operation: &str) -> Result<(), String> {
    if prefix.status == Status::Ok {
        Ok(())
    } else {
        let detail = prefix
            .detail
            .0
            .iter()
            .find_map(|extension| String::from_utf8(extension.value.clone()).ok())
            .unwrap_or_default();
        Err(if detail.is_empty() {
            format!("{operation} failed with {:?}", prefix.status)
        } else {
            format!("{operation} failed with {:?}: {detail}", prefix.status)
        })
    }
}

fn nonzero_operation_id() -> [u8; 16] {
    loop {
        let value: [u8; 16] = rand::random();
        if value.iter().any(|byte| *byte != 0) {
            return value;
        }
    }
}

fn client_abs(path: &str) -> String {
    std::path::absolute(path)
        .unwrap_or_else(|_| std::path::PathBuf::from(path))
        .to_string_lossy()
        .into_owned()
}

fn wire_path(text: &str) -> Result<Path, String> {
    if text.as_bytes().contains(&0) {
        return Err("filesystem path contains NUL".into());
    }
    if text.starts_with('/') {
        return Err(format!("path must be relative to --root: {text}"));
    }
    let mut components = Vec::new();
    for component in text.split('/') {
        match component {
            "" | "." => {}
            ".." => return Err(format!("path must stay below --root: {text}")),
            value if value.contains('\\') => {
                return Err(format!("path component contains a separator: {value}"));
            }
            value => components.push(value.as_bytes().to_vec()),
        }
    }
    Ok(Path { components })
}

fn display_path(path: &Path) -> String {
    if path.components.is_empty() {
        return ".".into();
    }
    path.components
        .iter()
        .map(|component| String::from_utf8_lossy(component))
        .collect::<Vec<_>>()
        .join("/")
}

fn parse_mode(mode: Option<&str>) -> Result<u32, String> {
    match mode {
        None => Ok(0),
        Some(mode) => u32::from_str_radix(mode.trim_start_matches("0o"), 8)
            .map_err(|_| format!("invalid mode: {mode}")),
    }
}

fn parse_hash(text: &str) -> Result<[u8; 32], String> {
    let text = text.strip_prefix("0x").unwrap_or(text);
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("not a 32-byte BLAKE3 hex hash: {text}"));
    }
    let mut hash = [0; 32];
    for (index, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        hash[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(hash)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("invalid hex digit".into()),
    }
}

fn encode_hash(hash: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in hash {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0xf) as usize] as char);
    }
    output
}

fn entry_kind(entry: &EntryRecord) -> (&'static str, char, u64, Option<[u8; 32]>) {
    match &entry.body {
        EntryBody::File {
            byte_len,
            content_hash,
            ..
        } => ("file", 'f', *byte_len, Some(*content_hash)),
        EntryBody::Directory => ("dir", 'd', 0, None),
        EntryBody::Symlink {
            content_hash,
            target,
        } => ("symlink", 'l', target.len() as u64, Some(*content_hash)),
    }
}

fn print_entry_change(entry: &EntryRecord, existed: bool, json: bool) {
    let path = display_path(&entry.path);
    let (kind, kind_char, size, hash) = entry_kind(entry);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "type": "upsert",
                "path": path,
                "kind": kind,
                "size": size,
                "mtime_ns": entry.modified_unix_ns,
                "mode": entry.mode,
                "hash": hash.map(|hash| encode_hash(&hash)),
                "filtered": entry.flags & yas_wire::schema::fs::ENTRY_DIRECTORY_FILTERED as u8 != 0,
            })
        );
    } else {
        println!("{} {kind_char} {path}", if existed { '~' } else { '+' });
    }
}

fn print_remove(path: &Path, json: bool) {
    let path = display_path(path);
    if json {
        println!("{}", serde_json::json!({"type": "delete", "path": path}));
    } else {
        println!("- {path}");
    }
}

fn print_move(from: &Path, to: &Path, json: bool) {
    let from = display_path(from);
    let to = display_path(to);
    if json {
        println!(
            "{}",
            serde_json::json!({"type": "move", "from": from, "to": to})
        );
    } else {
        println!("> {from} -> {to}");
    }
}

fn print_snapshot(entries: &BTreeMap<Path, EntryRecord>) {
    for entry in entries.values() {
        let (_, kind, size, _) = entry_kind(entry);
        println!("{kind} {size:>12} {}", display_path(&entry.path));
    }
}

fn wire_error(error: impl std::fmt::Display) -> String {
    format!("invalid YAS FS payload: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_paths_are_relative_components() {
        assert_eq!(
            wire_path("./src/main.rs").unwrap().components,
            vec![b"src".to_vec(), b"main.rs".to_vec()]
        );
        assert!(wire_path("../secret").is_err());
        assert!(wire_path("/etc/passwd").is_err());
        assert_eq!(display_path(&wire_path(".").unwrap()), ".");
    }

    #[test]
    fn full_width_hash_and_octal_mode() {
        let text = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(encode_hash(&parse_hash(text).unwrap()), text);
        assert!(parse_hash("0123456789abcdef0123456789abcdef").is_err());
        assert_eq!(parse_mode(Some("0o755")).unwrap(), 0o755);
        assert!(parse_mode(Some("89")).is_err());
    }
}
