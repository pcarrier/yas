//! Native YAS Git commands.
//!
//! The command dispatcher uses the typed YAS Git family while the server
//! delegates repository work to its protocol-neutral Git engine.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::time::Duration;

use yas_wire::{
    Decode, Encode, Extensions,
    core::Status,
    family,
    fs::Path as FsPath,
    git::{
        self, Close, ContentDelivery, ContentRecord, EntityBody, EntityPatch, EntityRecord, Fetch,
        FetchResult, ObjectId, Open, OpenResult, PageDelivery, Query, QueryBody, QueryCursor,
        QueryEndpoint, QueryPage, QueryRecord, QueryState, RemovedEntity, RepositorySource, Watch,
        WatchQuery,
    },
    state::{Phase, RecordKind, StateAck, StateEvent, Unwatch, WatchResult},
};

use crate::{cli::GitCommand, yas_native::NativeClient};

const STATE_CREDIT: u64 = yas_wire::schema::transport::RECOMMENDED_BUFFERED;
const QUERY_CREDIT: u64 = git::MAX_QUERY_BYTES as u64;
const MAX_COLLECTED_RECORDS: usize = 65_536;

struct Repository {
    client: NativeClient,
    handle: u64,
}

impl Repository {
    async fn open(on: Option<&str>, hub: &str, path: &str) -> Result<Self, String> {
        let mut client = NativeClient::connect(on, hub).await?;
        let opened: OpenResult = client
            .request_typed(
                family::GIT,
                git::request_kind::OPEN,
                &Open {
                    source: RepositorySource::PlatformPath(path.as_bytes().to_vec()),
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        Ok(Self {
            client,
            handle: opened.repository_handle,
        })
    }

    async fn close(&mut self) -> Result<(), String> {
        self.client
            .request(
                family::GIT,
                git::request_kind::CLOSE,
                Close {
                    repository_handle: self.handle,
                    extensions: Extensions::default(),
                }
                .encode()
                .map_err(wire_error)?,
                true,
            )
            .await
            .map(|_| ())
    }

    async fn query(
        &mut self,
        body: QueryBody,
        page_records: u16,
        maximum_records: usize,
    ) -> Result<Vec<QueryRecord>, String> {
        query_all(
            &mut self.client,
            self.handle,
            body,
            page_records,
            maximum_records,
        )
        .await
    }

    async fn resolve_one(&mut self, spec: &str) -> Result<ObjectId, String> {
        let records = self
            .query(
                QueryBody::Resolve {
                    spec: spec.as_bytes().to_vec(),
                },
                16,
                16,
            )
            .await?;
        let mut tips = records.into_iter().filter_map(|record| match record {
            QueryRecord::Object(value)
                if value.role == yas_wire::schema::git::OBJECT_ROLE_TIP as u8 =>
            {
                Some(value.object)
            }
            _ => None,
        });
        let object = tips
            .next()
            .ok_or_else(|| format!("'{spec}' did not resolve to an object"))?;
        if tips.next().is_some() {
            return Err(format!("'{spec}' does not name one object"));
        }
        Ok(object)
    }
}

pub(crate) async fn dispatch(
    on: Option<&str>,
    hub: &str,
    command: GitCommand,
) -> Result<i32, String> {
    match command {
        GitCommand::Status { repo, watch, json } => cmd_status(on, hub, repo, watch, json).await,
        GitCommand::Log {
            rev,
            pathspec,
            repo,
            limit,
            watch,
            follow,
            first_parent,
            full_message,
            topo,
            json,
        } => {
            if pathspec.len() > 1 {
                return Err("only one path filter is supported".into());
            }
            cmd_log(
                on,
                hub,
                repo,
                rev,
                pathspec.into_iter().next(),
                limit,
                watch,
                follow,
                first_parent,
                full_message,
                topo,
                json,
            )
            .await
        }
        GitCommand::Diff {
            revs,
            pathspec,
            repo,
            staged,
            merge_base,
            patch,
            binary,
            json,
        } => {
            if pathspec.len() > 1 {
                return Err("only one path filter is supported".into());
            }
            cmd_diff(
                on,
                hub,
                repo,
                revs,
                pathspec.into_iter().next(),
                staged,
                merge_base,
                patch,
                binary,
                json,
            )
            .await
        }
        GitCommand::Show {
            spec,
            repo,
            max_len,
        } => cmd_show(on, hub, repo, spec, max_len).await,
        GitCommand::LsTree { spec, repo, json } => cmd_ls_tree(on, hub, repo, spec, json).await,
        GitCommand::LsFiles { path, repo, json } => cmd_ls_files(on, hub, repo, path, json).await,
        GitCommand::MergeBase { revs, repo, json } => {
            cmd_merge_base(on, hub, repo, revs, json).await
        }
        GitCommand::Blame {
            path,
            repo,
            rev,
            start,
            lines,
            follow,
            json,
        } => cmd_blame(on, hub, repo, path, rev, start, lines, follow, json).await,
        GitCommand::Reflog {
            ref_name,
            repo,
            limit,
            reverse,
            json,
        } => cmd_reflog(on, hub, repo, ref_name, limit, reverse, json).await,
        GitCommand::Discover {
            path,
            depth,
            nested,
            bare,
            json,
        } => cmd_discover(on, hub, path, depth, nested, bare, json).await,
        GitCommand::Fetch {
            remote,
            refspecs,
            repo,
            prune,
            anchor,
            timeout,
            json,
        } => {
            cmd_fetch(
                on, hub, repo, remote, refspecs, prune, anchor, timeout, json,
            )
            .await
        }
    }
}

async fn query_all(
    client: &mut NativeClient,
    repository_handle: u64,
    body: QueryBody,
    page_records: u16,
    maximum_records: usize,
) -> Result<Vec<QueryRecord>, String> {
    let mut cursor = QueryCursor::Start;
    let mut records = Vec::new();
    loop {
        let page: QueryPage = client
            .request_typed(
                family::GIT,
                git::request_kind::QUERY,
                &Query {
                    repository_handle,
                    max_records: page_records,
                    cursor: cursor.clone(),
                    initial_receive_credit: QUERY_CREDIT,
                    body: body.clone(),
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        let typed = match page.delivery {
            PageDelivery::Inline(records) => records,
            PageDelivery::Transfer(descriptor) => client
                .receive_message_transfer(
                    &descriptor,
                    git::MAX_QUERY_BYTES as u64,
                    git::MAX_QUERY_RECORDS,
                )
                .await?
                .into_iter()
                .map(|message| git::TypedRecord::decode_message(&message).map_err(wire_error))
                .collect::<Result<Vec<_>, _>>()?,
        };
        for record in typed {
            if let Some(record) = QueryRecord::decode_typed(&record).map_err(wire_error)? {
                records.push(record);
                if records.len() > maximum_records.min(MAX_COLLECTED_RECORDS) {
                    return Err("Git query exceeded the CLI record limit; narrow it".into());
                }
            }
        }
        cursor = page.next_cursor;
        if matches!(cursor, QueryCursor::Start) || records.len() >= maximum_records {
            records.truncate(maximum_records);
            return Ok(records);
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn cmd_log(
    on: Option<&str>,
    hub: &str,
    repo: String,
    rev: Option<String>,
    path: Option<String>,
    limit: u16,
    watch: bool,
    follow: bool,
    first_parent: bool,
    full_message: bool,
    topo: bool,
    json: bool,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let mut flags = 0u16;
    if follow {
        flags |= yas_wire::schema::git::LOG_FOLLOW as u16;
    }
    if first_parent {
        flags |= yas_wire::schema::git::LOG_FIRST_PARENT as u16;
    }
    if full_message {
        flags |= yas_wire::schema::git::LOG_FULL_MESSAGE as u16;
    }
    if topo {
        flags |= yas_wire::schema::git::LOG_TOPO as u16;
    }
    let body = QueryBody::Log {
        spec: rev.unwrap_or_else(|| "HEAD".into()).into_bytes(),
        tips: Vec::new(),
        hides: Vec::new(),
        path: path.as_deref().map(fs_path).transpose()?,
        flags,
    };
    if watch {
        return watch_log(&mut repository, body, limit, full_message, json).await;
    }
    let records = repository
        .query(body, limit.max(1), usize::from(limit))
        .await?;
    print_commits(records, full_message, json);
    repository.close().await?;
    Ok(0)
}

async fn watch_log(
    repository: &mut Repository,
    body: QueryBody,
    limit: u16,
    full_message: bool,
    json: bool,
) -> Result<i32, String> {
    let watched: WatchResult = repository
        .client
        .request_typed(
            family::GIT,
            git::request_kind::WATCH_QUERY,
            &WatchQuery {
                repository_handle: repository.handle,
                max_records: limit.max(1),
                body,
                state: yas_wire::state::Watch {
                    initial_credit: STATE_CREDIT,
                    resume: None,
                    extensions: Extensions::default(),
                },
            },
            true,
        )
        .await?;
    let mut cumulative_credit = STATE_CREDIT;
    loop {
        let frame = repository
            .client
            .next_matching_event(family::GIT, git::event_kind::QUERY_STATE)
            .await?;
        if !frame.header.sensitive {
            return Err("Git QUERY_STATE event was not marked sensitive".into());
        }
        let state = QueryState::decode(&frame.payload).map_err(wire_error)?;
        if state.query_subscription_id != watched.subscription_id {
            continue;
        }
        if let Some(value) = state.value().map_err(wire_error)? {
            if value.status != Status::Ok {
                if !json {
                    eprintln!("(log unavailable: {:?}: {})", value.status, value.detail);
                }
            } else if let Some(page) = value.page {
                let PageDelivery::Inline(typed) = page.delivery else {
                    return Err("Git watched log unexpectedly used Transfer delivery".into());
                };
                let records = typed
                    .into_iter()
                    .map(|record| QueryRecord::decode_typed(&record).map_err(wire_error))
                    .collect::<Result<Vec<_>, _>>()?
                    .into_iter()
                    .flatten()
                    .collect();
                if json {
                    println!("{}", serde_json::json!({"type": "page"}));
                } else {
                    print!("\x1b[2J\x1b[H");
                }
                print_commits(records, full_message, json);
                if page.flags & yas_wire::schema::git::QUERY_PAGE_MORE as u16 != 0 && !json {
                    eprintln!("… (more; raise -n)");
                }
            }
        }
        cumulative_credit = cumulative_credit.saturating_add(frame.payload.len() as u64);
        repository
            .client
            .send_typed_event(
                family::GIT,
                git::event_kind::QUERY_STATE_ACK,
                &StateAck {
                    subscription_id: watched.subscription_id,
                    applied_revision: state.event.to_revision,
                    cumulative_byte_limit: cumulative_credit,
                },
                false,
            )
            .await?;
        if state.event.phase == Phase::Reset {
            continue;
        }
    }
}

fn print_commits(records: Vec<QueryRecord>, full_message: bool, json: bool) {
    for record in records {
        let QueryRecord::Commit(commit) = record else {
            continue;
        };
        let oid = object_hex(&commit.object);
        let author = String::from_utf8_lossy(&commit.author_name);
        let email = String::from_utf8_lossy(&commit.author_email);
        let message = String::from_utf8_lossy(&commit.message);
        if json {
            println!(
                "{}",
                serde_json::json!({
                    "type": "commit", "oid": oid,
                    "tree": object_hex(&commit.tree),
                    "parents": commit.parents.iter().map(object_hex).collect::<Vec<_>>(),
                    "author": {"name": author, "email": email, "time": commit.authored_unix_seconds, "tz": commit.author_timezone_minutes},
                    "committer": {"name": String::from_utf8_lossy(&commit.committer_name), "email": String::from_utf8_lossy(&commit.committer_email), "time": commit.committed_unix_seconds, "tz": commit.committer_timezone_minutes},
                    "message": message,
                })
            );
        } else {
            println!(
                "{} {author} <{email}> {}",
                &oid[..oid.len().min(8)],
                message.lines().next().unwrap_or("")
            );
            if full_message {
                for line in message.lines().skip(1) {
                    println!("    {line}");
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn cmd_diff(
    on: Option<&str>,
    hub: &str,
    repo: String,
    revs: Vec<String>,
    path: Option<String>,
    staged: bool,
    merge_base: bool,
    patch: bool,
    binary: bool,
    json: bool,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let (mut left, right) = diff_endpoints(&mut repository, &revs, staged).await?;
    if merge_base {
        left = match left {
            QueryEndpoint::Commit(object) => QueryEndpoint::MergeBase(object),
            _ => return Err("--merge-base needs a commit revision".into()),
        };
    }
    let path = path.as_deref().map(fs_path).transpose()?;
    let records = if patch {
        let mut flags =
            yas_wire::schema::git::PATCH_RENAMES as u16 | yas_wire::schema::git::PATCH_TEXT as u16;
        if matches!(right, QueryEndpoint::Worktree) {
            flags |= yas_wire::schema::git::PATCH_UNTRACKED as u16;
        }
        if binary {
            flags |= yas_wire::schema::git::PATCH_BINARY as u16;
        }
        repository
            .query(
                QueryBody::Patch {
                    left,
                    right,
                    path,
                    context_lines: 3,
                    rename_threshold: 0,
                    max_bytes: git::MAX_QUERY_BYTES as u32,
                    flags,
                },
                git::MAX_QUERY_RECORDS as u16,
                MAX_COLLECTED_RECORDS,
            )
            .await?
    } else {
        let mut flags = yas_wire::schema::git::DIFF_RENAMES as u16;
        if matches!(right, QueryEndpoint::Worktree) {
            flags |= yas_wire::schema::git::DIFF_UNTRACKED as u16;
        }
        repository
            .query(
                QueryBody::Diff {
                    left,
                    right,
                    path,
                    rename_threshold: 0,
                    flags,
                },
                git::MAX_QUERY_RECORDS as u16,
                MAX_COLLECTED_RECORDS,
            )
            .await?
    };
    for record in records {
        match record {
            QueryRecord::Diff(diff) => print_diff(&diff, json),
            QueryRecord::PatchContent(content) => {
                let bytes = content_bytes(&mut repository.client, content).await?;
                if json {
                    println!(
                        "{}",
                        serde_json::json!({"type": "patch", "text": String::from_utf8_lossy(&bytes)})
                    );
                } else {
                    std::io::stdout()
                        .write_all(&bytes)
                        .map_err(|error| format!("writing stdout: {error}"))?;
                }
            }
            QueryRecord::PatchFile(file) => {
                let old = file.old_path.as_ref().map(display_path).unwrap_or_default();
                let new = file.new_path.as_ref().map(display_path).unwrap_or_default();
                println!("diff --git a/{old} b/{new}");
            }
            QueryRecord::PatchRow(row) => {
                if row.old_line != 0 {
                    println!("-{}", String::from_utf8_lossy(&row.old_text));
                }
                if row.new_line != 0 {
                    println!("+{}", String::from_utf8_lossy(&row.new_text));
                }
            }
            _ => {}
        }
    }
    repository.close().await?;
    Ok(0)
}

async fn diff_endpoints(
    repository: &mut Repository,
    revs: &[String],
    staged: bool,
) -> Result<(QueryEndpoint, QueryEndpoint), String> {
    if staged && revs.len() > 1 {
        return Err("--staged cannot be combined with two revisions".into());
    }
    match revs {
        [] if staged => Ok((
            QueryEndpoint::Commit(repository.resolve_one("HEAD").await?),
            QueryEndpoint::Index,
        )),
        [] => Ok((QueryEndpoint::Index, QueryEndpoint::Worktree)),
        [range] if range.contains("...") => {
            if staged {
                return Err("--staged cannot be combined with a range".into());
            }
            let (left, right) = split_range(range, "...");
            Ok((
                QueryEndpoint::MergeBase(repository.resolve_one(left).await?),
                QueryEndpoint::Commit(repository.resolve_one(right).await?),
            ))
        }
        [range] if range.contains("..") => {
            if staged {
                return Err("--staged cannot be combined with a range".into());
            }
            let (left, right) = split_range(range, "..");
            Ok((
                QueryEndpoint::Commit(repository.resolve_one(left).await?),
                QueryEndpoint::Commit(repository.resolve_one(right).await?),
            ))
        }
        [one] => Ok((
            QueryEndpoint::Commit(repository.resolve_one(one).await?),
            if staged {
                QueryEndpoint::Index
            } else {
                QueryEndpoint::Worktree
            },
        )),
        [left, right] => Ok((
            QueryEndpoint::Commit(repository.resolve_one(left).await?),
            QueryEndpoint::Commit(repository.resolve_one(right).await?),
        )),
        _ => Err("git diff takes at most two revisions".into()),
    }
}

async fn cmd_show(
    on: Option<&str>,
    hub: &str,
    repo: String,
    spec: String,
    max_len: u32,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let (revision, path) = split_revision_path(&spec);
    let object = repository.resolve_one(revision).await?;
    let records = repository
        .query(
            QueryBody::Blob {
                object,
                path: path.map(fs_path).transpose()?,
                offset: 0,
                max_bytes: max_len,
                flags: yas_wire::schema::git::BLOB_WHOLE as u16,
            },
            1,
            1,
        )
        .await?;
    let content = records.into_iter().find_map(|record| match record {
        QueryRecord::Blob(content) => Some(content),
        _ => None,
    });
    let Some(content) = content else {
        repository.close().await?;
        return Ok(1);
    };
    let bytes = content_bytes(&mut repository.client, content).await?;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|error| format!("writing stdout: {error}"))?;
    repository.close().await?;
    Ok(0)
}

async fn cmd_ls_tree(
    on: Option<&str>,
    hub: &str,
    repo: String,
    spec: String,
    json: bool,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let (revision, path) = split_revision_path(&spec);
    let tree = repository.resolve_one(revision).await?;
    let records = repository
        .query(
            QueryBody::Tree {
                tree,
                path: path.map(fs_path).transpose()?.unwrap_or(FsPath {
                    components: Vec::new(),
                }),
            },
            git::MAX_QUERY_RECORDS as u16,
            MAX_COLLECTED_RECORDS,
        )
        .await?;
    for record in records {
        if let QueryRecord::TreeEntry(entry) = record {
            let kind = match entry.entry_kind {
                value if value == yas_wire::schema::git::TREE_TREE as u8 => "tree",
                value if value == yas_wire::schema::git::TREE_COMMIT as u8 => "commit",
                _ => "blob",
            };
            if json {
                println!(
                    "{}",
                    serde_json::json!({"mode": format!("{:06o}", entry.mode), "type": kind, "oid": object_hex(&entry.object), "name": String::from_utf8_lossy(&entry.name)})
                );
            } else {
                println!(
                    "{:06o} {kind} {}\t{}",
                    entry.mode,
                    object_hex(&entry.object),
                    String::from_utf8_lossy(&entry.name)
                );
            }
        }
    }
    repository.close().await?;
    Ok(0)
}

async fn cmd_ls_files(
    on: Option<&str>,
    hub: &str,
    repo: String,
    path: String,
    json: bool,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let records = repository
        .query(
            QueryBody::Index {
                path: (!path.is_empty()).then(|| fs_path(&path)).transpose()?,
                flags: yas_wire::schema::git::INDEX_STAGED as u16,
            },
            git::MAX_QUERY_RECORDS as u16,
            MAX_COLLECTED_RECORDS,
        )
        .await?;
    for record in records {
        if let QueryRecord::IndexEntry(entry) = record {
            let path = display_path(&entry.path);
            if json {
                println!(
                    "{}",
                    serde_json::json!({"mode": format!("{:06o}", entry.mode), "stage": entry.stage, "oid": object_hex(&entry.object), "path": path})
                );
            } else {
                println!(
                    "{:06o} {} {}\t{path}",
                    entry.mode,
                    entry.stage,
                    object_hex(&entry.object)
                );
            }
        }
    }
    repository.close().await?;
    Ok(0)
}

async fn cmd_merge_base(
    on: Option<&str>,
    hub: &str,
    repo: String,
    revs: Vec<String>,
    json: bool,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let mut objects = Vec::with_capacity(revs.len());
    for revision in revs {
        objects.push(repository.resolve_one(&revision).await?);
    }
    let records = repository
        .query(QueryBody::MergeBase { objects }, 16, 16)
        .await?;
    let bases = records
        .into_iter()
        .filter_map(|record| match record {
            QueryRecord::Object(value) => Some(value.object),
            _ => None,
        })
        .collect::<Vec<_>>();
    for base in &bases {
        if json {
            println!("{}", serde_json::json!({"oid": object_hex(base)}));
        } else {
            println!("{}", object_hex(base));
        }
    }
    repository.close().await?;
    Ok(if bases.is_empty() { 1 } else { 0 })
}

#[allow(clippy::too_many_arguments)]
async fn cmd_blame(
    on: Option<&str>,
    hub: &str,
    repo: String,
    path: String,
    rev: Option<String>,
    start: Option<u32>,
    lines: Option<u32>,
    follow: bool,
    json: bool,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let object = repository
        .resolve_one(rev.as_deref().unwrap_or("HEAD"))
        .await?;
    let records = repository
        .query(
            QueryBody::Blame {
                object,
                path: fs_path(&path)?,
                start_line: start.unwrap_or(1),
                line_count: lines.unwrap_or(0),
                flags: if follow {
                    yas_wire::schema::git::BLAME_FOLLOW_RENAMES as u16
                } else {
                    0
                },
            },
            git::MAX_QUERY_RECORDS as u16,
            MAX_COLLECTED_RECORDS,
        )
        .await?;
    for record in records {
        if let QueryRecord::Blame(blame) = record {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"oid": object_hex(&blame.commit), "start": blame.start_line, "end": blame.end_line, "originalStart": blame.original_start_line, "originalPath": blame.original_path.as_ref().map(display_path)})
                );
            } else {
                println!(
                    "{} {}-{}",
                    object_hex(&blame.commit),
                    blame.start_line,
                    blame.end_line.saturating_sub(1)
                );
            }
        }
    }
    repository.close().await?;
    Ok(0)
}

async fn cmd_reflog(
    on: Option<&str>,
    hub: &str,
    repo: String,
    ref_name: String,
    limit: u16,
    reverse: bool,
    json: bool,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let records = repository
        .query(
            QueryBody::Reflog {
                name: if ref_name.is_empty() {
                    b"HEAD".to_vec()
                } else {
                    ref_name.into_bytes()
                },
                flags: if reverse {
                    yas_wire::schema::git::REFLOG_OLDEST_FIRST as u16
                } else {
                    0
                },
            },
            limit.max(1),
            usize::from(limit),
        )
        .await?;
    for record in records {
        if let QueryRecord::Reflog(entry) = record {
            if json {
                println!(
                    "{}",
                    serde_json::json!({"index": entry.index, "old": object_hex(&entry.old_object), "new": object_hex(&entry.new_object), "time": entry.committed_unix_seconds, "message": String::from_utf8_lossy(&entry.message)})
                );
            } else {
                println!(
                    "{} HEAD@{{{}}}: {}",
                    &object_hex(&entry.new_object)[..8],
                    entry.index,
                    String::from_utf8_lossy(&entry.message)
                );
            }
        }
    }
    repository.close().await?;
    Ok(0)
}

async fn cmd_discover(
    on: Option<&str>,
    hub: &str,
    path: String,
    depth: u8,
    nested: bool,
    bare: bool,
    json: bool,
) -> Result<i32, String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let mut flags = 0u16;
    if nested {
        flags |= yas_wire::schema::git::DISCOVER_NESTED as u16;
    }
    if bare {
        flags |= yas_wire::schema::git::DISCOVER_BARE as u16;
    }
    let records = query_all(
        &mut client,
        0,
        QueryBody::Discover {
            source: RepositorySource::PlatformPath(path.into_bytes()),
            max_depth: u16::from(depth),
            flags,
        },
        git::MAX_QUERY_RECORDS as u16,
        MAX_COLLECTED_RECORDS,
    )
    .await?;
    for record in records {
        if let QueryRecord::Discovery(found) = record {
            let worktree = String::from_utf8_lossy(&found.worktree_path);
            let git_dir = String::from_utf8_lossy(&found.git_dir);
            if json {
                println!(
                    "{}",
                    serde_json::json!({"worktree": worktree, "gitDir": git_dir, "bare": found.flags & yas_wire::schema::git::DISCOVERY_BARE as u16 != 0})
                );
            } else {
                println!("{worktree}\t{git_dir}");
            }
        }
    }
    Ok(0)
}

#[allow(clippy::too_many_arguments)]
async fn cmd_fetch(
    on: Option<&str>,
    hub: &str,
    repo: String,
    remote: String,
    refspecs: Vec<String>,
    prune: bool,
    anchor: bool,
    timeout: u32,
    json: bool,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let mut flags = 0u16;
    if prune {
        flags |= yas_wire::schema::git::FETCH_PRUNE as u16;
    }
    if anchor {
        flags |= yas_wire::schema::git::FETCH_ANCHOR as u16;
    }
    let result: FetchResult = repository
        .client
        .request_typed_with_timeout(
            family::GIT,
            git::request_kind::FETCH,
            &Fetch {
                repository_handle: repository.handle,
                operation_id: rand::random(),
                flags,
                timeout_ms: timeout.saturating_mul(1_000),
                remote: remote.into_bytes(),
                refspecs: refspecs.into_iter().map(String::into_bytes).collect(),
                extensions: Extensions::default(),
            },
            true,
            Duration::from_secs(u64::from(timeout).saturating_add(10)),
        )
        .await?;
    let mut failed = false;
    for reference in result.refs {
        let status = Status::from_code(reference.status);
        failed |= status != Status::Ok;
        let name = String::from_utf8_lossy(&reference.name);
        if json {
            println!(
                "{}",
                serde_json::json!({"name": name, "status": format!("{status:?}").to_ascii_lowercase(), "old": reference.old.as_ref().map(object_hex), "new": reference.new.as_ref().map(object_hex), "detail": reference.detail})
            );
        } else {
            println!("{name}\t{status:?}\t{}", reference.detail);
        }
    }
    repository.close().await?;
    Ok(i32::from(failed))
}

async fn cmd_status(
    on: Option<&str>,
    hub: &str,
    repo: String,
    watch_forever: bool,
    json: bool,
) -> Result<i32, String> {
    let mut repository = Repository::open(on, hub, &repo).await?;
    let result: WatchResult = repository
        .client
        .request_typed(
            family::GIT,
            git::request_kind::WATCH,
            &Watch {
                repository_handle: repository.handle,
                datasets: (yas_wire::schema::git::WATCH_HEAD
                    | yas_wire::schema::git::WATCH_STATUS
                    | yas_wire::schema::git::WATCH_UPSTREAMS
                    | yas_wire::schema::git::WATCH_STASHES) as u16,
                state: yas_wire::state::Watch {
                    initial_credit: STATE_CREDIT,
                    resume: None,
                    extensions: Extensions::default(),
                },
            },
            true,
        )
        .await?;
    let mut entities = BTreeMap::<(u16, Vec<u8>), EntityRecord>::new();
    let mut snapshot_done = false;
    let mut cumulative_credit = STATE_CREDIT;
    loop {
        let frame = repository
            .client
            .next_matching_event(family::GIT, git::event_kind::STATE)
            .await?;
        let event = StateEvent::decode(&frame.payload).map_err(wire_error)?;
        if event.subscription_id != result.subscription_id {
            continue;
        }
        if matches!(event.phase, Phase::SnapshotBegin | Phase::Reset) {
            entities.clear();
            snapshot_done = false;
        }
        for record in &event.records {
            match record.kind {
                RecordKind::Add | RecordKind::Replace => {
                    let entity = EntityRecord::decode(&record.body).map_err(wire_error)?;
                    entities.insert((entity.entity_kind, entity.key.clone()), entity);
                }
                RecordKind::Patch => {
                    let patch = EntityPatch::decode(&record.body).map_err(wire_error)?;
                    entities.insert(
                        (patch.replacement.entity_kind, patch.replacement.key.clone()),
                        patch.replacement,
                    );
                }
                RecordKind::Remove => {
                    let removed = RemovedEntity::decode(&record.body).map_err(wire_error)?;
                    entities.remove(&(removed.entity_kind, removed.key));
                }
                RecordKind::Family(_) if record.required => {
                    return Err("Git sent an unsupported required State record".into());
                }
                _ => {}
            }
        }
        cumulative_credit = cumulative_credit.saturating_add(frame.payload.len() as u64);
        repository
            .client
            .send_typed_event(
                family::GIT,
                git::event_kind::STATE_ACK,
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
            print_status(&entities, json);
            if !watch_forever {
                repository
                    .client
                    .request(
                        family::GIT,
                        git::request_kind::UNWATCH,
                        Unwatch {
                            subscription_id: result.subscription_id,
                        }
                        .encode()
                        .map_err(wire_error)?,
                        true,
                    )
                    .await?;
                repository.close().await?;
                return Ok(0);
            }
        }
    }
}

fn print_status(entities: &BTreeMap<(u16, Vec<u8>), EntityRecord>, json: bool) {
    let head = entities.values().find_map(|entity| match &entity.body {
        EntityBody::Head(value) => Some(value),
        _ => None,
    });
    let upstream = entities.values().find_map(|entity| match &entity.body {
        EntityBody::Upstream(value) => Some(value),
        _ => None,
    });
    let stashes = entities
        .values()
        .filter(|entity| matches!(entity.body, EntityBody::Stash(_)))
        .count();
    let statuses = entities
        .values()
        .filter_map(|entity| match &entity.body {
            EntityBody::Status(value) => FsPath::decode(&entity.key)
                .ok()
                .map(|path| (display_path(&path), value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let branch = head
        .map(|head| String::from_utf8_lossy(&head.symbolic_target).into_owned())
        .unwrap_or_default();
    if json {
        println!(
            "{}",
            serde_json::json!({
                "type": "state", "head": branch,
                "oid": head.and_then(|head| head.object.as_ref()).map(object_hex),
                "ahead": upstream.filter(|value| value.flags & yas_wire::schema::git::UPSTREAM_COUNTS_VALID as u16 != 0).map(|value| value.ahead),
                "behind": upstream.filter(|value| value.flags & yas_wire::schema::git::UPSTREAM_COUNTS_VALID as u16 != 0).map(|value| value.behind),
                "stashes": stashes,
                "status": statuses.iter().map(|(path, value)| serde_json::json!({"staged": status_char(value.index_status).to_string(), "unstaged": status_char(value.worktree_status).to_string(), "path": path, "old_path": value.old_path.as_ref().map(display_path)})).collect::<Vec<_>>(),
            })
        );
        return;
    }
    if !branch.is_empty() {
        let short = branch.strip_prefix("refs/heads/").unwrap_or(&branch);
        print!("on {short}");
        if let Some(upstream) = upstream
            && upstream.flags & yas_wire::schema::git::UPSTREAM_COUNTS_VALID as u16 != 0
        {
            if upstream.ahead != 0 {
                print!(" ↑{}", upstream.ahead);
            }
            if upstream.behind != 0 {
                print!(" ↓{}", upstream.behind);
            }
        }
        if stashes != 0 {
            print!(" [{stashes} stashed]");
        }
        println!();
    }
    if statuses.is_empty() {
        println!("clean");
    } else {
        for (path, value) in statuses {
            println!(
                "{}{} {path}",
                status_char(value.index_status),
                status_char(value.worktree_status)
            );
        }
    }
}

async fn content_bytes(
    client: &mut NativeClient,
    content: ContentRecord,
) -> Result<Vec<u8>, String> {
    let expected = content.next_offset.saturating_sub(content.offset);
    match content.delivery {
        ContentDelivery::Inline(bytes) => Ok(bytes),
        ContentDelivery::Transfer(descriptor) => {
            client
                .receive_byte_transfer(&descriptor, Some(expected), git::MAX_QUERY_BYTES as u64)
                .await
        }
    }
}

fn print_diff(diff: &git::DiffRecord, json: bool) {
    let old = diff.old_path.as_ref().map(display_path);
    let new = diff.new_path.as_ref().map(display_path);
    let status = diff_status_name(diff.status);
    if json {
        println!(
            "{}",
            serde_json::json!({"type": "diff", "status": status, "oldPath": old, "newPath": new, "similarity": diff.similarity_percent, "oldMode": diff.old_mode, "newMode": diff.new_mode})
        );
    } else if old != new && old.is_some() && new.is_some() {
        println!(
            "{status}\t{} -> {}",
            old.unwrap_or_default(),
            new.unwrap_or_default()
        );
    } else {
        println!("{status}\t{}", new.or(old).unwrap_or_default());
    }
}

fn fs_path(path: &str) -> Result<FsPath, String> {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(format!("Git path must be repository-relative: {path}"));
    }
    let mut components = Vec::new();
    for component in normalized.split('/') {
        match component {
            "" | "." => {}
            ".." => return Err(format!("Git path escapes repository: {path}")),
            component if component.as_bytes().contains(&0) => {
                return Err("Git path contains NUL".into());
            }
            component => components.push(component.as_bytes().to_vec()),
        }
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

fn split_revision_path(spec: &str) -> (&str, Option<&str>) {
    match spec.split_once(':') {
        Some(("", path)) => ("HEAD", Some(path)),
        Some((revision, path)) => (revision, Some(path)),
        None => (spec, None),
    }
}

fn split_range<'a>(spec: &'a str, separator: &str) -> (&'a str, &'a str) {
    let (left, right) = spec.split_once(separator).unwrap_or((spec, ""));
    (
        if left.is_empty() { "HEAD" } else { left },
        if right.is_empty() { "HEAD" } else { right },
    )
}

fn object_hex(object: &ObjectId) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(object.bytes.len() * 2);
    for byte in &object.bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0xf) as usize] as char);
    }
    output
}

fn status_char(status: u8) -> char {
    match status {
        value if value == yas_wire::schema::git::WORKTREE_STATUS_ADDED as u8 => 'A',
        value if value == yas_wire::schema::git::WORKTREE_STATUS_MODIFIED as u8 => 'M',
        value if value == yas_wire::schema::git::WORKTREE_STATUS_DELETED as u8 => 'D',
        value if value == yas_wire::schema::git::WORKTREE_STATUS_RENAMED as u8 => 'R',
        value if value == yas_wire::schema::git::WORKTREE_STATUS_COPIED as u8 => 'C',
        value if value == yas_wire::schema::git::WORKTREE_STATUS_TYPE_CHANGED as u8 => 'T',
        value if value == yas_wire::schema::git::WORKTREE_STATUS_UNMERGED as u8 => 'U',
        value if value == yas_wire::schema::git::WORKTREE_STATUS_UNTRACKED as u8 => '?',
        value if value == yas_wire::schema::git::WORKTREE_STATUS_IGNORED as u8 => '!',
        _ => ' ',
    }
}

fn diff_status_name(status: u8) -> &'static str {
    match status {
        value if value == yas_wire::schema::git::DIFF_ADDED as u8 => "A",
        value if value == yas_wire::schema::git::DIFF_MODIFIED as u8 => "M",
        value if value == yas_wire::schema::git::DIFF_DELETED as u8 => "D",
        value if value == yas_wire::schema::git::DIFF_RENAMED as u8 => "R",
        value if value == yas_wire::schema::git::DIFF_COPIED as u8 => "C",
        _ => "?",
    }
}

fn wire_error(error: impl std::fmt::Display) -> String {
    format!("invalid YAS Git payload: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn revision_path_and_ranges_match_git_cli_spellings() {
        assert_eq!(
            split_revision_path("HEAD:src/lib.rs"),
            ("HEAD", Some("src/lib.rs"))
        );
        assert_eq!(
            split_revision_path(":README.md"),
            ("HEAD", Some("README.md"))
        );
        assert_eq!(split_range("..feature", ".."), ("HEAD", "feature"));
    }

    #[test]
    fn repository_paths_cannot_escape() {
        assert_eq!(
            display_path(&fs_path("src/main.rs").unwrap()),
            "src/main.rs"
        );
        assert!(fs_path("../outside").is_err());
        assert!(fs_path("/outside").is_err());
    }
}
