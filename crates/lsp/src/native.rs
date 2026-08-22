//! Protocol-neutral typed API for the native YAS server.
//!
//! Server code consumes these owned semantic values directly; no serialized
//! transport packet or mirrored codec crosses the crate boundary.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::model as engine;

pub type EventSink = Arc<dyn Fn(Event) -> bool + Send + Sync>;
pub type QuerySink = Arc<dyn Fn(QueryResponse) -> bool + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stream {
    State,
    Diagnostics,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    State {
        update_id: u32,
        servers: Vec<Server>,
    },
    Diagnostics {
        update_id: u32,
        full: bool,
        files: Vec<FileDiagnostics>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub server_ref: u16,
    pub phase: u8,
    pub progress_pct: u8,
    pub capabilities: Capabilities,
    pub epoch: u32,
    pub refused_edits: u32,
    pub rss_bytes: u64,
    pub id: String,
    pub message: String,
    pub root: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities(u32);

impl Capabilities {
    pub(crate) const fn from_engine(value: u32) -> Self {
        Self(value)
    }

    pub const fn definition(self) -> bool {
        self.0 & engine::LSP_CAP_DEFINITION != 0
    }
    pub const fn references(self) -> bool {
        self.0 & engine::LSP_CAP_REFERENCES != 0
    }
    pub const fn hover(self) -> bool {
        self.0 & engine::LSP_CAP_HOVER != 0
    }
    pub const fn document_symbols(self) -> bool {
        self.0 & engine::LSP_CAP_DOC_SYMBOLS != 0
    }
    pub const fn workspace_symbols(self) -> bool {
        self.0 & engine::LSP_CAP_WS_SYMBOLS != 0
    }
    pub const fn rename(self) -> bool {
        self.0 & engine::LSP_CAP_RENAME != 0
    }
    pub const fn completion(self) -> bool {
        self.0 & engine::LSP_CAP_COMPLETION != 0
    }
    pub const fn signature_help(self) -> bool {
        self.0 & engine::LSP_CAP_SIGNATURE != 0
    }
    pub const fn code_actions(self) -> bool {
        self.0 & engine::LSP_CAP_CODE_ACTIONS != 0
    }
    pub const fn formatting(self) -> bool {
        self.0 & engine::LSP_CAP_FORMATTING != 0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiagnostics {
    pub path: PathBuf,
    pub hash: [u8; 16],
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: u8,
    pub unnecessary: bool,
    pub deprecated: bool,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub code: String,
    pub source: String,
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryKind {
    Definition,
    References,
    Hover,
    DocumentSymbols,
    WorkspaceSymbols,
    Rename,
    Completion,
    SignatureHelp,
    CodeActions,
    Formatting,
}

impl QueryKind {
    pub(crate) const fn engine_kind(self) -> u8 {
        match self {
            Self::Definition => engine::LSP_QUERY_DEFINITION,
            Self::References => engine::LSP_QUERY_REFERENCES,
            Self::Hover => engine::LSP_QUERY_HOVER,
            Self::DocumentSymbols => engine::LSP_QUERY_DOC_SYMBOLS,
            Self::WorkspaceSymbols => engine::LSP_QUERY_WS_SYMBOLS,
            Self::Rename => engine::LSP_QUERY_RENAME,
            Self::Completion => engine::LSP_QUERY_COMPLETION,
            Self::SignatureHelp => engine::LSP_QUERY_SIGNATURE,
            Self::CodeActions => engine::LSP_QUERY_CODE_ACTIONS,
            Self::Formatting => engine::LSP_QUERY_FORMATTING,
        }
    }
}

pub struct QueryRequest<'a> {
    pub nonce: u16,
    pub kind: QueryKind,
    pub flags: u8,
    pub line: u32,
    pub column: u32,
    /// Absolute document path. `None` only for workspace-wide queries.
    pub path: Option<&'a Path>,
    pub argument: &'a str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    NotFound,
    Unsupported,
    Permission,
    ResourceExhausted,
    Invalid,
    Cancelled,
    Warming,
    Other,
}

impl Status {
    pub(crate) fn from_engine(value: u8) -> Self {
        match value {
            engine::LSP_STATUS_OK => Self::Ok,
            engine::LSP_STATUS_UNKNOWN_ID | engine::LSP_STATUS_NOT_FOUND => Self::NotFound,
            engine::LSP_STATUS_WRONG_TYPE => Self::Unsupported,
            engine::LSP_STATUS_PERMISSION => Self::Permission,
            engine::LSP_STATUS_TOO_LARGE | engine::LSP_STATUS_BUDGET => Self::ResourceExhausted,
            engine::LSP_STATUS_INVALID => Self::Invalid,
            engine::LSP_STATUS_CANCELLED => Self::Cancelled,
            engine::LSP_STATUS_WARMING => Self::Warming,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub status: Status,
    pub detail: String,
}

impl Failure {
    pub(crate) fn from_engine(status: u8, detail: String) -> Self {
        Self {
            status: Status::from_engine(status),
            detail,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryResponse {
    pub nonce: u16,
    pub status: Status,
    pub truncated: bool,
    pub incomplete: bool,
    pub detail: String,
    pub records: Vec<QueryRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryRecord {
    Location {
        declaration: bool,
        hash: [u8; 16],
        line: u32,
        column: u32,
        end_line: u32,
        end_column: u32,
        path: PathBuf,
    },
    Markup {
        markdown: bool,
        text: String,
    },
    Symbol {
        symbol_kind: u8,
        deprecated: bool,
        depth: u8,
        line: u32,
        column: u32,
        end_line: u32,
        end_column: u32,
        name: String,
        path: Option<PathBuf>,
    },
    Edit {
        hash: [u8; 16],
        line: u32,
        column: u32,
        end_line: u32,
        end_column: u32,
        new_text: String,
        path: PathBuf,
    },
    Completion {
        item_kind: u8,
        deprecated: bool,
        preselect: bool,
        snippet: bool,
        line: u32,
        column: u32,
        end_line: u32,
        end_column: u32,
        label: String,
        insert: String,
        detail: String,
    },
    Signature {
        active: bool,
        active_parameter: Option<u16>,
        parameter_start: u16,
        parameter_end: u16,
        label: String,
        documentation: String,
    },
    Action {
        preferred: bool,
        disabled: bool,
        edit_count: u16,
        title: String,
        action_kind: String,
        disabled_reason: String,
    },
}
