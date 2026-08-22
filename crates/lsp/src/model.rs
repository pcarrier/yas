//! Semantic constants shared by the native language-intelligence engine.

pub type LspHash = [u8; 16];
pub const LSP_HASH_NONE: LspHash = [0; 16];

pub const LSP_STATUS_OK: u8 = 0;
pub const LSP_STATUS_UNKNOWN_ID: u8 = 1;
pub const LSP_STATUS_NOT_FOUND: u8 = 2;
pub const LSP_STATUS_WRONG_TYPE: u8 = 3;
pub const LSP_STATUS_PERMISSION: u8 = 4;
pub const LSP_STATUS_TOO_LARGE: u8 = 5;
pub const LSP_STATUS_BUDGET: u8 = 6;
pub const LSP_STATUS_INVALID: u8 = 7;
pub const LSP_STATUS_CANCELLED: u8 = 8;
pub const LSP_STATUS_OTHER: u8 = 9;
pub const LSP_STATUS_WARMING: u8 = 10;

pub const LSP_STREAM_STATE: u8 = 0;
pub const LSP_STREAM_DIAG: u8 = 1;
pub const LSP_QUERY_DEFINITION: u8 = 1;
pub const LSP_QUERY_REFERENCES: u8 = 2;
pub const LSP_QUERY_HOVER: u8 = 3;
pub const LSP_QUERY_DOC_SYMBOLS: u8 = 4;
pub const LSP_QUERY_WS_SYMBOLS: u8 = 5;
pub const LSP_QUERY_RENAME: u8 = 6;
pub const LSP_QUERY_COMPLETION: u8 = 7;
pub const LSP_QUERY_SIGNATURE: u8 = 8;
pub const LSP_QUERY_CODE_ACTIONS: u8 = 9;
pub const LSP_QUERY_FORMATTING: u8 = 10;
pub const LSP_REFS_INCLUDE_DECLARATION: u8 = 1 << 0;
pub const LSP_RESP_TRUNCATED: u8 = 1 << 0;
pub const LSP_RESP_INCOMPLETE: u8 = 1 << 1;

pub const LSP_PHASE_SPAWNING: u8 = 0;
pub const LSP_PHASE_INITIALIZING: u8 = 1;
pub const LSP_PHASE_INDEXING: u8 = 2;
pub const LSP_PHASE_READY: u8 = 3;
pub const LSP_PHASE_FAILED: u8 = 4;
pub const LSP_PROGRESS_UNKNOWN: u8 = 255;

pub const LSP_CAP_DEFINITION: u32 = 1 << 0;
pub const LSP_CAP_REFERENCES: u32 = 1 << 1;
pub const LSP_CAP_HOVER: u32 = 1 << 2;
pub const LSP_CAP_DOC_SYMBOLS: u32 = 1 << 3;
pub const LSP_CAP_WS_SYMBOLS: u32 = 1 << 4;
pub const LSP_CAP_RENAME: u32 = 1 << 5;
pub const LSP_CAP_COMPLETION: u32 = 1 << 6;
pub const LSP_CAP_SIGNATURE: u32 = 1 << 7;
pub const LSP_CAP_CODE_ACTIONS: u32 = 1 << 8;
pub const LSP_CAP_FORMATTING: u32 = 1 << 9;

pub const LSP_COMPLETION_DEPRECATED: u8 = 1 << 0;
pub const LSP_COMPLETION_SNIPPET: u8 = 1 << 1;
pub const LSP_COMPLETION_PRESELECT: u8 = 1 << 2;
pub const LSP_SIGNATURE_ACTIVE: u8 = 1 << 0;
pub const LSP_ACTION_PREFERRED: u8 = 1 << 0;
pub const LSP_ACTION_DISABLED: u8 = 1 << 1;
pub const LSP_SIGNATURE_NO_PARAM: u16 = u16::MAX;
pub const LSP_DIAG_UNNECESSARY: u8 = 1 << 0;
pub const LSP_DIAG_DEPRECATED: u8 = 1 << 1;
pub const LSP_MARKUP_PLAIN: u8 = 0;
pub const LSP_MARKUP_MARKDOWN: u8 = 1;
pub const LSP_SYMBOL_DEPRECATED: u8 = 1 << 0;

pub fn query_record_size(record: &crate::native::QueryRecord) -> usize {
    use crate::native::QueryRecord;
    match record {
        QueryRecord::Location { path, .. } => 48 + path.as_os_str().len(),
        QueryRecord::Markup { text, .. } => 8 + text.len(),
        QueryRecord::Symbol { name, path, .. } => {
            32 + name.len() + path.as_deref().map_or(0, |path| path.as_os_str().len())
        }
        QueryRecord::Edit { new_text, path, .. } => 44 + new_text.len() + path.as_os_str().len(),
        QueryRecord::Completion {
            label,
            insert,
            detail,
            ..
        } => 36 + label.len() + insert.len() + detail.len(),
        QueryRecord::Signature {
            label,
            documentation,
            ..
        } => 20 + label.len() + documentation.len(),
        QueryRecord::Action {
            title,
            action_kind,
            disabled_reason,
            ..
        } => 16 + title.len() + action_kind.len() + disabled_reason.len(),
    }
}

pub fn diagnostic_size(diagnostic: &crate::backend::CachedDiagnostic) -> usize {
    24 + diagnostic.code.len() + diagnostic.source.len() + diagnostic.msg.len()
}
