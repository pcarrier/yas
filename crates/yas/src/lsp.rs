//! YAS LSP family version 1 wire values.

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, limit_u32, limit_u64,
    put_bytes_u16, put_bytes_u32, put_len_u16, put_len_u32, put_string_u16, put_string_u32,
    put_u16, put_u32, put_u64, read_limit_u32, read_limit_u64,
};
use crate::fs::Path as FsPath;
use crate::prelude::*;
use crate::state::{Record, RecordKind, Watch as StateWatch};
use crate::transfer::{Descriptor, Direction, Mode, Reset, UploadStage};

pub const VERSION: u16 = crate::schema::lsp::VERSION;
pub const MAX_QUERY_RECORDS: usize = crate::schema::lsp::MAX_QUERY_RECORDS as usize;
pub const MAX_QUERY_BYTES: usize = crate::schema::lsp::MAX_QUERY_BYTES as usize;
pub const MAX_CURSOR_BYTES: usize = crate::schema::lsp::MAX_CURSOR_BYTES as usize;
pub const MAX_INLINE_BUFFER_BYTES: usize = crate::schema::lsp::MAX_INLINE_BUFFER_BYTES as usize;

/// Validate document bytes before publishing either an inline or staged LSP
/// overlay. The LSP engine accepts UTF-8 text only; staged Transfer
/// bytes are therefore checked by BUFFER_COMMIT rather than by its descriptor.
pub fn validate_buffer_content(content: &[u8]) -> Result<()> {
    core::str::from_utf8(content)
        .map(|_| ())
        .map_err(|_| Error::Invalid("LSP buffer content is not UTF-8"))
}

pub mod request_kind {
    pub use crate::schema::lsp::request::*;
}

pub mod event_kind {
    pub use crate::schema::lsp::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_workspaces_per_session: u32,
    pub max_watches_per_workspace: u32,
    pub max_query_records: u32,
    pub max_query_bytes: u32,
    pub max_cursor_bytes: u32,
    pub max_inline_buffer_bytes: u32,
    pub max_buffer_bytes: u64,
    pub max_buffers_per_workspace: u32,
    pub max_stages_per_session: u32,
    pub max_diagnostics_per_file: u32,
    pub max_servers: u32,
    pub max_concurrent_queries: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_workspaces_per_session: crate::schema::lsp::MAX_WORKSPACES_PER_SESSION as u32,
        max_watches_per_workspace: crate::schema::lsp::MAX_WATCHES_PER_WORKSPACE as u32,
        max_query_records: crate::schema::lsp::MAX_QUERY_RECORDS as u32,
        max_query_bytes: crate::schema::lsp::MAX_QUERY_BYTES as u32,
        max_cursor_bytes: crate::schema::lsp::MAX_CURSOR_BYTES as u32,
        max_inline_buffer_bytes: crate::schema::lsp::MAX_INLINE_BUFFER_BYTES as u32,
        max_buffer_bytes: crate::schema::lsp::MAX_BUFFER_BYTES,
        max_buffers_per_workspace: crate::schema::lsp::MAX_BUFFERS_PER_WORKSPACE as u32,
        max_stages_per_session: crate::schema::lsp::MAX_STAGES_PER_SESSION as u32,
        max_diagnostics_per_file: crate::schema::lsp::MAX_DIAGNOSTICS_PER_FILE as u32,
        max_servers: crate::schema::lsp::MAX_SERVERS as u32,
        max_concurrent_queries: crate::schema::lsp::MAX_CONCURRENT_QUERIES as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        if [
            (
                self.max_workspaces_per_session,
                hard.max_workspaces_per_session,
            ),
            (
                self.max_watches_per_workspace,
                hard.max_watches_per_workspace,
            ),
            (self.max_query_records, hard.max_query_records),
            (self.max_query_bytes, hard.max_query_bytes),
            (self.max_cursor_bytes, hard.max_cursor_bytes),
            (self.max_inline_buffer_bytes, hard.max_inline_buffer_bytes),
            (
                self.max_buffers_per_workspace,
                hard.max_buffers_per_workspace,
            ),
            (self.max_stages_per_session, hard.max_stages_per_session),
            (self.max_diagnostics_per_file, hard.max_diagnostics_per_file),
            (self.max_servers, hard.max_servers),
            (self.max_concurrent_queries, hard.max_concurrent_queries),
        ]
        .into_iter()
        .any(|(value, maximum)| value == 0 || value > maximum)
            || self.max_buffer_bytes == 0
            || self.max_buffer_bytes > hard.max_buffer_bytes
        {
            return Err(Error::Invalid("LSP family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_WORKSPACES_PER_SESSION,
                self.max_workspaces_per_session,
            ),
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_WATCHES_PER_WORKSPACE,
                self.max_watches_per_workspace,
            ),
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_QUERY_RECORDS,
                self.max_query_records,
            ),
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_QUERY_BYTES,
                self.max_query_bytes,
            ),
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_CURSOR_BYTES,
                self.max_cursor_bytes,
            ),
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_INLINE_BUFFER_BYTES,
                self.max_inline_buffer_bytes,
            ),
            limit_u64(
                crate::schema::lsp::LIMIT_MAX_BUFFER_BYTES,
                self.max_buffer_bytes,
            ),
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_BUFFERS_PER_WORKSPACE,
                self.max_buffers_per_workspace,
            ),
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_STAGES_PER_SESSION,
                self.max_stages_per_session,
            ),
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_DIAGNOSTICS_PER_FILE,
                self.max_diagnostics_per_file,
            ),
            limit_u32(crate::schema::lsp::LIMIT_MAX_SERVERS, self.max_servers),
            limit_u32(
                crate::schema::lsp::LIMIT_MAX_CONCURRENT_QUERIES,
                self.max_concurrent_queries,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required(
            extensions,
            &[
                crate::schema::lsp::LIMIT_MAX_WORKSPACES_PER_SESSION as u16,
                crate::schema::lsp::LIMIT_MAX_WATCHES_PER_WORKSPACE as u16,
                crate::schema::lsp::LIMIT_MAX_QUERY_RECORDS as u16,
                crate::schema::lsp::LIMIT_MAX_QUERY_BYTES as u16,
                crate::schema::lsp::LIMIT_MAX_CURSOR_BYTES as u16,
                crate::schema::lsp::LIMIT_MAX_INLINE_BUFFER_BYTES as u16,
                crate::schema::lsp::LIMIT_MAX_BUFFER_BYTES as u16,
                crate::schema::lsp::LIMIT_MAX_BUFFERS_PER_WORKSPACE as u16,
                crate::schema::lsp::LIMIT_MAX_STAGES_PER_SESSION as u16,
                crate::schema::lsp::LIMIT_MAX_DIAGNOSTICS_PER_FILE as u16,
                crate::schema::lsp::LIMIT_MAX_SERVERS as u16,
                crate::schema::lsp::LIMIT_MAX_CONCURRENT_QUERIES as u16,
            ],
        )?;
        let value = Self {
            max_workspaces_per_session: read_limit_u32(
                extensions,
                crate::schema::lsp::LIMIT_MAX_WORKSPACES_PER_SESSION,
            )?,
            max_watches_per_workspace: read_limit_u32(
                extensions,
                crate::schema::lsp::LIMIT_MAX_WATCHES_PER_WORKSPACE,
            )?,
            max_query_records: read_limit_u32(
                extensions,
                crate::schema::lsp::LIMIT_MAX_QUERY_RECORDS,
            )?,
            max_query_bytes: read_limit_u32(extensions, crate::schema::lsp::LIMIT_MAX_QUERY_BYTES)?,
            max_cursor_bytes: read_limit_u32(
                extensions,
                crate::schema::lsp::LIMIT_MAX_CURSOR_BYTES,
            )?,
            max_inline_buffer_bytes: read_limit_u32(
                extensions,
                crate::schema::lsp::LIMIT_MAX_INLINE_BUFFER_BYTES,
            )?,
            max_buffer_bytes: read_limit_u64(
                extensions,
                crate::schema::lsp::LIMIT_MAX_BUFFER_BYTES,
            )?,
            max_buffers_per_workspace: read_limit_u32(
                extensions,
                crate::schema::lsp::LIMIT_MAX_BUFFERS_PER_WORKSPACE,
            )?,
            max_stages_per_session: read_limit_u32(
                extensions,
                crate::schema::lsp::LIMIT_MAX_STAGES_PER_SESSION,
            )?,
            max_diagnostics_per_file: read_limit_u32(
                extensions,
                crate::schema::lsp::LIMIT_MAX_DIAGNOSTICS_PER_FILE,
            )?,
            max_servers: read_limit_u32(extensions, crate::schema::lsp::LIMIT_MAX_SERVERS)?,
            max_concurrent_queries: read_limit_u32(
                extensions,
                crate::schema::lsp::LIMIT_MAX_CONCURRENT_QUERIES,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn validate_handle(value: u64, what: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(what))
    } else {
        Ok(())
    }
}

fn validate_revision(value: u64, what: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(what))
    } else {
        Ok(())
    }
}

fn validate_operation_id(value: &[u8; 16]) -> Result<()> {
    if *value == [0; 16] {
        Err(Error::Invalid("zero LSP operation ID"))
    } else {
        Ok(())
    }
}

fn reject_unknown_required(extensions: &Extensions, known: &[u16]) -> Result<()> {
    extensions.validate()?;
    if extensions
        .0
        .iter()
        .any(|extension| extension.required && !known.contains(&extension.tag))
    {
        return Err(Error::Invalid("unknown required LSP extension"));
    }
    Ok(())
}

fn validate_name(value: &str, maximum: usize, what: &'static str) -> Result<()> {
    if value.is_empty() || value.len() > maximum || value.as_bytes().contains(&0) {
        return Err(Error::Invalid(what));
    }
    Ok(())
}

fn validate_platform_path(value: &[u8], what: &'static str) -> Result<()> {
    if value.is_empty()
        || value.len() > crate::schema::lsp::MAX_ROOT_BYTES as usize
        || value.contains(&0)
    {
        return Err(Error::Invalid(what));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Position {
    pub line: u32,
    pub byte_column: u32,
}

impl Position {
    fn encode_into(self, out: &mut Vec<u8>) {
        put_u32(out, self.line);
        put_u32(out, self.byte_column);
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        Ok(Self {
            line: decoder.u32()?,
            byte_column: decoder.u32()?,
        })
    }
}

impl Encode for Position {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_into(out);
        Ok(())
    }
}

impl Decode for Position {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextRange {
    pub start: Position,
    pub end: Position,
}

impl TextRange {
    fn validate(self) -> Result<()> {
        if self.start > self.end {
            return Err(Error::Invalid("reversed LSP text range"));
        }
        Ok(())
    }

    fn encode_into(self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        self.start.encode_into(out);
        self.end.encode_into(out);
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            start: Position::decode_from(decoder)?,
            end: Position::decode_from(decoder)?,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for TextRange {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_into(out)
    }
}

impl Decode for TextRange {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocumentTarget {
    pub path: FsPath,
    /// Zero selects the current disk snapshot; nonzero names a buffer revision.
    pub document_revision: u64,
    /// BLAKE3-256 of the exact selected bytes.
    pub content_hash: [u8; 32],
}

impl DocumentTarget {
    fn validate(&self) -> Result<()> {
        if self.path.components.is_empty()
            || self.document_revision != 0 && self.content_hash == [0; 32]
        {
            return Err(Error::Invalid("LSP document target"));
        }
        self.path.encode().map(|_| ())
    }

    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u64(out, self.document_revision);
        out.extend_from_slice(&self.content_hash);
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            path: FsPath::decode(decoder.len_bytes_u32()?)?,
            document_revision: decoder.u64()?,
            content_hash: decoder.array_32()?,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for DocumentTarget {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_into(out)
    }
}

impl Decode for DocumentTarget {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkspaceSource {
    Fs {
        root_handle: u64,
        root_path: FsPath,
    },
    PlatformPath(Vec<u8>),
    TerminalCwd {
        terminal_handle: u64,
        suffix: FsPath,
    },
}

impl WorkspaceSource {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Fs {
                root_handle,
                root_path,
            } => {
                validate_handle(*root_handle, "zero LSP FS root handle")?;
                root_path.encode().map(|_| ())
            }
            Self::PlatformPath(path) => validate_platform_path(path, "LSP platform root path"),
            Self::TerminalCwd {
                terminal_handle,
                suffix,
            } => {
                validate_handle(*terminal_handle, "zero LSP Terminal handle")?;
                suffix.encode().map(|_| ())
            }
        }
    }
}

impl Encode for WorkspaceSource {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        match self {
            Self::Fs {
                root_handle,
                root_path,
            } => {
                out.push(crate::schema::lsp::SOURCE_FS as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *root_handle);
                put_bytes_u32(out, &root_path.encode()?)?;
            }
            Self::PlatformPath(path) => {
                out.push(crate::schema::lsp::SOURCE_PLATFORM_PATH as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, path)?;
            }
            Self::TerminalCwd {
                terminal_handle,
                suffix,
            } => {
                out.push(crate::schema::lsp::SOURCE_TERMINAL_CWD as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *terminal_handle);
                put_bytes_u32(out, &suffix.encode()?)?;
            }
        }
        Ok(())
    }
}

impl Decode for WorkspaceSource {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("LSP workspace source reserved bytes"));
        }
        let value = match kind {
            value if value == crate::schema::lsp::SOURCE_FS as u8 => Self::Fs {
                root_handle: decoder.u64()?,
                root_path: FsPath::decode(decoder.len_bytes_u32()?)?,
            },
            value if value == crate::schema::lsp::SOURCE_PLATFORM_PATH as u8 => {
                Self::PlatformPath(decoder.len_bytes_u32()?.to_vec())
            }
            value if value == crate::schema::lsp::SOURCE_TERMINAL_CWD as u8 => Self::TerminalCwd {
                terminal_handle: decoder.u64()?,
                suffix: FsPath::decode(decoder.len_bytes_u32()?)?,
            },
            _ => return Err(Error::Invalid("LSP workspace source kind")),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub source: WorkspaceSource,
    pub open_mode: u8,
    pub diagnostics_settle_ms: u16,
    pub language: String,
    pub profile: String,
    pub initialization_options: Vec<u8>,
    pub extensions: Extensions,
}

impl Encode for Open {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.source.validate()?;
        let explicit = self.open_mode == crate::schema::lsp::OPEN_EXPLICIT as u8;
        let auto = self.open_mode == crate::schema::lsp::OPEN_AUTO_DISCOVER as u8;
        if (!explicit && !auto)
            || self.diagnostics_settle_ms > crate::schema::lsp::MAX_DIAGNOSTICS_SETTLE_MS as u16
            || self.initialization_options.len()
                > crate::schema::lsp::MAX_INITIALIZATION_BYTES as usize
        {
            return Err(Error::Invalid("LSP OPEN mode or limits"));
        }
        if explicit {
            validate_name(
                &self.language,
                crate::schema::lsp::MAX_LANGUAGE_BYTES as usize,
                "LSP language",
            )?;
            validate_name(
                &self.profile,
                crate::schema::lsp::MAX_PROFILE_BYTES as usize,
                "LSP profile",
            )?;
        } else if !self.language.is_empty()
            || !self.profile.is_empty()
            || !self.initialization_options.is_empty()
        {
            return Err(Error::Invalid("LSP auto-discovery metadata"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_bytes_u32(out, &self.source.encode()?)?;
        out.push(self.open_mode);
        out.push(0);
        put_u16(out, self.diagnostics_settle_ms);
        put_string_u16(out, &self.language)?;
        put_string_u16(out, &self.profile)?;
        put_bytes_u32(out, &self.initialization_options)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Open {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let source = WorkspaceSource::decode(decoder.len_bytes_u32()?)?;
        let open_mode = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("LSP OPEN reserved byte"));
        }
        let value = Self {
            source,
            open_mode,
            diagnostics_settle_ms: decoder.u16()?,
            language: decoder.string_u16()?,
            profile: decoder.string_u16()?,
            initialization_options: decoder.len_bytes_u32()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenResult {
    pub workspace_handle: u64,
    pub workspace_revision: u64,
    pub position_encoding: u8,
    pub backend_count: u16,
    pub capabilities: u64,
    pub canonical_root: Vec<u8>,
    pub extensions: Extensions,
}

pub fn open_no_backend_detail_extension(detail: &str) -> Result<Extension> {
    validate_name(
        detail,
        crate::schema::lsp::MAX_DETAIL_BYTES as usize,
        "LSP no-backend detail",
    )?;
    let mut value = Vec::new();
    put_string_u32(&mut value, detail)?;
    Ok(Extension {
        tag: crate::schema::lsp::OPEN_NO_BACKEND_DETAIL_EXTENSION as u16,
        required: true,
        value,
    })
}

fn decode_open_no_backend_detail(extensions: &Extensions) -> Result<Option<String>> {
    reject_unknown_required(
        extensions,
        &[crate::schema::lsp::OPEN_NO_BACKEND_DETAIL_EXTENSION as u16],
    )?;
    let Some(extension) = extensions.0.iter().find(|extension| {
        extension.tag == crate::schema::lsp::OPEN_NO_BACKEND_DETAIL_EXTENSION as u16
    }) else {
        return Ok(None);
    };
    if !extension.required {
        return Err(Error::Invalid("optional LSP no-backend detail"));
    }
    let mut decoder = Decoder::new(&extension.value);
    let detail = decoder.string_u32()?;
    decoder.finish()?;
    validate_name(
        &detail,
        crate::schema::lsp::MAX_DETAIL_BYTES as usize,
        "LSP no-backend detail",
    )?;
    Ok(Some(detail))
}

impl OpenResult {
    pub fn no_backend_detail(&self) -> Result<Option<String>> {
        decode_open_no_backend_detail(&self.extensions)
    }
}

impl Encode for OpenResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.workspace_handle, "zero LSP workspace handle")?;
        validate_revision(self.workspace_revision, "zero LSP workspace revision")?;
        if self.position_encoding != crate::schema::lsp::POSITION_UTF8 as u8
            || usize::from(self.backend_count) > crate::schema::lsp::MAX_SERVERS as usize
            || self.capabilities & !crate::schema::lsp::CAPABILITIES != 0
        {
            return Err(Error::Invalid("LSP workspace metadata"));
        }
        validate_platform_path(&self.canonical_root, "LSP canonical root")?;
        let detail = decode_open_no_backend_detail(&self.extensions)?;
        if (self.backend_count == 0) != detail.is_some() {
            return Err(Error::Invalid("LSP no-backend detail presence"));
        }
        put_u64(out, self.workspace_handle);
        put_u64(out, self.workspace_revision);
        out.push(self.position_encoding);
        out.push(0);
        put_u16(out, self.backend_count);
        put_u64(out, self.capabilities);
        put_bytes_u32(out, &self.canonical_root)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for OpenResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let workspace_handle = decoder.u64()?;
        let workspace_revision = decoder.u64()?;
        let position_encoding = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("LSP OPEN Result reserved byte"));
        }
        let value = Self {
            workspace_handle,
            workspace_revision,
            position_encoding,
            backend_count: decoder.u16()?,
            capabilities: decoder.u64()?,
            canonical_root: decoder.len_bytes_u32()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Close {
    pub workspace_handle: u64,
    pub extensions: Extensions,
}

impl Encode for Close {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.workspace_handle, "zero LSP workspace handle")?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.workspace_handle);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Close {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            workspace_handle: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    pub workspace_handle: u64,
    pub reason: u8,
    pub detail: String,
}

impl Encode for Closed {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.workspace_handle, "zero LSP workspace handle")?;
        if self.reason > crate::schema::lsp::CLOSED_RESOURCE_LIMIT as u8
            || self.detail.len() > crate::schema::lsp::MAX_DETAIL_BYTES as usize
        {
            return Err(Error::Invalid("LSP CLOSED metadata"));
        }
        put_u64(out, self.workspace_handle);
        out.push(self.reason);
        out.extend_from_slice(&[0; 3]);
        put_string_u32(out, &self.detail)
    }
}

impl Decode for Closed {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let workspace_handle = decoder.u64()?;
        let reason = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("LSP CLOSED reserved bytes"));
        }
        let value = Self {
            workspace_handle,
            reason,
            detail: decoder.string_u32()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    pub workspace_handle: u64,
    pub datasets: u16,
    pub state: StateWatch,
}

impl Encode for Watch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.workspace_handle, "zero LSP workspace handle")?;
        if self.datasets == 0 || self.datasets & !(crate::schema::lsp::WATCH_DATASETS as u16) != 0 {
            return Err(Error::Invalid("LSP WATCH datasets"));
        }
        put_u64(out, self.workspace_handle);
        put_u16(out, self.datasets);
        put_u16(out, 0);
        put_bytes_u32(out, &self.state.encode()?)
    }
}

impl Decode for Watch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let workspace_handle = decoder.u64()?;
        let datasets = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("LSP WATCH reserved field"));
        }
        let value = Self {
            workspace_handle,
            datasets,
            state: StateWatch::decode(decoder.len_bytes_u32()?)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryBody {
    Definition {
        target: DocumentTarget,
        position: Position,
    },
    References {
        target: DocumentTarget,
        position: Position,
        flags: u16,
    },
    Hover {
        target: DocumentTarget,
        position: Position,
    },
    DocumentSymbols {
        target: DocumentTarget,
    },
    WorkspaceSymbols {
        query: String,
    },
    Completion {
        target: DocumentTarget,
        position: Position,
        trigger_kind: u8,
        trigger: String,
    },
    CodeActions {
        target: DocumentTarget,
        range: TextRange,
        diagnostic_ids: Vec<u64>,
    },
    Formatting {
        target: DocumentTarget,
        range: Option<TextRange>,
        tab_width: u16,
        flags: u16,
    },
    Rename {
        target: DocumentTarget,
        position: Position,
        new_name: String,
    },
    SignatureHelp {
        target: DocumentTarget,
        position: Position,
    },
}

impl QueryBody {
    fn encode_body(&self) -> Result<(u16, Vec<u8>)> {
        let mut out = Vec::new();
        let kind = match self {
            Self::Definition { target, position } => {
                target.encode_into(&mut out)?;
                position.encode_into(&mut out);
                crate::schema::lsp::QUERY_DEFINITION
            }
            Self::References {
                target,
                position,
                flags,
            } => {
                if *flags & !(crate::schema::lsp::REFERENCES_FLAGS as u16) != 0 {
                    return Err(Error::Invalid("LSP REFERENCES flags"));
                }
                target.encode_into(&mut out)?;
                position.encode_into(&mut out);
                put_u16(&mut out, *flags);
                put_u16(&mut out, 0);
                crate::schema::lsp::QUERY_REFERENCES
            }
            Self::Hover { target, position } => {
                target.encode_into(&mut out)?;
                position.encode_into(&mut out);
                crate::schema::lsp::QUERY_HOVER
            }
            Self::DocumentSymbols { target } => {
                target.encode_into(&mut out)?;
                crate::schema::lsp::QUERY_DOCUMENT_SYMBOLS
            }
            Self::WorkspaceSymbols { query } => {
                // LSP defines an empty workspace-symbol query as an
                // unfiltered request. Keep the normal byte and NUL bounds,
                // but do not apply `validate_name`'s nonempty requirement.
                if query.len() > crate::schema::lsp::MAX_QUERY_TEXT_BYTES as usize
                    || query.as_bytes().contains(&0)
                {
                    return Err(Error::Invalid("LSP workspace symbol query"));
                }
                put_string_u16(&mut out, query)?;
                crate::schema::lsp::QUERY_WORKSPACE_SYMBOLS
            }
            Self::Completion {
                target,
                position,
                trigger_kind,
                trigger,
            } => {
                if *trigger_kind > crate::schema::lsp::COMPLETION_TRIGGER_CHARACTER as u8
                    || trigger.len() > crate::schema::lsp::MAX_TRIGGER_BYTES as usize
                    || *trigger_kind == crate::schema::lsp::COMPLETION_TRIGGER_CHARACTER as u8
                        && trigger.is_empty()
                    || *trigger_kind != crate::schema::lsp::COMPLETION_TRIGGER_CHARACTER as u8
                        && !trigger.is_empty()
                {
                    return Err(Error::Invalid("LSP completion trigger"));
                }
                target.encode_into(&mut out)?;
                position.encode_into(&mut out);
                out.push(*trigger_kind);
                out.extend_from_slice(&[0; 3]);
                put_string_u16(&mut out, trigger)?;
                crate::schema::lsp::QUERY_COMPLETION
            }
            Self::CodeActions {
                target,
                range,
                diagnostic_ids,
            } => {
                if diagnostic_ids.len() > crate::schema::lsp::MAX_DIAGNOSTIC_IDS as usize {
                    return Err(Error::Invalid("LSP code-action diagnostic count"));
                }
                target.encode_into(&mut out)?;
                range.encode_into(&mut out)?;
                put_len_u16(&mut out, diagnostic_ids.len())?;
                put_u16(&mut out, 0);
                for id in diagnostic_ids {
                    if *id == 0 {
                        return Err(Error::Invalid("zero LSP diagnostic ID"));
                    }
                    put_u64(&mut out, *id);
                }
                crate::schema::lsp::QUERY_CODE_ACTIONS
            }
            Self::Formatting {
                target,
                range,
                tab_width,
                flags,
            } => {
                if *tab_width == 0 || *flags & !(crate::schema::lsp::FORMATTING_FLAGS as u16) != 0 {
                    return Err(Error::Invalid("LSP formatting options"));
                }
                target.encode_into(&mut out)?;
                out.push(u8::from(range.is_some()));
                out.extend_from_slice(&[0; 3]);
                if let Some(range) = range {
                    range.encode_into(&mut out)?;
                }
                put_u16(&mut out, *tab_width);
                put_u16(&mut out, *flags);
                crate::schema::lsp::QUERY_FORMATTING
            }
            Self::Rename {
                target,
                position,
                new_name,
            } => {
                validate_name(
                    new_name,
                    crate::schema::lsp::MAX_SYMBOL_NAME_BYTES as usize,
                    "LSP rename name",
                )?;
                target.encode_into(&mut out)?;
                position.encode_into(&mut out);
                put_string_u16(&mut out, new_name)?;
                crate::schema::lsp::QUERY_RENAME
            }
            Self::SignatureHelp { target, position } => {
                target.encode_into(&mut out)?;
                position.encode_into(&mut out);
                crate::schema::lsp::QUERY_SIGNATURE_HELP
            }
        };
        Ok((kind as u16, out))
    }

    fn decode_body(kind: u16, input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = match kind {
            value if value == crate::schema::lsp::QUERY_DEFINITION as u16 => Self::Definition {
                target: DocumentTarget::decode_from(&mut decoder)?,
                position: Position::decode_from(&mut decoder)?,
            },
            value if value == crate::schema::lsp::QUERY_REFERENCES as u16 => {
                let target = DocumentTarget::decode_from(&mut decoder)?;
                let position = Position::decode_from(&mut decoder)?;
                let flags = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("LSP REFERENCES reserved field"));
                }
                Self::References {
                    target,
                    position,
                    flags,
                }
            }
            value if value == crate::schema::lsp::QUERY_HOVER as u16 => Self::Hover {
                target: DocumentTarget::decode_from(&mut decoder)?,
                position: Position::decode_from(&mut decoder)?,
            },
            value if value == crate::schema::lsp::QUERY_DOCUMENT_SYMBOLS as u16 => {
                Self::DocumentSymbols {
                    target: DocumentTarget::decode_from(&mut decoder)?,
                }
            }
            value if value == crate::schema::lsp::QUERY_WORKSPACE_SYMBOLS as u16 => {
                Self::WorkspaceSymbols {
                    query: decoder.string_u16()?,
                }
            }
            value if value == crate::schema::lsp::QUERY_COMPLETION as u16 => {
                let target = DocumentTarget::decode_from(&mut decoder)?;
                let position = Position::decode_from(&mut decoder)?;
                let trigger_kind = decoder.u8()?;
                if decoder.take(3)? != [0; 3] {
                    return Err(Error::Invalid("LSP completion reserved bytes"));
                }
                Self::Completion {
                    target,
                    position,
                    trigger_kind,
                    trigger: decoder.string_u16()?,
                }
            }
            value if value == crate::schema::lsp::QUERY_CODE_ACTIONS as u16 => {
                let target = DocumentTarget::decode_from(&mut decoder)?;
                let range = TextRange::decode_from(&mut decoder)?;
                let count = usize::from(decoder.u16()?);
                if decoder.u16()? != 0
                    || count > crate::schema::lsp::MAX_DIAGNOSTIC_IDS as usize
                    || count > decoder.remaining() / 8
                {
                    return Err(Error::Invalid("LSP diagnostic ID count"));
                }
                let mut diagnostic_ids = Vec::with_capacity(count);
                for _ in 0..count {
                    diagnostic_ids.push(decoder.u64()?);
                }
                Self::CodeActions {
                    target,
                    range,
                    diagnostic_ids,
                }
            }
            value if value == crate::schema::lsp::QUERY_FORMATTING as u16 => {
                let target = DocumentTarget::decode_from(&mut decoder)?;
                let present = decoder.u8()?;
                if present > 1 || decoder.take(3)? != [0; 3] {
                    return Err(Error::Invalid("LSP formatting range presence"));
                }
                Self::Formatting {
                    target,
                    range: if present != 0 {
                        Some(TextRange::decode_from(&mut decoder)?)
                    } else {
                        None
                    },
                    tab_width: decoder.u16()?,
                    flags: decoder.u16()?,
                }
            }
            value if value == crate::schema::lsp::QUERY_RENAME as u16 => Self::Rename {
                target: DocumentTarget::decode_from(&mut decoder)?,
                position: Position::decode_from(&mut decoder)?,
                new_name: decoder.string_u16()?,
            },
            value if value == crate::schema::lsp::QUERY_SIGNATURE_HELP as u16 => {
                Self::SignatureHelp {
                    target: DocumentTarget::decode_from(&mut decoder)?,
                    position: Position::decode_from(&mut decoder)?,
                }
            }
            _ => return Err(Error::Invalid("LSP query kind")),
        };
        decoder.finish()?;
        let (encoded_kind, _) = value.encode_body()?;
        if encoded_kind != kind {
            return Err(Error::Invalid("LSP query kind mismatch"));
        }
        Ok(value)
    }
}

impl Encode for QueryBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let (kind, body) = self.encode_body()?;
        put_u16(out, kind);
        put_u16(out, 0);
        out.extend_from_slice(&body);
        Ok(())
    }
}

impl Decode for QueryBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("LSP query flags"));
        }
        Self::decode_body(kind, decoder.rest())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub workspace_handle: u64,
    pub max_records: u16,
    pub cursor: Vec<u8>,
    pub initial_receive_credit: u64,
    pub body: QueryBody,
    pub extensions: Extensions,
}

impl Encode for Query {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.workspace_handle, "zero LSP workspace handle")?;
        if usize::from(self.max_records) > MAX_QUERY_RECORDS || self.cursor.len() > MAX_CURSOR_BYTES
        {
            return Err(Error::Invalid("LSP query page limits"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.workspace_handle);
        put_u16(out, self.max_records);
        put_u16(out, 0);
        put_bytes_u16(out, &self.cursor)?;
        put_u64(out, self.initial_receive_credit);
        put_bytes_u32(out, &self.body.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Query {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let workspace_handle = decoder.u64()?;
        let max_records = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("LSP QUERY reserved field"));
        }
        let value = Self {
            workspace_handle,
            max_records,
            cursor: decoder.len_bytes_u16()?.to_vec(),
            initial_receive_credit: decoder.u64()?,
            body: QueryBody::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferIdentity {
    pub buffer_handle: u64,
    pub buffer_revision: u64,
    pub workspace_revision: u64,
    pub byte_len: u64,
    pub content_hash: [u8; 32],
    pub extensions: Extensions,
}

impl BufferIdentity {
    fn validate(&self) -> Result<()> {
        validate_handle(self.buffer_handle, "zero LSP buffer handle")?;
        validate_revision(self.buffer_revision, "zero LSP buffer revision")?;
        validate_revision(self.workspace_revision, "zero LSP workspace revision")?;
        if self.byte_len > crate::schema::lsp::MAX_BUFFER_BYTES {
            return Err(Error::Invalid("LSP buffer length"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for BufferIdentity {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.buffer_handle);
        put_u64(out, self.buffer_revision);
        put_u64(out, self.workspace_revision);
        put_u64(out, self.byte_len);
        out.extend_from_slice(&self.content_hash);
        self.extensions.encode_tail(out)
    }
}

impl Decode for BufferIdentity {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            buffer_handle: decoder.u64()?,
            buffer_revision: decoder.u64()?,
            workspace_revision: decoder.u64()?,
            byte_len: decoder.u64()?,
            content_hash: decoder.array_32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferPut {
    pub workspace_handle: u64,
    pub operation_id: [u8; 16],
    /// Zero requires that no overlay exists; otherwise this is a CAS revision.
    pub expected_revision: u64,
    pub path: FsPath,
    pub content: Vec<u8>,
    pub extensions: Extensions,
}

impl Encode for BufferPut {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.workspace_handle, "zero LSP workspace handle")?;
        validate_operation_id(&self.operation_id)?;
        if self.path.components.is_empty() || self.content.len() > MAX_INLINE_BUFFER_BYTES {
            return Err(Error::Invalid("LSP inline buffer"));
        }
        validate_buffer_content(&self.content)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.workspace_handle);
        out.extend_from_slice(&self.operation_id);
        put_u64(out, self.expected_revision);
        put_bytes_u32(out, &self.path.encode()?)?;
        put_bytes_u32(out, &self.content)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for BufferPut {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            workspace_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            expected_revision: decoder.u64()?,
            path: FsPath::decode(decoder.len_bytes_u32()?)?,
            content: decoder.len_bytes_u32()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferBegin {
    pub workspace_handle: u64,
    pub expected_revision: u64,
    pub path: FsPath,
    pub byte_len: u64,
    pub content_hash: [u8; 32],
    pub initial_send_credit: u64,
    pub extensions: Extensions,
}

impl Encode for BufferBegin {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.workspace_handle, "zero LSP workspace handle")?;
        if self.path.components.is_empty()
            || self.byte_len == 0
            || self.byte_len > crate::schema::lsp::MAX_BUFFER_BYTES
            || self.initial_send_credit == 0
        {
            return Err(Error::Invalid("LSP staged buffer metadata"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.workspace_handle);
        put_u64(out, self.expected_revision);
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u64(out, self.byte_len);
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.initial_send_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for BufferBegin {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            workspace_handle: decoder.u64()?,
            expected_revision: decoder.u64()?,
            path: FsPath::decode(decoder.len_bytes_u32()?)?,
            byte_len: decoder.u64()?,
            content_hash: decoder.array_32()?,
            initial_send_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

fn validate_buffer_descriptor(descriptor: &Descriptor) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Byte
        || descriptor.direction != Direction::RECEIVER_TO_SENDER
        || descriptor.receiver_send_credit == 0
        || descriptor.sender_send_credit != 0
        || descriptor.max_item_bytes != 0
        || descriptor.content_family != crate::family::LSP
        || descriptor.content_kind != crate::schema::lsp::BUFFER_CONTENT_KIND as u16
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("LSP buffer Transfer descriptor"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferBeginResult {
    pub staging_handle: u64,
    pub descriptor: Descriptor,
    pub extensions: Extensions,
}

impl BufferBeginResult {
    fn validate(&self) -> Result<()> {
        validate_handle(self.staging_handle, "zero LSP staging handle")?;
        validate_buffer_descriptor(&self.descriptor)?;
        self.descriptor.require_upload_stage(self.staging_handle)?;
        reject_unknown_required(&self.extensions, &[])
    }

    /// Return the LSP buffer stage discarded when `reset` targets its upload.
    pub fn stage_discarded_by(&self, reset: &Reset) -> Result<Option<UploadStage>> {
        self.validate()?;
        reset.disposed_upload_stage_from(self.staging_handle, core::iter::once(&self.descriptor))
    }
}

impl Encode for BufferBeginResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.staging_handle);
        put_bytes_u32(out, &self.descriptor.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for BufferBeginResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            staging_handle: decoder.u64()?,
            descriptor: Descriptor::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferCommit {
    pub staging_handle: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for BufferCommit {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.staging_handle, "zero LSP staging handle")?;
        validate_operation_id(&self.operation_id)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.staging_handle);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for BufferCommit {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            staging_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferClose {
    pub buffer_handle: u64,
    pub expected_revision: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for BufferClose {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.buffer_handle, "zero LSP buffer handle")?;
        validate_revision(self.expected_revision, "zero expected LSP buffer revision")?;
        validate_operation_id(&self.operation_id)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.buffer_handle);
        put_u64(out, self.expected_revision);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for BufferClose {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            buffer_handle: decoder.u64()?,
            expected_revision: decoder.u64()?,
            operation_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListServers {
    /// Zero lists all session-visible servers; nonzero restricts to a workspace.
    pub workspace_handle: u64,
    pub extensions: Extensions,
}

impl Encode for ListServers {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.workspace_handle);
        self.extensions.encode_tail(out)
    }
}

impl Decode for ListServers {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            workspace_handle: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StopServer {
    pub server_handle: u64,
    pub generation: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for StopServer {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.server_handle, "zero LSP server handle")?;
        validate_revision(self.generation, "zero LSP server generation")?;
        validate_operation_id(&self.operation_id)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.server_handle);
        put_u64(out, self.generation);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for StopServer {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            server_handle: decoder.u64()?,
            generation: decoder.u64()?,
            operation_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypedRecord {
    pub kind: u16,
    pub required: bool,
    pub body: Vec<u8>,
}

impl TypedRecord {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        let len = 4usize
            .checked_add(self.body.len())
            .ok_or(Error::LengthOverflow)?;
        put_len_u32(out, len)?;
        put_u16(out, self.kind);
        put_u16(out, u16::from(self.required));
        out.extend_from_slice(&self.body);
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let bytes = decoder.len_bytes_u32()?;
        let mut record = Decoder::new(bytes);
        let kind = record.u16()?;
        let flags = record.u16()?;
        if flags & !1 != 0 {
            return Err(Error::Invalid("LSP typed record flags"));
        }
        Ok(Self {
            kind,
            required: flags & 1 != 0,
            body: record.rest().to_vec(),
        })
    }

    /// Encode one complete MESSAGE item for a query Transfer.
    pub fn encode_message(&self) -> Result<Vec<u8>> {
        QueryRecord::decode_typed(self)?;
        let mut out = Vec::new();
        self.encode_into(&mut out)?;
        if out.len() > MAX_QUERY_BYTES {
            return Err(Error::Invalid("LSP query message bytes"));
        }
        Ok(out)
    }

    /// Decode one complete MESSAGE item from a query Transfer.
    pub fn decode_message(input: &[u8]) -> Result<Self> {
        if input.len() > MAX_QUERY_BYTES {
            return Err(Error::Invalid("LSP query message bytes"));
        }
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        QueryRecord::decode_typed(&value)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocationRecord {
    pub path: FsPath,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub range: TextRange,
    pub flags: u16,
}

impl Encode for LocationRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.path.components.is_empty()
            || self.flags & !(crate::schema::lsp::LOCATION_FLAGS as u16) != 0
        {
            return Err(Error::Invalid("LSP location"));
        }
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u64(out, self.document_revision);
        out.extend_from_slice(&self.content_hash);
        self.range.encode_into(out)?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        Ok(())
    }
}

impl Decode for LocationRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            path: FsPath::decode(decoder.len_bytes_u32()?)?,
            document_revision: decoder.u64()?,
            content_hash: decoder.array_32()?,
            range: TextRange::decode_from(&mut decoder)?,
            flags: decoder.u16()?,
        };
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("LSP location reserved field"));
        }
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HoverRecord {
    pub target: LocationRecord,
    pub markup_kind: u8,
    pub content: Vec<u8>,
}

impl Encode for HoverRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.markup_kind > crate::schema::lsp::MARKUP_MARKDOWN as u8
            || self.content.len() > crate::schema::lsp::MAX_MARKUP_BYTES as usize
        {
            return Err(Error::Invalid("LSP hover markup"));
        }
        put_bytes_u32(out, &self.target.encode()?)?;
        out.push(self.markup_kind);
        out.extend_from_slice(&[0; 3]);
        put_bytes_u32(out, &self.content)
    }
}

impl Decode for HoverRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let target = LocationRecord::decode(decoder.len_bytes_u32()?)?;
        let markup_kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("LSP hover reserved bytes"));
        }
        let value = Self {
            target,
            markup_kind,
            content: decoder.len_bytes_u32()?.to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolRecord {
    pub symbol_kind: u16,
    pub flags: u16,
    pub depth: u16,
    pub name: String,
    pub detail: String,
    pub path: Option<FsPath>,
    pub content_hash: Option<[u8; 32]>,
    pub range: TextRange,
    pub selection_range: TextRange,
}

impl Encode for SymbolRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.symbol_kind > crate::schema::lsp::SYMBOL_TYPE_PARAMETER as u16
            || self.flags & !(crate::schema::lsp::SYMBOL_FLAGS as u16) != 0
            || self.path.is_some() != self.content_hash.is_some()
        {
            return Err(Error::Invalid("LSP symbol metadata"));
        }
        validate_name(
            &self.name,
            crate::schema::lsp::MAX_SYMBOL_NAME_BYTES as usize,
            "LSP symbol name",
        )?;
        if self.detail.len() > crate::schema::lsp::MAX_DETAIL_BYTES as usize {
            return Err(Error::Invalid("LSP symbol detail"));
        }
        put_u16(out, self.symbol_kind);
        put_u16(out, self.flags);
        put_u16(out, self.depth);
        put_u16(out, 0);
        put_string_u16(out, &self.name)?;
        put_string_u16(out, &self.detail)?;
        match &self.path {
            Some(path) => put_bytes_u32(out, &path.encode()?)?,
            None => put_bytes_u32(out, &[])?,
        }
        out.push(u8::from(self.content_hash.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(content_hash) = self.content_hash {
            out.extend_from_slice(&content_hash);
        }
        self.range.encode_into(out)?;
        self.selection_range.encode_into(out)
    }
}

impl Decode for SymbolRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let symbol_kind = decoder.u16()?;
        let flags = decoder.u16()?;
        let depth = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("LSP symbol reserved field"));
        }
        let name = decoder.string_u16()?;
        let detail = decoder.string_u16()?;
        let path = decoder.len_bytes_u32()?;
        let path = if path.is_empty() {
            None
        } else {
            Some(FsPath::decode(path)?)
        };
        let content_hash_present = decoder.u8()?;
        if content_hash_present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("LSP symbol content hash presence"));
        }
        let value = Self {
            symbol_kind,
            flags,
            depth,
            name,
            detail,
            path,
            content_hash: if content_hash_present != 0 {
                Some(decoder.array_32()?)
            } else {
                None
            },
            range: TextRange::decode_from(&mut decoder)?,
            selection_range: TextRange::decode_from(&mut decoder)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionRecord {
    pub item_kind: u16,
    pub flags: u16,
    pub label: String,
    pub detail: String,
    pub filter_text: String,
    pub insert_text: Vec<u8>,
    pub replacement_range: Option<TextRange>,
}

impl Encode for CompletionRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.item_kind > crate::schema::lsp::COMPLETION_TYPE_PARAMETER as u16
            || self.flags & !(crate::schema::lsp::COMPLETION_FLAGS as u16) != 0
            || self.insert_text.len() > crate::schema::lsp::MAX_EDIT_BYTES as usize
        {
            return Err(Error::Invalid("LSP completion item"));
        }
        validate_name(
            &self.label,
            crate::schema::lsp::MAX_SYMBOL_NAME_BYTES as usize,
            "LSP completion label",
        )?;
        if self.detail.len() > crate::schema::lsp::MAX_DETAIL_BYTES as usize
            || self.filter_text.len() > crate::schema::lsp::MAX_SYMBOL_NAME_BYTES as usize
        {
            return Err(Error::Invalid("LSP completion text"));
        }
        put_u16(out, self.item_kind);
        put_u16(out, self.flags);
        put_string_u16(out, &self.label)?;
        put_string_u16(out, &self.detail)?;
        put_string_u16(out, &self.filter_text)?;
        put_bytes_u32(out, &self.insert_text)?;
        out.push(u8::from(self.replacement_range.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(range) = self.replacement_range {
            range.encode_into(out)?;
        }
        Ok(())
    }
}

impl Decode for CompletionRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let item_kind = decoder.u16()?;
        let flags = decoder.u16()?;
        let label = decoder.string_u16()?;
        let detail = decoder.string_u16()?;
        let filter_text = decoder.string_u16()?;
        let insert_text = decoder.len_bytes_u32()?.to_vec();
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("LSP completion range presence"));
        }
        let value = Self {
            item_kind,
            flags,
            label,
            detail,
            filter_text,
            insert_text,
            replacement_range: if present != 0 {
                Some(TextRange::decode_from(&mut decoder)?)
            } else {
                None
            },
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditRecord {
    pub path: FsPath,
    pub expected_revision: u64,
    pub expected_content_hash: [u8; 32],
    pub range: TextRange,
    pub replacement: Vec<u8>,
}

impl EditRecord {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.path.components.is_empty()
            || self.replacement.len() > crate::schema::lsp::MAX_EDIT_BYTES as usize
        {
            return Err(Error::Invalid("LSP text edit"));
        }
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u64(out, self.expected_revision);
        out.extend_from_slice(&self.expected_content_hash);
        self.range.encode_into(out)?;
        put_bytes_u32(out, &self.replacement)
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            path: FsPath::decode(decoder.len_bytes_u32()?)?,
            expected_revision: decoder.u64()?,
            expected_content_hash: decoder.array_32()?,
            range: TextRange::decode_from(decoder)?,
            replacement: decoder.len_bytes_u32()?.to_vec(),
        };
        let mut ignored = Vec::new();
        value.encode_into(&mut ignored)?;
        Ok(value)
    }
}

impl Encode for EditRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_into(out)
    }
}

impl Decode for EditRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureRecord {
    pub flags: u16,
    pub active_parameter: u16,
    pub parameter_start: u32,
    pub parameter_end: u32,
    pub label: String,
    pub documentation: String,
}

impl Encode for SignatureRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let no_parameter =
            self.active_parameter == crate::schema::lsp::SIGNATURE_NO_ACTIVE_PARAMETER as u16;
        if self.flags & !(crate::schema::lsp::SIGNATURE_FLAGS as u16) != 0
            || self.label.is_empty()
            || self.label.len() > crate::schema::lsp::MAX_SYMBOL_NAME_BYTES as usize
            || self.documentation.len() > crate::schema::lsp::MAX_MARKUP_BYTES as usize
            || no_parameter && (self.parameter_start != 0 || self.parameter_end != 0)
            || !no_parameter
                && (self.parameter_start > self.parameter_end
                    || self.parameter_end as usize > self.label.len())
        {
            return Err(Error::Invalid("LSP signature"));
        }
        put_u16(out, self.flags);
        put_u16(out, self.active_parameter);
        put_u32(out, self.parameter_start);
        put_u32(out, self.parameter_end);
        put_string_u16(out, &self.label)?;
        put_string_u32(out, &self.documentation)
    }
}

impl Decode for SignatureRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            flags: decoder.u16()?,
            active_parameter: decoder.u16()?,
            parameter_start: decoder.u32()?,
            parameter_end: decoder.u32()?,
            label: decoder.string_u16()?,
            documentation: decoder.string_u32()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionRecord {
    pub title: String,
    pub kind: String,
    pub flags: u16,
    pub edits: Vec<EditRecord>,
    pub disabled_reason: String,
}

impl Encode for ActionRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_name(
            &self.title,
            crate::schema::lsp::MAX_DETAIL_BYTES as usize,
            "LSP code action title",
        )?;
        if self.kind.len() > crate::schema::lsp::MAX_ACTION_KIND_BYTES as usize
            || self.flags & !(crate::schema::lsp::ACTION_FLAGS as u16) != 0
            || self.edits.len() > crate::schema::lsp::MAX_EDITS_PER_ACTION as usize
            || self.disabled_reason.len() > crate::schema::lsp::MAX_DETAIL_BYTES as usize
            || self.flags & crate::schema::lsp::ACTION_DISABLED as u16 != 0
                && self.disabled_reason.is_empty()
            || self.flags & crate::schema::lsp::ACTION_DISABLED as u16 == 0
                && !self.disabled_reason.is_empty()
        {
            return Err(Error::Invalid("LSP code action"));
        }
        put_string_u16(out, &self.title)?;
        put_string_u16(out, &self.kind)?;
        put_u16(out, self.flags);
        put_len_u16(out, self.edits.len())?;
        for edit in &self.edits {
            put_bytes_u32(out, &edit.encode()?)?;
        }
        put_string_u32(out, &self.disabled_reason)
    }
}

impl Decode for ActionRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let title = decoder.string_u16()?;
        let kind = decoder.string_u16()?;
        let flags = decoder.u16()?;
        let count = usize::from(decoder.u16()?);
        if count > crate::schema::lsp::MAX_EDITS_PER_ACTION as usize
            || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("LSP code action edit count"));
        }
        let mut edits = Vec::with_capacity(count);
        for _ in 0..count {
            edits.push(EditRecord::decode(decoder.len_bytes_u32()?)?);
        }
        let value = Self {
            title,
            kind,
            flags,
            edits,
            disabled_reason: decoder.string_u32()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryRecord {
    Location(LocationRecord),
    Hover(HoverRecord),
    Symbol(SymbolRecord),
    Completion(CompletionRecord),
    Action(ActionRecord),
    Edit(EditRecord),
    Signature(SignatureRecord),
}

impl QueryRecord {
    pub fn encode_typed(&self) -> Result<TypedRecord> {
        let (kind, body) = match self {
            Self::Location(value) => (crate::schema::lsp::RESULT_LOCATION, value.encode()?),
            Self::Hover(value) => (crate::schema::lsp::RESULT_HOVER, value.encode()?),
            Self::Symbol(value) => (crate::schema::lsp::RESULT_SYMBOL, value.encode()?),
            Self::Completion(value) => (crate::schema::lsp::RESULT_COMPLETION, value.encode()?),
            Self::Action(value) => (crate::schema::lsp::RESULT_ACTION, value.encode()?),
            Self::Edit(value) => (crate::schema::lsp::RESULT_EDIT, value.encode()?),
            Self::Signature(value) => (crate::schema::lsp::RESULT_SIGNATURE, value.encode()?),
        };
        Ok(TypedRecord {
            kind: kind as u16,
            required: false,
            body,
        })
    }

    pub fn decode_typed(value: &TypedRecord) -> Result<Option<Self>> {
        let decoded = match value.kind {
            kind if kind == crate::schema::lsp::RESULT_LOCATION as u16 => {
                Self::Location(LocationRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::lsp::RESULT_HOVER as u16 => {
                Self::Hover(HoverRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::lsp::RESULT_SYMBOL as u16 => {
                Self::Symbol(SymbolRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::lsp::RESULT_COMPLETION as u16 => {
                Self::Completion(CompletionRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::lsp::RESULT_ACTION as u16 => {
                Self::Action(ActionRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::lsp::RESULT_EDIT as u16 => {
                Self::Edit(EditRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::lsp::RESULT_SIGNATURE as u16 => {
                Self::Signature(SignatureRecord::decode(&value.body)?)
            }
            _ if !value.required => return Ok(None),
            _ => return Err(Error::Invalid("unknown required LSP query record")),
        };
        Ok(Some(decoded))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageDelivery {
    Inline(Vec<TypedRecord>),
    Transfer(Descriptor),
}

fn validate_query_descriptor(descriptor: &Descriptor) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Message
        || descriptor.direction != Direction::SENDER_TO_RECEIVER
        || descriptor.content_family != crate::family::LSP
        || descriptor.content_kind != crate::schema::lsp::QUERY_CONTENT_KIND as u16
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("LSP query Transfer descriptor"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryPage {
    pub query_status: u16,
    pub flags: u16,
    pub detail: String,
    pub next_cursor: Vec<u8>,
    pub total_hint: u64,
    pub delivery: PageDelivery,
    pub extensions: Extensions,
}

impl QueryPage {
    fn validate(&self) -> Result<()> {
        let status = crate::core::Status::from_code(self.query_status);
        if matches!(status, crate::core::Status::Unknown(_))
            || self.flags & !(crate::schema::lsp::PAGE_FLAGS as u16) != 0
            || self.detail.len() > crate::schema::lsp::MAX_DETAIL_BYTES as usize
            || status.is_ok() && !self.detail.is_empty()
            || !status.is_ok()
                && (self.flags & crate::schema::lsp::PAGE_INCOMPLETE as u16 == 0
                    || self.detail.is_empty())
            || (self.flags & crate::schema::lsp::PAGE_TRUNCATED as u16 != 0)
                != !self.next_cursor.is_empty()
            || self.next_cursor.len() > MAX_CURSOR_BYTES
        {
            return Err(Error::Invalid("LSP query cursor"));
        }
        match &self.delivery {
            PageDelivery::Inline(records) => {
                if records.len() > MAX_QUERY_RECORDS {
                    return Err(Error::Invalid("LSP query record count"));
                }
                let mut bytes = Vec::new();
                for record in records {
                    QueryRecord::decode_typed(record)?;
                    record.encode_into(&mut bytes)?;
                }
                if bytes.len() > MAX_QUERY_BYTES {
                    return Err(Error::Invalid("LSP query page bytes"));
                }
            }
            PageDelivery::Transfer(descriptor) => validate_query_descriptor(descriptor)?,
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for QueryPage {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.query_status);
        put_u16(out, self.flags);
        put_string_u32(out, &self.detail)?;
        put_bytes_u16(out, &self.next_cursor)?;
        put_u64(out, self.total_hint);
        match &self.delivery {
            PageDelivery::Inline(records) => {
                out.push(crate::schema::lsp::PAGE_INLINE as u8);
                out.extend_from_slice(&[0; 3]);
                put_len_u16(out, records.len())?;
                put_u16(out, 0);
                let mut bytes = Vec::new();
                for record in records {
                    record.encode_into(&mut bytes)?;
                }
                put_bytes_u32(out, &bytes)?;
            }
            PageDelivery::Transfer(descriptor) => {
                out.push(crate::schema::lsp::PAGE_TRANSFER as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, &descriptor.encode()?)?;
            }
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for QueryPage {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let query_status = decoder.u16()?;
        let flags = decoder.u16()?;
        let detail = decoder.string_u32()?;
        let next_cursor = decoder.len_bytes_u16()?.to_vec();
        let total_hint = decoder.u64()?;
        let delivery = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("LSP query page reserved bytes"));
        }
        let delivery = match delivery {
            value if value == crate::schema::lsp::PAGE_INLINE as u8 => {
                let count = usize::from(decoder.u16()?);
                if decoder.u16()? != 0 || count > MAX_QUERY_RECORDS {
                    return Err(Error::Invalid("LSP query record count"));
                }
                let bytes = decoder.len_bytes_u32()?;
                if bytes.len() > MAX_QUERY_BYTES || count > bytes.len() / 8 {
                    return Err(Error::Invalid("LSP query record stream"));
                }
                let mut records_decoder = Decoder::new(bytes);
                let mut records = Vec::with_capacity(count);
                for _ in 0..count {
                    let record = TypedRecord::decode_from(&mut records_decoder)?;
                    if QueryRecord::decode_typed(&record)?.is_some() {
                        records.push(record);
                    }
                }
                records_decoder.finish()?;
                PageDelivery::Inline(records)
            }
            value if value == crate::schema::lsp::PAGE_TRANSFER as u8 => {
                PageDelivery::Transfer(Descriptor::decode(decoder.len_bytes_u32()?)?)
            }
            _ => return Err(Error::Invalid("LSP query page delivery")),
        };
        let value = Self {
            query_status,
            flags,
            detail,
            next_cursor,
            total_hint,
            delivery,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerRecord {
    pub server_handle: u64,
    pub generation: u64,
    pub server_revision: u64,
    pub workspace_handle: u64,
    pub phase: u8,
    pub progress_pct: u8,
    pub epoch: u32,
    pub refused_edits: u32,
    pub rss_bytes: u64,
    pub capabilities: u64,
    pub language: String,
    pub profile: String,
    pub backend_id: String,
    pub last_message: String,
    pub extensions: Extensions,
}

impl ServerRecord {
    fn validate(&self) -> Result<()> {
        validate_handle(self.server_handle, "zero LSP server handle")?;
        validate_revision(self.generation, "zero LSP server generation")?;
        validate_revision(self.server_revision, "zero LSP server revision")?;
        if self.phase > crate::schema::lsp::SERVER_FAILED as u8
            || self.progress_pct > 100
                && self.progress_pct != crate::schema::lsp::SERVER_PROGRESS_UNKNOWN as u8
            || self.capabilities & !crate::schema::lsp::CAPABILITIES != 0
            || self.last_message.len() > crate::schema::lsp::MAX_DETAIL_BYTES as usize
        {
            return Err(Error::Invalid("LSP server metadata"));
        }
        validate_name(
            &self.language,
            crate::schema::lsp::MAX_LANGUAGE_BYTES as usize,
            "LSP server language",
        )?;
        validate_name(
            &self.profile,
            crate::schema::lsp::MAX_PROFILE_BYTES as usize,
            "LSP server profile",
        )?;
        validate_name(
            &self.backend_id,
            crate::schema::lsp::MAX_BACKEND_ID_BYTES as usize,
            "LSP backend ID",
        )?;
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for ServerRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.server_handle);
        put_u64(out, self.generation);
        put_u64(out, self.server_revision);
        put_u64(out, self.workspace_handle);
        out.push(self.phase);
        out.push(self.progress_pct);
        put_u16(out, 0);
        put_u32(out, self.epoch);
        put_u32(out, self.refused_edits);
        put_u32(out, 0);
        put_u64(out, self.rss_bytes);
        put_u64(out, self.capabilities);
        put_string_u16(out, &self.language)?;
        put_string_u16(out, &self.profile)?;
        put_string_u16(out, &self.backend_id)?;
        put_string_u32(out, &self.last_message)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for ServerRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let server_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let server_revision = decoder.u64()?;
        let workspace_handle = decoder.u64()?;
        let phase = decoder.u8()?;
        let progress_pct = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("LSP server reserved field"));
        }
        let epoch = decoder.u32()?;
        let refused_edits = decoder.u32()?;
        if decoder.u32()? != 0 {
            return Err(Error::Invalid("LSP server second reserved field"));
        }
        let rss_bytes = decoder.u64()?;
        let value = Self {
            server_handle,
            generation,
            server_revision,
            workspace_handle,
            phase,
            progress_pct,
            epoch,
            refused_edits,
            rss_bytes,
            capabilities: decoder.u64()?,
            language: decoder.string_u16()?,
            profile: decoder.string_u16()?,
            backend_id: decoder.string_u16()?,
            last_message: decoder.string_u32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerList {
    pub servers: Vec<ServerRecord>,
}

impl Encode for ServerList {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.servers.len() > crate::schema::lsp::MAX_SERVERS as usize {
            return Err(Error::Invalid("LSP server count"));
        }
        put_len_u16(out, self.servers.len())?;
        put_u16(out, 0);
        for server in &self.servers {
            put_bytes_u32(out, &server.encode()?)?;
        }
        Ok(())
    }
}

impl Decode for ServerList {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count > crate::schema::lsp::MAX_SERVERS as usize
            || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("LSP server count"));
        }
        let mut servers = Vec::with_capacity(count);
        for _ in 0..count {
            servers.push(ServerRecord::decode(decoder.len_bytes_u32()?)?);
        }
        decoder.finish()?;
        Ok(Self { servers })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub diagnostic_id: u64,
    pub severity: u8,
    pub tags: u16,
    pub range: TextRange,
    pub code: String,
    pub source: String,
    pub message: String,
}

impl Diagnostic {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.diagnostic_id == 0
            || self.severity > crate::schema::lsp::DIAGNOSTIC_HINT as u8
            || self.tags & !(crate::schema::lsp::DIAGNOSTIC_TAGS as u16) != 0
            || self.code.len() > crate::schema::lsp::MAX_DIAGNOSTIC_CODE_BYTES as usize
            || self.source.len() > crate::schema::lsp::MAX_LANGUAGE_BYTES as usize
            || self.message.len() > crate::schema::lsp::MAX_DIAGNOSTIC_MESSAGE_BYTES as usize
        {
            return Err(Error::Invalid("LSP diagnostic"));
        }
        put_u64(out, self.diagnostic_id);
        out.push(self.severity);
        out.push(0);
        put_u16(out, self.tags);
        self.range.encode_into(out)?;
        put_string_u16(out, &self.code)?;
        put_string_u16(out, &self.source)?;
        put_string_u32(out, &self.message)
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let diagnostic_id = decoder.u64()?;
        let severity = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("LSP diagnostic reserved byte"));
        }
        let value = Self {
            diagnostic_id,
            severity,
            tags: decoder.u16()?,
            range: TextRange::decode_from(decoder)?,
            code: decoder.string_u16()?,
            source: decoder.string_u16()?,
            message: decoder.string_u32()?,
        };
        let mut ignored = Vec::new();
        value.encode_into(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticRecord {
    pub path: FsPath,
    pub document_revision: u64,
    pub content_hash: [u8; 32],
    pub diagnostics_revision: u64,
    pub diagnostics: Vec<Diagnostic>,
    pub extensions: Extensions,
}

impl Encode for DiagnosticRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.path.components.is_empty()
            || self.diagnostics.len() > crate::schema::lsp::MAX_DIAGNOSTICS_PER_FILE as usize
        {
            return Err(Error::Invalid("LSP diagnostic record"));
        }
        validate_revision(self.diagnostics_revision, "zero LSP diagnostics revision")?;
        reject_unknown_required(&self.extensions, &[])?;
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u64(out, self.document_revision);
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.diagnostics_revision);
        put_len_u16(out, self.diagnostics.len())?;
        put_u16(out, 0);
        for diagnostic in &self.diagnostics {
            put_bytes_u32(out, &{
                let mut body = Vec::new();
                diagnostic.encode_into(&mut body)?;
                body
            })?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for DiagnosticRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let path = FsPath::decode(decoder.len_bytes_u32()?)?;
        let document_revision = decoder.u64()?;
        let content_hash = decoder.array_32()?;
        let diagnostics_revision = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count > crate::schema::lsp::MAX_DIAGNOSTICS_PER_FILE as usize
            || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("LSP diagnostic count"));
        }
        let mut diagnostics = Vec::with_capacity(count);
        for _ in 0..count {
            let mut body = Decoder::new(decoder.len_bytes_u32()?);
            diagnostics.push(Diagnostic::decode_from(&mut body)?);
            body.finish()?;
        }
        let value = Self {
            path,
            document_revision,
            content_hash,
            diagnostics_revision,
            diagnostics,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferRecord {
    pub workspace_handle: u64,
    pub buffer_handle: u64,
    pub buffer_revision: u64,
    pub path: FsPath,
    pub byte_len: u64,
    pub content_hash: [u8; 32],
    pub extensions: Extensions,
}

impl Encode for BufferRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.workspace_handle, "zero LSP workspace handle")?;
        validate_handle(self.buffer_handle, "zero LSP buffer handle")?;
        validate_revision(self.buffer_revision, "zero LSP buffer revision")?;
        if self.path.components.is_empty() || self.byte_len > crate::schema::lsp::MAX_BUFFER_BYTES {
            return Err(Error::Invalid("LSP buffer state"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.workspace_handle);
        put_u64(out, self.buffer_handle);
        put_u64(out, self.buffer_revision);
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u64(out, self.byte_len);
        out.extend_from_slice(&self.content_hash);
        self.extensions.encode_tail(out)
    }
}

impl Decode for BufferRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            workspace_handle: decoder.u64()?,
            buffer_handle: decoder.u64()?,
            buffer_revision: decoder.u64()?,
            path: FsPath::decode(decoder.len_bytes_u32()?)?,
            byte_len: decoder.u64()?,
            content_hash: decoder.array_32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StateEntity {
    Backend(ServerRecord),
    Diagnostics(DiagnosticRecord),
    Buffer(BufferRecord),
}

impl StateEntity {
    fn kind_and_body(&self) -> Result<(u16, Vec<u8>)> {
        match self {
            Self::Backend(value) => {
                validate_handle(
                    value.workspace_handle,
                    "zero LSP state server workspace handle",
                )?;
                Ok((crate::schema::lsp::ENTITY_BACKEND as u16, value.encode()?))
            }
            Self::Diagnostics(value) => Ok((
                crate::schema::lsp::ENTITY_DIAGNOSTICS as u16,
                value.encode()?,
            )),
            Self::Buffer(value) => Ok((crate::schema::lsp::ENTITY_BUFFER as u16, value.encode()?)),
        }
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("LSP complete state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }

    pub fn from_state_record(record: &Record) -> Result<Self> {
        if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("LSP complete state record kind"));
        }
        Self::decode(&record.body)
    }
}

impl Encode for StateEntity {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let (kind, body) = self.kind_and_body()?;
        put_u16(out, kind);
        put_u16(out, 0);
        put_bytes_u32(out, &body)
    }
}

impl Decode for StateEntity {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("LSP state entity reserved field"));
        }
        let body = decoder.len_bytes_u32()?;
        let value = match kind {
            kind if kind == crate::schema::lsp::ENTITY_BACKEND as u16 => {
                let server = ServerRecord::decode(body)?;
                validate_handle(
                    server.workspace_handle,
                    "zero LSP state server workspace handle",
                )?;
                Self::Backend(server)
            }
            kind if kind == crate::schema::lsp::ENTITY_DIAGNOSTICS as u16 => {
                Self::Diagnostics(DiagnosticRecord::decode(body)?)
            }
            kind if kind == crate::schema::lsp::ENTITY_BUFFER as u16 => {
                Self::Buffer(BufferRecord::decode(body)?)
            }
            _ => return Err(Error::Invalid("LSP state entity kind")),
        };
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityPatch {
    pub entity_kind: u16,
    pub observed_revision: u64,
    pub replacement: StateEntity,
    pub extensions: Extensions,
}

impl EntityPatch {
    fn validate(&self) -> Result<()> {
        let (replacement_kind, _) = self.replacement.kind_and_body()?;
        if self.entity_kind != replacement_kind || self.observed_revision == 0 {
            return Err(Error::Invalid("LSP state patch"));
        }
        reject_unknown_required(&self.extensions, &[])
    }

    pub fn state_record(&self) -> Result<Record> {
        self.validate()?;
        Ok(Record {
            kind: RecordKind::Patch,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for EntityPatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.entity_kind);
        put_u16(out, 0);
        put_u64(out, self.observed_revision);
        put_bytes_u32(out, &self.replacement.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for EntityPatch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let entity_kind = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("LSP state patch reserved field"));
        }
        let value = Self {
            entity_kind,
            observed_revision: decoder.u64()?,
            replacement: StateEntity::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RemovedEntityKey {
    Backend { server_handle: u64 },
    Diagnostics { path: FsPath },
    Buffer { buffer_handle: u64 },
}

impl RemovedEntityKey {
    pub fn entity_kind(&self) -> u16 {
        match self {
            Self::Backend { .. } => crate::schema::lsp::ENTITY_BACKEND as u16,
            Self::Diagnostics { .. } => crate::schema::lsp::ENTITY_DIAGNOSTICS as u16,
            Self::Buffer { .. } => crate::schema::lsp::ENTITY_BUFFER as u16,
        }
    }

    fn encode_body(&self) -> Result<Vec<u8>> {
        let mut body = Vec::new();
        match self {
            Self::Backend { server_handle } => {
                validate_handle(*server_handle, "zero removed LSP server handle")?;
                put_u64(&mut body, *server_handle);
            }
            Self::Diagnostics { path } => {
                if path.components.is_empty() {
                    return Err(Error::Invalid("root LSP diagnostics remove path"));
                }
                body = path.encode()?;
            }
            Self::Buffer { buffer_handle } => {
                validate_handle(*buffer_handle, "zero removed LSP buffer handle")?;
                put_u64(&mut body, *buffer_handle);
            }
        }
        if body.len() > crate::schema::lsp::MAX_ENTITY_KEY_BYTES as usize {
            return Err(Error::Invalid("LSP removed entity key length"));
        }
        Ok(body)
    }

    fn decode_body(entity_kind: u16, input: &[u8]) -> Result<Self> {
        match entity_kind {
            kind if kind == crate::schema::lsp::ENTITY_BACKEND as u16 => {
                let mut decoder = Decoder::new(input);
                let value = Self::Backend {
                    server_handle: decoder.u64()?,
                };
                decoder.finish()?;
                value.encode_body()?;
                Ok(value)
            }
            kind if kind == crate::schema::lsp::ENTITY_DIAGNOSTICS as u16 => {
                let value = Self::Diagnostics {
                    path: FsPath::decode(input)?,
                };
                value.encode_body()?;
                Ok(value)
            }
            kind if kind == crate::schema::lsp::ENTITY_BUFFER as u16 => {
                let mut decoder = Decoder::new(input);
                let value = Self::Buffer {
                    buffer_handle: decoder.u64()?,
                };
                decoder.finish()?;
                value.encode_body()?;
                Ok(value)
            }
            _ => Err(Error::Invalid("LSP removed entity kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedEntity {
    pub key: RemovedEntityKey,
    pub removed_revision: u64,
}

impl RemovedEntity {
    fn validate(&self) -> Result<()> {
        self.key.encode_body()?;
        validate_revision(self.removed_revision, "zero LSP removed revision")
    }

    pub fn state_record(&self) -> Result<Record> {
        self.validate()?;
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for RemovedEntity {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.key.entity_kind());
        put_u16(out, 0);
        put_bytes_u32(out, &self.key.encode_body()?)?;
        put_u64(out, self.removed_revision);
        Ok(())
    }
}

impl Decode for RemovedEntity {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let entity_kind = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("LSP remove reserved field"));
        }
        let key = RemovedEntityKey::decode_body(entity_kind, decoder.len_bytes_u32()?)?;
        let value = Self {
            key,
            removed_revision: decoder.u64()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Extension;

    fn round_trip<T>(value: T)
    where
        T: Encode + Decode + PartialEq + std::fmt::Debug,
    {
        let bytes = value.encode().unwrap();
        assert_eq!(T::decode(&bytes).unwrap(), value);
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
    }

    fn path() -> FsPath {
        FsPath {
            components: vec![b"src".to_vec(), b"main.rs".to_vec()],
        }
    }

    fn target() -> DocumentTarget {
        DocumentTarget {
            path: path(),
            document_revision: 3,
            content_hash: [9; 32],
        }
    }

    fn position() -> Position {
        Position {
            line: 2,
            byte_column: 4,
        }
    }

    fn range() -> TextRange {
        TextRange {
            start: position(),
            end: Position {
                line: 2,
                byte_column: 8,
            },
        }
    }

    fn runtime_server() -> ServerRecord {
        ServerRecord {
            server_handle: 1,
            generation: 1,
            server_revision: 2,
            workspace_handle: 3,
            phase: crate::schema::lsp::SERVER_READY as u8,
            progress_pct: 100,
            epoch: 4,
            refused_edits: 2,
            rss_bytes: 65_536,
            capabilities: crate::schema::lsp::CAPABILITIES,
            language: "rust".into(),
            profile: "default".into(),
            backend_id: "rust-analyzer".into(),
            last_message: "ready".into(),
            extensions: Extensions::default(),
        }
    }

    fn buffer_descriptor() -> Descriptor {
        let stage = UploadStage {
            staging_handle: 2,
            expires_server_ns: 1,
        };
        Descriptor {
            transfer_id: 3,
            mode: Mode::Byte,
            direction: Direction::RECEIVER_TO_SENDER,
            receiver_send_credit: 4096,
            sender_send_credit: 0,
            max_item_bytes: 0,
            max_chunk_bytes: 1024,
            content_family: crate::family::LSP,
            content_kind: crate::schema::lsp::BUFFER_CONTENT_KIND as u16,
            content_version: VERSION,
            extensions: Extensions(vec![
                Extension {
                    tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                    required: true,
                    value: Vec::new(),
                },
                stage.extension().unwrap(),
            ]),
        }
    }

    #[test]
    fn workspace_and_queries_round_trip() {
        round_trip(Open {
            source: WorkspaceSource::Fs {
                root_handle: 1,
                root_path: FsPath {
                    components: vec![b"repo".to_vec()],
                },
            },
            open_mode: crate::schema::lsp::OPEN_EXPLICIT as u8,
            diagnostics_settle_ms: 250,
            language: "rust".into(),
            profile: "default".into(),
            initialization_options: br#"{"cargo":{"allFeatures":true}}"#.to_vec(),
            extensions: Extensions::default(),
        });
        round_trip(Open {
            source: WorkspaceSource::TerminalCwd {
                terminal_handle: 7,
                suffix: FsPath {
                    components: vec![b"workspace".to_vec()],
                },
            },
            open_mode: crate::schema::lsp::OPEN_AUTO_DISCOVER as u8,
            diagnostics_settle_ms: 0,
            language: String::new(),
            profile: String::new(),
            initialization_options: Vec::new(),
            extensions: Extensions::default(),
        });
        round_trip(OpenResult {
            workspace_handle: 1,
            workspace_revision: 2,
            position_encoding: crate::schema::lsp::POSITION_UTF8 as u8,
            backend_count: 1,
            capabilities: crate::schema::lsp::CAPABILITIES,
            canonical_root: b"/workspace".to_vec(),
            extensions: Extensions::default(),
        });
        round_trip(OpenResult {
            workspace_handle: 1,
            workspace_revision: 2,
            position_encoding: crate::schema::lsp::POSITION_UTF8 as u8,
            backend_count: 0,
            capabilities: 0,
            canonical_root: b"/workspace".to_vec(),
            extensions: Extensions(vec![
                open_no_backend_detail_extension("no supported language found").unwrap(),
            ]),
        });
        round_trip(Closed {
            workspace_handle: 1,
            reason: crate::schema::lsp::CLOSED_ROOT_GONE as u8,
            detail: "root was removed".into(),
        });
        let queries = vec![
            QueryBody::Definition {
                target: target(),
                position: position(),
            },
            QueryBody::References {
                target: target(),
                position: position(),
                flags: crate::schema::lsp::REFERENCES_INCLUDE_DECLARATION as u16,
            },
            QueryBody::Hover {
                target: target(),
                position: position(),
            },
            QueryBody::DocumentSymbols { target: target() },
            QueryBody::WorkspaceSymbols {
                query: "main".into(),
            },
            QueryBody::WorkspaceSymbols {
                query: String::new(),
            },
            QueryBody::Completion {
                target: target(),
                position: position(),
                trigger_kind: crate::schema::lsp::COMPLETION_TRIGGER_CHARACTER as u8,
                trigger: ".".into(),
            },
            QueryBody::CodeActions {
                target: target(),
                range: range(),
                diagnostic_ids: vec![1, 2],
            },
            QueryBody::Formatting {
                target: target(),
                range: Some(range()),
                tab_width: 4,
                flags: crate::schema::lsp::FORMATTING_INSERT_SPACES as u16,
            },
            QueryBody::Rename {
                target: target(),
                position: position(),
                new_name: "renamed".into(),
            },
            QueryBody::SignatureHelp {
                target: target(),
                position: position(),
            },
        ];
        for body in queries {
            round_trip(body);
        }

        // Stable query-body vector: kind 4, zero flags, empty string_u16.
        let empty_workspace_symbols = QueryBody::WorkspaceSymbols {
            query: String::new(),
        };
        assert_eq!(
            empty_workspace_symbols.encode().unwrap(),
            [4, 0, 0, 0, 0, 0]
        );
        assert_eq!(
            QueryBody::decode(&[4, 0, 0, 0, 0, 0]).unwrap(),
            empty_workspace_symbols
        );
    }

    #[test]
    fn buffers_and_servers_round_trip() {
        round_trip(BufferPut {
            workspace_handle: 1,
            operation_id: [1; 16],
            expected_revision: 0,
            path: path(),
            content: b"fn main() {}".to_vec(),
            extensions: Extensions::default(),
        });
        assert_eq!(
            BufferPut {
                workspace_handle: 1,
                operation_id: [1; 16],
                expected_revision: 0,
                path: path(),
                content: vec![0xff],
                extensions: Extensions::default(),
            }
            .encode(),
            Err(Error::Invalid("LSP buffer content is not UTF-8"))
        );
        assert_eq!(validate_buffer_content(&[0xf0, 0x9f, 0xa6, 0x80]), Ok(()));
        round_trip(BufferBegin {
            workspace_handle: 1,
            expected_revision: 2,
            path: path(),
            byte_len: 65_536,
            content_hash: [2; 32],
            initial_send_credit: 4096,
            extensions: Extensions::default(),
        });
        let stage = BufferBeginResult {
            staging_handle: 2,
            descriptor: buffer_descriptor(),
            extensions: Extensions::default(),
        };
        round_trip(stage.clone());
        let reset = Reset {
            transfer_id: stage.descriptor.transfer_id,
            status: crate::schema::core::status::CANCELLED,
            detail: Vec::new(),
        };
        assert_eq!(
            stage.stage_discarded_by(&reset).unwrap(),
            stage.descriptor.upload_stage().unwrap()
        );
        round_trip(BufferCommit {
            staging_handle: 2,
            operation_id: [3; 16],
            extensions: Extensions::default(),
        });
        round_trip(BufferClose {
            buffer_handle: 4,
            expected_revision: 5,
            operation_id: [4; 16],
            extensions: Extensions::default(),
        });
        round_trip(ServerList {
            servers: vec![runtime_server()],
        });

        let mut foreign_server = runtime_server();
        foreign_server.workspace_handle = 0;
        round_trip(ServerList {
            servers: vec![foreign_server.clone()],
        });
        assert_eq!(
            StateEntity::Backend(foreign_server).encode(),
            Err(Error::Invalid("zero LSP state server workspace handle"))
        );
    }

    #[test]
    fn query_records_and_state_round_trip() {
        let values = [
            QueryRecord::Location(LocationRecord {
                path: path(),
                document_revision: 3,
                content_hash: [9; 32],
                range: range(),
                flags: crate::schema::lsp::LOCATION_DECLARATION as u16,
            }),
            QueryRecord::Hover(HoverRecord {
                target: LocationRecord {
                    path: path(),
                    document_revision: 3,
                    content_hash: [9; 32],
                    range: range(),
                    flags: 0,
                },
                markup_kind: crate::schema::lsp::MARKUP_MARKDOWN as u8,
                content: b"**type**".to_vec(),
            }),
            QueryRecord::Symbol(SymbolRecord {
                symbol_kind: crate::schema::lsp::SYMBOL_FUNCTION as u16,
                flags: 0,
                depth: 2,
                name: "main".into(),
                detail: "fn main()".into(),
                path: Some(path()),
                content_hash: Some([9; 32]),
                range: range(),
                selection_range: range(),
            }),
            QueryRecord::Completion(CompletionRecord {
                item_kind: crate::schema::lsp::COMPLETION_FUNCTION as u16,
                flags: crate::schema::lsp::COMPLETION_PRESELECT as u16,
                label: "main".into(),
                detail: "fn".into(),
                filter_text: "main".into(),
                insert_text: b"main()".to_vec(),
                replacement_range: Some(range()),
            }),
            QueryRecord::Action(ActionRecord {
                title: "rename".into(),
                kind: "refactor.rename".into(),
                flags: 0,
                edits: vec![EditRecord {
                    path: path(),
                    expected_revision: 3,
                    expected_content_hash: [9; 32],
                    range: range(),
                    replacement: b"renamed".to_vec(),
                }],
                disabled_reason: String::new(),
            }),
            QueryRecord::Signature(SignatureRecord {
                flags: crate::schema::lsp::SIGNATURE_ACTIVE as u16,
                active_parameter: 0,
                parameter_start: 3,
                parameter_end: 8,
                label: "fn main(value: u32)".into(),
                documentation: "Calls main.".into(),
            }),
        ];
        let records = values
            .iter()
            .map(|value| value.encode_typed().unwrap())
            .collect::<Vec<_>>();
        for record in &records {
            let message = record.encode_message().unwrap();
            assert_eq!(
                TypedRecord::decode_message(&message).unwrap(),
                record.clone()
            );
        }
        round_trip(QueryPage {
            query_status: crate::core::Status::Ok.code(),
            flags: 0,
            detail: String::new(),
            next_cursor: Vec::new(),
            total_hint: values.len() as u64,
            delivery: PageDelivery::Inline(records),
            extensions: Extensions::default(),
        });

        let backend = StateEntity::Backend(runtime_server());
        round_trip(backend.clone());
        assert_eq!(
            StateEntity::from_state_record(&backend.state_record(RecordKind::Replace).unwrap())
                .unwrap(),
            backend
        );
        round_trip(DiagnosticRecord {
            path: path(),
            document_revision: 3,
            content_hash: [9; 32],
            diagnostics_revision: 4,
            diagnostics: vec![Diagnostic {
                diagnostic_id: 1,
                severity: crate::schema::lsp::DIAGNOSTIC_WARNING as u8,
                tags: 0,
                range: range(),
                code: "unused".into(),
                source: "rustc".into(),
                message: "unused value".into(),
            }],
            extensions: Extensions::default(),
        });

        for removed in [
            RemovedEntity {
                key: RemovedEntityKey::Backend { server_handle: 7 },
                removed_revision: 8,
            },
            RemovedEntity {
                key: RemovedEntityKey::Diagnostics { path: path() },
                removed_revision: 9,
            },
            RemovedEntity {
                key: RemovedEntityKey::Buffer { buffer_handle: 10 },
                removed_revision: 11,
            },
        ] {
            round_trip(removed.clone());
            assert_eq!(
                RemovedEntity::decode(&removed.state_record().unwrap().body).unwrap(),
                removed
            );
        }
    }

    #[test]
    fn parity_metadata_invariants_are_enforced() {
        round_trip(DocumentTarget {
            path: path(),
            document_revision: 0,
            content_hash: [0; 32],
        });
        assert!(
            DocumentTarget {
                path: path(),
                document_revision: 1,
                content_hash: [0; 32],
            }
            .encode()
            .is_err()
        );

        let invalid_auto = Open {
            source: WorkspaceSource::PlatformPath(b"/workspace".to_vec()),
            open_mode: crate::schema::lsp::OPEN_AUTO_DISCOVER as u8,
            diagnostics_settle_ms: 0,
            language: "rust".into(),
            profile: String::new(),
            initialization_options: Vec::new(),
            extensions: Extensions::default(),
        };
        assert!(invalid_auto.encode().is_err());

        let missing_no_backend_detail = OpenResult {
            workspace_handle: 1,
            workspace_revision: 2,
            position_encoding: crate::schema::lsp::POSITION_UTF8 as u8,
            backend_count: 0,
            capabilities: 0,
            canonical_root: b"/workspace".to_vec(),
            extensions: Extensions::default(),
        };
        assert!(missing_no_backend_detail.encode().is_err());

        let mut invalid_server = runtime_server();
        invalid_server.progress_pct = 101;
        assert!(invalid_server.encode().is_err());

        let invalid_symbol = SymbolRecord {
            symbol_kind: crate::schema::lsp::SYMBOL_FUNCTION as u16,
            flags: 0,
            depth: 0,
            name: "main".into(),
            detail: String::new(),
            path: Some(path()),
            content_hash: None,
            range: range(),
            selection_range: range(),
        };
        assert!(invalid_symbol.encode().is_err());

        let invalid_signature = SignatureRecord {
            flags: 0,
            active_parameter: 0,
            parameter_start: 0,
            parameter_end: 99,
            label: "fn main()".into(),
            documentation: String::new(),
        };
        assert!(invalid_signature.encode().is_err());

        let mut invalid_page = QueryPage {
            query_status: crate::core::Status::Ok.code(),
            flags: crate::schema::lsp::PAGE_TRUNCATED as u16,
            detail: String::new(),
            next_cursor: Vec::new(),
            total_hint: 0,
            delivery: PageDelivery::Inline(Vec::new()),
            extensions: Extensions::default(),
        };
        assert!(invalid_page.encode().is_err());
        invalid_page.flags = crate::schema::lsp::PAGE_INCOMPLETE as u16;
        invalid_page.query_status = crate::core::Status::Unavailable.code();
        assert!(invalid_page.encode().is_err());
        invalid_page.detail = "backend is indexing".into();
        round_trip(invalid_page);

        assert!(
            Closed {
                workspace_handle: 1,
                reason: 255,
                detail: String::new(),
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);
        let mut invalid = Limits::HARD;
        invalid.max_buffer_bytes = 0;
        assert!(invalid.to_extensions().is_err());
        assert!(Limits::from_extensions(&Extensions::default()).is_err());
    }
}
