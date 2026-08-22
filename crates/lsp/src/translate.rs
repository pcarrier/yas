//! LSP JSON results → native semantic records.
//!
//! Normalization lives here: `Location` vs `LocationLink`, hierarchical
//! `DocumentSymbol[]` vs flat `SymbolInformation[]`, the three hover
//! content shapes, and `WorkspaceEdit`'s two encodings all become the
//! four record kinds. Positions are transcoded from the backend's
//! negotiated encoding to UTF-8 byte columns against the text the
//! backend actually holds.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::model::{
    LSP_ACTION_DISABLED, LSP_ACTION_PREFERRED, LSP_COMPLETION_DEPRECATED, LSP_COMPLETION_PRESELECT,
    LSP_COMPLETION_SNIPPET, LSP_HASH_NONE, LSP_MARKUP_MARKDOWN, LSP_MARKUP_PLAIN,
    LSP_SIGNATURE_ACTIVE, LSP_SIGNATURE_NO_PARAM, LSP_SYMBOL_DEPRECATED, LspHash,
};
use serde_json::Value;

use crate::native::QueryRecord;
use crate::text::{self, IndexedText, PositionEncoding};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectedRecord<'a> {
    Location {
        flags: u8,
        hash: LspHash,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
        path: &'a Path,
    },
    Markup {
        format: u8,
        text: &'a str,
    },
    Symbol {
        sym_kind: u8,
        flags: u8,
        depth: u8,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
        name: &'a str,
        path: Option<&'a Path>,
    },
    Edit {
        flags: u8,
        hash: LspHash,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
        new_text: &'a str,
        path: &'a Path,
    },
    Completion {
        item_kind: u8,
        flags: u8,
        line: u32,
        col: u32,
        end_line: u32,
        end_col: u32,
        label: &'a str,
        insert: &'a str,
        detail: &'a str,
    },
    Signature {
        flags: u8,
        active_param: u16,
        param_start: u16,
        param_end: u16,
        label: &'a str,
        doc: &'a str,
    },
    Action {
        flags: u8,
        edit_count: u16,
        title: &'a str,
        action_kind: &'a str,
        disabled_reason: &'a str,
    },
}

impl ProjectedRecord<'_> {
    fn owned(&self) -> QueryRecord {
        match self {
            Self::Location {
                flags,
                hash,
                line,
                col,
                end_line,
                end_col,
                path,
            } => QueryRecord::Location {
                declaration: flags & 1 != 0,
                hash: *hash,
                line: *line,
                column: *col,
                end_line: *end_line,
                end_column: *end_col,
                path: (*path).to_path_buf(),
            },
            Self::Markup { format, text } => QueryRecord::Markup {
                markdown: *format == LSP_MARKUP_MARKDOWN,
                text: (*text).to_owned(),
            },
            Self::Symbol {
                sym_kind,
                flags,
                depth,
                line,
                col,
                end_line,
                end_col,
                name,
                path,
            } => QueryRecord::Symbol {
                symbol_kind: *sym_kind,
                deprecated: flags & LSP_SYMBOL_DEPRECATED != 0,
                depth: *depth,
                line: *line,
                column: *col,
                end_line: *end_line,
                end_column: *end_col,
                name: (*name).to_owned(),
                path: path.map(Path::to_path_buf),
            },
            Self::Edit {
                hash,
                line,
                col,
                end_line,
                end_col,
                new_text,
                path,
                ..
            } => QueryRecord::Edit {
                hash: *hash,
                line: *line,
                column: *col,
                end_line: *end_line,
                end_column: *end_col,
                new_text: (*new_text).to_owned(),
                path: (*path).to_path_buf(),
            },
            Self::Completion {
                item_kind,
                flags,
                line,
                col,
                end_line,
                end_col,
                label,
                insert,
                detail,
            } => QueryRecord::Completion {
                item_kind: *item_kind,
                deprecated: flags & LSP_COMPLETION_DEPRECATED != 0,
                preselect: flags & LSP_COMPLETION_PRESELECT != 0,
                snippet: flags & LSP_COMPLETION_SNIPPET != 0,
                line: *line,
                column: *col,
                end_line: *end_line,
                end_column: *end_col,
                label: (*label).to_owned(),
                insert: (*insert).to_owned(),
                detail: (*detail).to_owned(),
            },
            Self::Signature {
                flags,
                active_param,
                param_start,
                param_end,
                label,
                doc,
            } => QueryRecord::Signature {
                active: flags & LSP_SIGNATURE_ACTIVE != 0,
                active_parameter: (*active_param != LSP_SIGNATURE_NO_PARAM)
                    .then_some(*active_param),
                parameter_start: *param_start,
                parameter_end: *param_end,
                label: (*label).to_owned(),
                documentation: (*doc).to_owned(),
            },
            Self::Action {
                flags,
                edit_count,
                title,
                action_kind,
                disabled_reason,
            } => QueryRecord::Action {
                preferred: flags & LSP_ACTION_PREFERRED != 0,
                disabled: flags & LSP_ACTION_DISABLED != 0,
                edit_count: *edit_count,
                title: (*title).to_owned(),
                action_kind: (*action_kind).to_owned(),
                disabled_reason: (*disabled_reason).to_owned(),
            },
        }
    }
}

/// Per-response source of file text: an owned open-set snapshot first
/// (the exact text the backend holds, as `Arc` handles — never copied,
/// since completion and signature queries run at typing frequency, and
/// owned so encoding can leave the engine thread), then a disk-read
/// cache.
pub struct TextSource {
    open: HashMap<PathBuf, IndexedText>,
    disk: HashMap<PathBuf, Option<IndexedText>>,
}

impl TextSource {
    pub fn new(open: HashMap<PathBuf, IndexedText>) -> Self {
        TextSource {
            open,
            disk: HashMap::new(),
        }
    }

    fn lookup(&mut self, path: &Path) -> Option<&IndexedText> {
        if self.open.contains_key(path) {
            return self.open.get(path);
        }
        self.disk
            .entry(path.to_path_buf())
            .or_insert_with(|| IndexedText::from_disk(path))
            .as_ref()
    }
}

/// Budgets applied while appending records.
pub struct RecordSink<'a> {
    pub records: &'a mut Vec<QueryRecord>,
    pub entries_left: usize,
    pub bytes_max: usize,
    pub bytes_used: usize,
    pub truncated: bool,
    /// A `RENAME` plan dropped whole-file operations it cannot project.
    pub incomplete: bool,
}

impl<'a> RecordSink<'a> {
    pub fn push(&mut self, record: &ProjectedRecord<'_>) {
        let record = record.owned();
        let size = crate::model::query_record_size(&record);
        if self.entries_left == 0 || self.bytes_used.saturating_add(size) > self.bytes_max {
            self.truncated = true;
            return;
        }
        self.entries_left -= 1;
        self.bytes_used += size;
        self.records.push(record);
    }

    /// Append a record group atomically. ACTION plus its declared EDIT
    /// records must never be split by the response budget.
    pub fn push_group(&mut self, records: &[ProjectedRecord<'_>]) {
        let owned: Vec<QueryRecord> = records.iter().map(ProjectedRecord::owned).collect();
        let added: usize = owned.iter().map(crate::model::query_record_size).sum();
        if records.len() > self.entries_left
            || self.bytes_used.saturating_add(added) > self.bytes_max
        {
            self.truncated = true;
            return;
        }
        self.entries_left -= records.len();
        self.bytes_used += added;
        self.records.extend(owned);
    }
}

/// One transcoded YAS range.
pub struct NativeRange {
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

/// A range in the server's own units, for when no text is available to
/// transcode against (unreadable or non-file targets).
pub fn raw_range(range: &Value) -> NativeRange {
    NativeRange {
        line: range["start"]["line"].as_u64().unwrap_or(0) as u32,
        col: range["start"]["character"].as_u64().unwrap_or(0) as u32,
        end_line: range["end"]["line"].as_u64().unwrap_or(0) as u32,
        end_col: range["end"]["character"].as_u64().unwrap_or(0) as u32,
    }
}

/// LSP `{start, end}` range → YAS byte columns, against `src`.
pub fn range_to_native(range: &Value, src: &IndexedText, enc: PositionEncoding) -> NativeRange {
    let pos = |p: &Value| -> (u32, u32) {
        let line = p["line"].as_u64().unwrap_or(0) as u32;
        let character = p["character"].as_u64().unwrap_or(0) as u32;
        (line, src.col_from_encoding(line, character, enc))
    };
    let (line, col) = pos(&range["start"]);
    let (end_line, end_col) = pos(&range["end"]);
    NativeRange {
        line,
        col,
        end_line,
        end_col,
    }
}

/// One `Location`/`LocationLink` → a `LOCATION` record. Non-`file:`
/// URIs are dropped rather than mis-projected.
fn push_location(
    sink: &mut RecordSink<'_>,
    src: &mut TextSource,
    uri: &str,
    range: &Value,
    enc: PositionEncoding,
) {
    let Some(path) = text::uri_to_path(uri) else {
        return;
    };
    let (range, hash) = match src.lookup(&path) {
        Some(s) => (range_to_native(range, s, enc), s.hash()),
        // Unreadable target: emit the server's raw positions with an
        // unknown hash — a byte-identical answer is better than none
        // for ASCII, and the zero hash marks it unverified.
        None => (raw_range(range), LSP_HASH_NONE),
    };
    sink.push(&ProjectedRecord::Location {
        flags: 0,
        hash,
        line: range.line,
        col: range.col,
        end_line: range.end_line,
        end_col: range.end_col,
        path: &path,
    });
}

/// `textDocument/definition` (and references): `Location | Location[]
/// | LocationLink[] | null`.
pub fn locations(
    sink: &mut RecordSink<'_>,
    src: &mut TextSource,
    result: &Value,
    enc: PositionEncoding,
) {
    let items: Vec<&Value> = match result {
        Value::Array(items) => items.iter().collect(),
        Value::Object(_) => vec![result],
        _ => return,
    };
    for item in items {
        if let Some(uri) = item["uri"].as_str() {
            push_location(sink, src, uri, &item["range"], enc);
        } else if let Some(uri) = item["targetUri"].as_str() {
            // LocationLink: the selection range is the jump target.
            let range = if item["targetSelectionRange"].is_object() {
                &item["targetSelectionRange"]
            } else {
                &item["targetRange"]
            };
            push_location(sink, src, uri, range, enc);
        }
    }
}

/// `textDocument/hover`: `contents` is `MarkupContent | MarkedString |
/// MarkedString[]`; everything becomes one markup record (plus a
/// `LOCATION` for the hovered range when the server reports one).
pub fn hover(
    sink: &mut RecordSink<'_>,
    src: &mut TextSource,
    query_path: &Path,
    result: &Value,
    enc: PositionEncoding,
) {
    let contents = &result["contents"];
    let mut format = LSP_MARKUP_MARKDOWN;
    let mut body = String::new();
    let append_marked = |body: &mut String, item: &Value| {
        if let Some(s) = item.as_str() {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str(s);
        } else if let (Some(language), Some(value)) =
            (item["language"].as_str(), item["value"].as_str())
        {
            if !body.is_empty() {
                body.push_str("\n\n");
            }
            body.push_str(&format!("```{language}\n{value}\n```"));
        }
    };
    if let Some(kind) = contents["kind"].as_str() {
        // MarkupContent.
        if kind == "plaintext" {
            format = LSP_MARKUP_PLAIN;
        }
        body = contents["value"].as_str().unwrap_or_default().to_string();
    } else if let Some(items) = contents.as_array() {
        for item in items {
            append_marked(&mut body, item);
        }
    } else {
        append_marked(&mut body, contents);
    }
    if body.is_empty() {
        return;
    }
    sink.push(&ProjectedRecord::Markup {
        format,
        text: &body,
    });
    if result["range"].is_object()
        && let Some(s) = src.lookup(query_path)
    {
        let range = range_to_native(&result["range"], s, enc);
        sink.push(&ProjectedRecord::Location {
            flags: 0,
            hash: s.hash(),
            line: range.line,
            col: range.col,
            end_line: range.end_line,
            end_col: range.end_col,
            path: query_path,
        });
    }
}

fn symbol_flags(item: &Value) -> u8 {
    let mut flags = 0;
    if item["deprecated"].as_bool() == Some(true) {
        flags |= LSP_SYMBOL_DEPRECATED;
    }
    if let Some(tags) = item["tags"].as_array()
        && tags.iter().any(|t| t.as_u64() == Some(1))
    {
        flags |= LSP_SYMBOL_DEPRECATED;
    }
    flags
}

/// `textDocument/documentSymbol`: hierarchical `DocumentSymbol[]`
/// (flattened pre-order via `depth`) or flat `SymbolInformation[]`.
pub fn doc_symbols(
    sink: &mut RecordSink<'_>,
    src: &mut TextSource,
    query_path: &Path,
    result: &Value,
    enc: PositionEncoding,
) {
    let Some(items) = result.as_array() else {
        return;
    };
    // An owned handle to the query file's text (two Arc bumps): the
    // tree branch and the per-item lookups below cannot share a live
    // borrow of `src`.
    let query_src: Option<IndexedText> = src.lookup(query_path).cloned();
    for item in items {
        if item["range"].is_object() {
            // DocumentSymbol tree.
            flatten_doc_symbol(sink, query_path, query_src.as_ref(), item, 0, enc);
        } else if let Some(uri) = item["location"]["uri"].as_str() {
            // SymbolInformation.
            let Some(path) = text::uri_to_path(uri) else {
                continue;
            };
            let looked = src.lookup(&path);
            push_symbol(
                sink,
                &path,
                looked,
                item,
                &item["location"]["range"],
                0,
                enc,
            );
        }
    }
}

fn flatten_doc_symbol(
    sink: &mut RecordSink<'_>,
    path: &Path,
    src: Option<&IndexedText>,
    item: &Value,
    depth: u8,
    enc: PositionEncoding,
) {
    push_symbol(sink, path, src, item, &item["range"], depth, enc);
    if let Some(children) = item["children"].as_array() {
        for child in children {
            flatten_doc_symbol(sink, path, src, child, depth.saturating_add(1), enc);
        }
    }
}

fn push_symbol(
    sink: &mut RecordSink<'_>,
    path: &Path,
    src: Option<&IndexedText>,
    item: &Value,
    range: &Value,
    depth: u8,
    enc: PositionEncoding,
) {
    let name = item["name"].as_str().unwrap_or_default();
    let sym_kind = item["kind"].as_u64().unwrap_or(0) as u8;
    let wr = match src {
        Some(s) => range_to_native(range, s, enc),
        None => raw_range(range),
    };
    sink.push(&ProjectedRecord::Symbol {
        sym_kind,
        flags: symbol_flags(item),
        depth,
        line: wr.line,
        col: wr.col,
        end_line: wr.end_line,
        end_col: wr.end_col,
        name,
        path: Some(path),
    });
}

/// `workspace/symbol`: `SymbolInformation[] | WorkspaceSymbol[]`.
/// 3.17 `WorkspaceSymbol` may carry a location without a range; those
/// emit the zero range (a `workspaceSymbol/resolve` round remains a
/// server-side improvement that needs no protocol change).
pub fn ws_symbols(
    sink: &mut RecordSink<'_>,
    src: &mut TextSource,
    result: &Value,
    enc: PositionEncoding,
) {
    let Some(items) = result.as_array() else {
        return;
    };
    for item in items {
        let Some(uri) = item["location"]["uri"].as_str() else {
            continue;
        };
        let Some(path) = text::uri_to_path(uri) else {
            continue;
        };
        let looked = src.lookup(&path);
        push_symbol(
            sink,
            &path,
            looked,
            item,
            &item["location"]["range"],
            0,
            enc,
        );
    }
}

/// `textDocument/rename`: a `WorkspaceEdit` in either encoding →
/// `EDIT` records. File create/rename/delete operations have no v1
/// projection and are skipped.
pub fn rename_edits(
    sink: &mut RecordSink<'_>,
    src: &mut TextSource,
    result: &Value,
    enc: PositionEncoding,
) {
    let push_edits = |sink: &mut RecordSink<'_>, src: &mut TextSource, uri: &str, edits: &Value| {
        let Some(path) = text::uri_to_path(uri) else {
            return;
        };
        let Some(edits) = edits.as_array() else {
            return;
        };
        let looked = src.lookup(&path);
        for edit in edits {
            let new_text = edit["newText"].as_str().unwrap_or_default();
            let (wr, hash) = match looked {
                Some(s) => (range_to_native(&edit["range"], s, enc), s.hash()),
                None => (raw_range(&edit["range"]), LSP_HASH_NONE),
            };
            sink.push(&ProjectedRecord::Edit {
                flags: 0,
                hash,
                line: wr.line,
                col: wr.col,
                end_line: wr.end_line,
                end_col: wr.end_col,
                new_text,
                path: &path,
            });
        }
    };
    // `documentChanges` and `changes` are mutually exclusive encodings
    // of the same edit set; the former supersedes the latter when
    // present, so never emit from both or every edit is duplicated.
    if let Some(doc_changes) = result["documentChanges"].as_array() {
        for change in doc_changes {
            if let Some(uri) = change["textDocument"]["uri"].as_str() {
                push_edits(sink, src, uri, &change["edits"]);
            } else if change["kind"].is_string() {
                // A create/rename/delete file operation: it has no
                // textDocument edits array, and v1 has no projection for
                // it. Flag the plan incomplete rather than presenting a
                // partial rename as whole.
                sink.incomplete = true;
            }
        }
    } else if let Some(changes) = result["changes"].as_object() {
        for (uri, edits) in changes {
            push_edits(sink, src, uri, edits);
        }
    }
}

struct OwnedEdit {
    hash: [u8; 16],
    range: NativeRange,
    new_text: String,
    path: PathBuf,
}

fn owned_edit(src: &mut TextSource, path: &Path, edit: &Value, enc: PositionEncoding) -> OwnedEdit {
    let (range, hash) = match src.lookup(path) {
        Some(text) => (range_to_native(&edit["range"], text, enc), text.hash()),
        None => (raw_range(&edit["range"]), LSP_HASH_NONE),
    };
    OwnedEdit {
        hash,
        range,
        new_text: edit["newText"].as_str().unwrap_or_default().to_owned(),
        path: path.to_path_buf(),
    }
}

fn collect_workspace_edits(
    src: &mut TextSource,
    workspace_edit: &Value,
    enc: PositionEncoding,
) -> (Vec<OwnedEdit>, bool) {
    let mut out = Vec::new();
    let mut incomplete = false;
    if let Some(changes) = workspace_edit["documentChanges"].as_array() {
        for change in changes {
            let Some(uri) = change["textDocument"]["uri"].as_str() else {
                if change["kind"].is_string() {
                    incomplete = true;
                }
                continue;
            };
            let Some(path) = text::uri_to_path(uri) else {
                continue;
            };
            if let Some(edits) = change["edits"].as_array() {
                out.extend(edits.iter().map(|edit| owned_edit(src, &path, edit, enc)));
            }
        }
    } else if let Some(changes) = workspace_edit["changes"].as_object() {
        for (uri, edits) in changes {
            let Some(path) = text::uri_to_path(uri) else {
                continue;
            };
            if let Some(edits) = edits.as_array() {
                out.extend(edits.iter().map(|edit| owned_edit(src, &path, edit, enc)));
            }
        }
    }
    (out, incomplete)
}

fn project_edit(edit: &OwnedEdit) -> ProjectedRecord<'_> {
    ProjectedRecord::Edit {
        flags: 0,
        hash: edit.hash,
        line: edit.range.line,
        col: edit.range.col,
        end_line: edit.range.end_line,
        end_col: edit.range.end_col,
        new_text: &edit.new_text,
        path: &edit.path,
    }
}

/// `textDocument/codeAction`: project only typed edit-bearing actions. An
/// action with an opaque command and no edit is omitted and marks the page
/// incomplete; disabled edit-less actions remain useful UI information.
pub fn code_actions(
    sink: &mut RecordSink<'_>,
    src: &mut TextSource,
    result: &Value,
    enc: PositionEncoding,
) {
    let Some(actions) = result.as_array() else {
        return;
    };
    for action in actions {
        let Some(title) = action["title"].as_str().filter(|title| !title.is_empty()) else {
            continue;
        };
        let disabled_reason = action["disabled"]["reason"].as_str().unwrap_or_default();
        let (edits, incomplete) = collect_workspace_edits(src, &action["edit"], enc);
        if incomplete {
            sink.incomplete = true;
        }
        if edits.is_empty() && action["command"].is_object() && disabled_reason.is_empty() {
            sink.incomplete = true;
            continue;
        }
        let mut flags = 0;
        if action["isPreferred"].as_bool() == Some(true) {
            flags |= LSP_ACTION_PREFERRED;
        }
        if !disabled_reason.is_empty() {
            flags |= LSP_ACTION_DISABLED;
        }
        let edit_count = match u16::try_from(edits.len()) {
            Ok(count) => count,
            Err(_) => {
                sink.truncated = true;
                continue;
            }
        };
        let mut records = Vec::with_capacity(edits.len() + 1);
        records.push(ProjectedRecord::Action {
            flags,
            edit_count,
            title,
            action_kind: action["kind"].as_str().unwrap_or_default(),
            disabled_reason,
        });
        records.extend(edits.iter().map(project_edit));
        sink.push_group(&records);
    }
}

/// `textDocument/formatting` and `textDocument/rangeFormatting` return edits
/// for the queried document, projected with the same exact content identity
/// and UTF-8 range conversion used by rename plans.
pub fn formatting_edits(
    sink: &mut RecordSink<'_>,
    src: &mut TextSource,
    query_path: &Path,
    result: &Value,
    enc: PositionEncoding,
) {
    let Some(edits) = result.as_array() else {
        return;
    };
    for edit in edits {
        let edit = owned_edit(src, query_path, edit, enc);
        sink.push(&project_edit(&edit));
    }
}

/// `textDocument/completion`: `CompletionItem[] | CompletionList`.
/// Items are emitted in `sortText` order so budget truncation keeps the
/// server's best-ranked items; a `CompletionList.isIncomplete` becomes
/// the response `INCOMPLETE` flag (retype should re-query).
pub fn completions(
    sink: &mut RecordSink<'_>,
    src: &mut TextSource,
    query_path: &Path,
    result: &Value,
    enc: PositionEncoding,
) {
    let items = if let Some(items) = result.as_array() {
        items
    } else if let Some(items) = result["items"].as_array() {
        if result["isIncomplete"].as_bool() == Some(true) {
            sink.incomplete = true;
        }
        items
    } else {
        return;
    };
    // An owned handle to the query file's text for range transcoding
    // (edit ranges always target the queried document).
    let query_src: Option<IndexedText> = src.lookup(query_path).cloned();
    fn sort_key(item: &Value) -> &str {
        item["sortText"]
            .as_str()
            .or_else(|| item["label"].as_str())
            .unwrap_or("")
    }
    let mut ordered: Vec<&Value> = items.iter().collect();
    ordered.sort_by(|a, b| sort_key(a).cmp(sort_key(b)));
    for item in ordered {
        let Some(label) = item["label"].as_str().filter(|l| !l.is_empty()) else {
            continue;
        };
        // The record's string fields carry u16 length prefixes; a
        // pathological item would silently wrap them and corrupt the
        // stream, so drop it instead.
        let fits = |s: &str| s.len() <= u16::MAX as usize;
        if !fits(label)
            || !item["textEdit"]["newText"].as_str().is_none_or(fits)
            || !item["insertText"].as_str().is_none_or(fits)
            || !item["detail"].as_str().is_none_or(fits)
        {
            continue;
        }
        let mut flags = 0u8;
        if item["deprecated"].as_bool() == Some(true)
            || item["tags"]
                .as_array()
                .is_some_and(|tags| tags.iter().any(|t| t.as_u64() == Some(1)))
        {
            flags |= LSP_COMPLETION_DEPRECATED;
        }
        // InsertTextFormat 2 = Snippet.
        if item["insertTextFormat"].as_u64() == Some(2) {
            flags |= LSP_COMPLETION_SNIPPET;
        }
        if item["preselect"].as_bool() == Some(true) {
            flags |= LSP_COMPLETION_PRESELECT;
        }
        // `textEdit` is `TextEdit {range}` or `InsertReplaceEdit
        // {insert, replace}`; the replace range is the primary edit.
        let edit = &item["textEdit"];
        let range = if edit["range"].is_object() {
            Some(&edit["range"])
        } else if edit["replace"].is_object() {
            Some(&edit["replace"])
        } else {
            None
        };
        let wr = match (range, &query_src) {
            (Some(range), Some(s)) => range_to_native(range, s, enc),
            (Some(range), None) => raw_range(range),
            // No edit range: the zero range tells the client to pick
            // its own word boundary.
            (None, _) => NativeRange {
                line: 0,
                col: 0,
                end_line: 0,
                end_col: 0,
            },
        };
        let insert = edit["newText"]
            .as_str()
            .or_else(|| item["insertText"].as_str())
            .unwrap_or("");
        sink.push(&ProjectedRecord::Completion {
            item_kind: item["kind"].as_u64().unwrap_or(0) as u8,
            flags,
            line: wr.line,
            col: wr.col,
            end_line: wr.end_line,
            end_col: wr.end_col,
            label,
            // Empty insert means "insert the label".
            insert: if insert == label { "" } else { insert },
            detail: item["detail"].as_str().unwrap_or(""),
        });
    }
}

/// A UTF-16 code-unit offset into `s` → UTF-8 byte offset, clamped.
/// LSP `ParameterInformation.label` offsets are always UTF-16,
/// regardless of the negotiated position encoding.
fn utf16_offset_to_byte(s: &str, offset: u64) -> usize {
    let mut units = 0u64;
    for (byte, ch) in s.char_indices() {
        if units >= offset {
            return byte;
        }
        units += ch.len_utf16() as u64;
    }
    s.len()
}

/// `textDocument/signatureHelp`: the active signature is emitted first
/// with the `ACTIVE` flag; the active parameter's label range is
/// transcoded to UTF-8 bytes within the signature label.
pub fn signatures(sink: &mut RecordSink<'_>, result: &Value) {
    let Some(sigs) = result["signatures"].as_array() else {
        return;
    };
    let active_sig = result["activeSignature"]
        .as_u64()
        .map(|i| i as usize)
        .filter(|i| *i < sigs.len())
        .unwrap_or(0);
    let global_active_param = result["activeParameter"].as_u64();
    let order = std::iter::once(active_sig).chain((0..sigs.len()).filter(|i| *i != active_sig));
    for idx in order {
        let sig = &sigs[idx];
        let Some(label) = sig["label"]
            .as_str()
            .filter(|l| !l.is_empty() && l.len() <= u16::MAX as usize)
        else {
            continue;
        };
        // An omitted activeParameter defaults to 0 per the LSP spec, so
        // the first parameter highlights the moment "(" is typed.
        let active_param = sig["activeParameter"]
            .as_u64()
            .or(global_active_param)
            .or_else(|| {
                sig["parameters"]
                    .as_array()
                    .is_some_and(|p| !p.is_empty())
                    .then_some(0)
            });
        let (active_param, param_start, param_end) = match active_param {
            Some(ap) => {
                let bounds = sig["parameters"]
                    .as_array()
                    .and_then(|params| params.get(ap as usize))
                    .and_then(|p| match &p["label"] {
                        // `[start, end)` in UTF-16 code units into the
                        // signature label (the LSP contract for offsets).
                        Value::Array(arr) => {
                            let s = arr.first()?.as_u64()?;
                            let e = arr.get(1)?.as_u64()?;
                            Some((
                                utf16_offset_to_byte(label, s),
                                utf16_offset_to_byte(label, e),
                            ))
                        }
                        // A plain substring: locate it in the label.
                        Value::String(pl) => label.find(pl.as_str()).map(|s| (s, s + pl.len())),
                        _ => None,
                    })
                    .unwrap_or((0, 0));
                (
                    ap.min(u64::from(LSP_SIGNATURE_NO_PARAM - 1)) as u16,
                    bounds.0.min(u16::MAX as usize) as u16,
                    bounds.1.min(u16::MAX as usize) as u16,
                )
            }
            None => (LSP_SIGNATURE_NO_PARAM, 0, 0),
        };
        // `documentation` is a plain string or MarkupContent.
        let doc = sig["documentation"]
            .as_str()
            .or_else(|| sig["documentation"]["value"].as_str())
            .unwrap_or("");
        sink.push(&ProjectedRecord::Signature {
            flags: if idx == active_sig {
                LSP_SIGNATURE_ACTIVE
            } else {
                0
            },
            active_param,
            param_start,
            param_end,
            label,
            doc,
        });
    }
}
