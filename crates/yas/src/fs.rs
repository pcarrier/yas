//! YAS filesystem family version 1 wire values.

use crate::prelude::*;

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, limit_u32, limit_u64,
    put_bytes_u16, put_bytes_u32, put_i64, put_len_u16, put_len_u32, put_string_u32, put_u16,
    put_u32, put_u64, read_limit_u32, read_limit_u64, reject_unknown_required_extensions,
};
use crate::state::{Record, RecordKind, Watch as StateWatch};
use crate::transfer::{
    Delivery, Descriptor, Direction, InlineOrTransfer, Mode, Reset, UploadStage,
};

pub const VERSION: u16 = crate::schema::fs::VERSION;
pub const MAX_PATH_COMPONENTS: usize = crate::schema::fs::MAX_PATH_COMPONENTS as usize;
pub const MAX_COMPONENT_BYTES: usize = crate::schema::fs::MAX_COMPONENT_BYTES as usize;
pub const MAX_PATH_BYTES: usize = crate::schema::fs::MAX_PATH_BYTES as usize;
pub const MAX_INLINE_BYTES: usize = crate::schema::fs::MAX_INLINE_BYTES as usize;
pub const MAX_QUERY_RECORDS: usize = crate::schema::fs::MAX_QUERY_RECORDS as usize;
pub const MAX_QUERY_BYTES: usize = crate::schema::fs::MAX_QUERY_BYTES as usize;
pub const MAX_BATCH_ITEMS: usize = crate::schema::fs::MAX_BATCH_ITEMS as usize;

pub mod request_kind {
    pub use crate::schema::fs::request::*;
}

pub mod event_kind {
    pub use crate::schema::fs::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_roots_per_session: u32,
    pub max_watches_per_root: u32,
    pub max_path_components: u32,
    pub max_component_bytes: u32,
    pub max_path_bytes: u32,
    pub max_inline_bytes: u32,
    pub max_query_records: u32,
    pub max_query_bytes: u32,
    pub max_stages_per_session: u32,
    pub max_staged_bytes: u64,
    pub max_batch_items: u32,
    pub max_query_concurrency: u32,
    pub max_catalog_entries: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_roots_per_session: crate::schema::fs::MAX_ROOTS_PER_SESSION as u32,
        max_watches_per_root: crate::schema::fs::MAX_WATCHES_PER_ROOT as u32,
        max_path_components: crate::schema::fs::MAX_PATH_COMPONENTS as u32,
        max_component_bytes: crate::schema::fs::MAX_COMPONENT_BYTES as u32,
        max_path_bytes: crate::schema::fs::MAX_PATH_BYTES as u32,
        max_inline_bytes: crate::schema::fs::MAX_INLINE_BYTES as u32,
        max_query_records: crate::schema::fs::MAX_QUERY_RECORDS as u32,
        max_query_bytes: crate::schema::fs::MAX_QUERY_BYTES as u32,
        max_stages_per_session: crate::schema::fs::MAX_STAGES_PER_SESSION as u32,
        max_staged_bytes: crate::schema::fs::MAX_STAGED_BYTES,
        max_batch_items: crate::schema::fs::MAX_BATCH_ITEMS as u32,
        max_query_concurrency: crate::schema::fs::MAX_QUERY_CONCURRENCY as u32,
        max_catalog_entries: crate::schema::fs::MAX_CATALOG_ENTRIES as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let u32_values = [
            (self.max_roots_per_session, hard.max_roots_per_session),
            (self.max_watches_per_root, hard.max_watches_per_root),
            (self.max_path_components, hard.max_path_components),
            (self.max_component_bytes, hard.max_component_bytes),
            (self.max_path_bytes, hard.max_path_bytes),
            (self.max_inline_bytes, hard.max_inline_bytes),
            (self.max_query_records, hard.max_query_records),
            (self.max_query_bytes, hard.max_query_bytes),
            (self.max_stages_per_session, hard.max_stages_per_session),
            (self.max_batch_items, hard.max_batch_items),
            (self.max_query_concurrency, hard.max_query_concurrency),
            (self.max_catalog_entries, hard.max_catalog_entries),
        ];
        if u32_values
            .into_iter()
            .any(|(value, maximum)| value == 0 || value > maximum)
            || self.max_staged_bytes == 0
            || self.max_staged_bytes > hard.max_staged_bytes
        {
            return Err(Error::Invalid("FS family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::fs::LIMIT_MAX_ROOTS_PER_SESSION,
                self.max_roots_per_session,
            ),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_WATCHES_PER_ROOT,
                self.max_watches_per_root,
            ),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_PATH_COMPONENTS,
                self.max_path_components,
            ),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_COMPONENT_BYTES,
                self.max_component_bytes,
            ),
            limit_u32(crate::schema::fs::LIMIT_MAX_PATH_BYTES, self.max_path_bytes),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_INLINE_BYTES,
                self.max_inline_bytes,
            ),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_QUERY_RECORDS,
                self.max_query_records,
            ),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_QUERY_BYTES,
                self.max_query_bytes,
            ),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_STAGES_PER_SESSION,
                self.max_stages_per_session,
            ),
            limit_u64(
                crate::schema::fs::LIMIT_MAX_STAGED_BYTES,
                self.max_staged_bytes,
            ),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_BATCH_ITEMS,
                self.max_batch_items,
            ),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_QUERY_CONCURRENCY,
                self.max_query_concurrency,
            ),
            limit_u32(
                crate::schema::fs::LIMIT_MAX_CATALOG_ENTRIES,
                self.max_catalog_entries,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required_extensions(
            extensions,
            &[
                crate::schema::fs::LIMIT_MAX_ROOTS_PER_SESSION as u16,
                crate::schema::fs::LIMIT_MAX_WATCHES_PER_ROOT as u16,
                crate::schema::fs::LIMIT_MAX_PATH_COMPONENTS as u16,
                crate::schema::fs::LIMIT_MAX_COMPONENT_BYTES as u16,
                crate::schema::fs::LIMIT_MAX_PATH_BYTES as u16,
                crate::schema::fs::LIMIT_MAX_INLINE_BYTES as u16,
                crate::schema::fs::LIMIT_MAX_QUERY_RECORDS as u16,
                crate::schema::fs::LIMIT_MAX_QUERY_BYTES as u16,
                crate::schema::fs::LIMIT_MAX_STAGES_PER_SESSION as u16,
                crate::schema::fs::LIMIT_MAX_STAGED_BYTES as u16,
                crate::schema::fs::LIMIT_MAX_BATCH_ITEMS as u16,
                crate::schema::fs::LIMIT_MAX_QUERY_CONCURRENCY as u16,
                crate::schema::fs::LIMIT_MAX_CATALOG_ENTRIES as u16,
            ],
            "unknown required FS family limit",
        )?;
        let value = Self {
            max_roots_per_session: read_limit_u32(
                extensions,
                crate::schema::fs::LIMIT_MAX_ROOTS_PER_SESSION,
            )?,
            max_watches_per_root: read_limit_u32(
                extensions,
                crate::schema::fs::LIMIT_MAX_WATCHES_PER_ROOT,
            )?,
            max_path_components: read_limit_u32(
                extensions,
                crate::schema::fs::LIMIT_MAX_PATH_COMPONENTS,
            )?,
            max_component_bytes: read_limit_u32(
                extensions,
                crate::schema::fs::LIMIT_MAX_COMPONENT_BYTES,
            )?,
            max_path_bytes: read_limit_u32(extensions, crate::schema::fs::LIMIT_MAX_PATH_BYTES)?,
            max_inline_bytes: read_limit_u32(
                extensions,
                crate::schema::fs::LIMIT_MAX_INLINE_BYTES,
            )?,
            max_query_records: read_limit_u32(
                extensions,
                crate::schema::fs::LIMIT_MAX_QUERY_RECORDS,
            )?,
            max_query_bytes: read_limit_u32(extensions, crate::schema::fs::LIMIT_MAX_QUERY_BYTES)?,
            max_stages_per_session: read_limit_u32(
                extensions,
                crate::schema::fs::LIMIT_MAX_STAGES_PER_SESSION,
            )?,
            max_staged_bytes: read_limit_u64(
                extensions,
                crate::schema::fs::LIMIT_MAX_STAGED_BYTES,
            )?,
            max_batch_items: read_limit_u32(extensions, crate::schema::fs::LIMIT_MAX_BATCH_ITEMS)?,
            max_query_concurrency: read_limit_u32(
                extensions,
                crate::schema::fs::LIMIT_MAX_QUERY_CONCURRENCY,
            )?,
            max_catalog_entries: read_limit_u32(
                extensions,
                crate::schema::fs::LIMIT_MAX_CATALOG_ENTRIES,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn handle(value: u64, what: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(what))
    } else {
        Ok(())
    }
}

fn revision(value: u64, what: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(what))
    } else {
        Ok(())
    }
}

fn operation_id(value: &[u8; 16]) -> Result<()> {
    if value.iter().all(|byte| *byte == 0) {
        Err(Error::Invalid("zero FS operation ID"))
    } else {
        Ok(())
    }
}

fn limit(name: &'static str, actual: u64, maximum: u64) -> Error {
    Error::LimitExceeded {
        limit: name,
        actual,
        maximum,
    }
}

fn reject_unknown_required(extensions: &Extensions, known: &[u16]) -> Result<()> {
    extensions.validate()?;
    if extensions
        .0
        .iter()
        .any(|extension| extension.required && !known.contains(&extension.tag))
    {
        return Err(Error::Invalid("unknown required FS extension"));
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Path {
    pub components: Vec<Vec<u8>>,
}

impl Path {
    fn validate(&self) -> Result<()> {
        if self.components.len() > MAX_PATH_COMPONENTS {
            return Err(limit(
                "FS path components",
                self.components.len() as u64,
                MAX_PATH_COMPONENTS as u64,
            ));
        }
        let mut total = 0usize;
        for component in &self.components {
            if component.is_empty()
                || component.len() > MAX_COMPONENT_BYTES
                || component == b"."
                || component == b".."
                || component.contains(&0)
                || component.contains(&b'/')
                || component.contains(&b'\\')
            {
                return Err(Error::Invalid("FS path component"));
            }
            total = total
                .checked_add(component.len())
                .ok_or(Error::LengthOverflow)?;
            if total > MAX_PATH_BYTES {
                return Err(limit("FS path bytes", total as u64, MAX_PATH_BYTES as u64));
            }
        }
        Ok(())
    }

    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_len_u16(out, self.components.len())?;
        for component in &self.components {
            put_bytes_u16(out, component)?;
        }
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let count = usize::from(decoder.u16()?);
        if count > MAX_PATH_COMPONENTS || count > decoder.remaining() / 2 {
            return Err(Error::Invalid("FS path component count"));
        }
        let mut components = Vec::with_capacity(count);
        for _ in 0..count {
            components.push(decoder.len_bytes_u16()?.to_vec());
        }
        let value = Self { components };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for Path {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_into(out)
    }
}

impl Decode for Path {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootSource {
    PlatformPath(Vec<u8>),
    TerminalCwd { terminal_handle: u64, suffix: Path },
    ProcessCwd(u64),
    Staging,
}

impl RootSource {
    fn validate(&self) -> Result<()> {
        match self {
            Self::PlatformPath(path)
                if !path.is_empty() && path.len() <= MAX_PATH_BYTES && !path.contains(&0) =>
            {
                Ok(())
            }
            Self::TerminalCwd {
                terminal_handle,
                suffix,
            } => {
                handle(*terminal_handle, "zero FS terminal handle")?;
                suffix.validate()
            }
            Self::ProcessCwd(value) => handle(*value, "zero FS process handle"),
            Self::Staging => Ok(()),
            Self::PlatformPath(_) => Err(Error::Invalid("FS platform path")),
        }
    }
}

impl Encode for RootSource {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        match self {
            Self::PlatformPath(path) => {
                out.push(crate::schema::fs::SOURCE_PLATFORM_PATH as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, path)?;
            }
            Self::TerminalCwd {
                terminal_handle,
                suffix,
            } => {
                out.push(crate::schema::fs::SOURCE_TERMINAL_CWD as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *terminal_handle);
                put_bytes_u32(out, &suffix.encode()?)?;
            }
            Self::ProcessCwd(value) => {
                out.push(crate::schema::fs::SOURCE_PROCESS_CWD as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *value);
            }
            Self::Staging => {
                out.push(crate::schema::fs::SOURCE_STAGING as u8);
                out.extend_from_slice(&[0; 3]);
            }
        }
        Ok(())
    }
}

impl Decode for RootSource {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("FS root source reserved bytes"));
        }
        let value = match kind {
            value if value == crate::schema::fs::SOURCE_PLATFORM_PATH as u8 => {
                Self::PlatformPath(decoder.len_bytes_u32()?.to_vec())
            }
            value if value == crate::schema::fs::SOURCE_TERMINAL_CWD as u8 => Self::TerminalCwd {
                terminal_handle: decoder.u64()?,
                suffix: Path::decode(decoder.len_bytes_u32()?)?,
            },
            value if value == crate::schema::fs::SOURCE_PROCESS_CWD as u8 => {
                Self::ProcessCwd(decoder.u64()?)
            }
            value if value == crate::schema::fs::SOURCE_STAGING as u8 => Self::Staging,
            _ => return Err(Error::Invalid("FS root source kind")),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub flags: u16,
    pub source: RootSource,
    pub extensions: Extensions,
}

impl Encode for Open {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.flags & !(crate::schema::fs::OPEN_FLAGS as u16) != 0 {
            return Err(Error::Invalid("FS OPEN flags"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_bytes_u32(out, &self.source.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Open {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS OPEN reserved field"));
        }
        let value = Self {
            flags,
            source: RootSource::decode(decoder.len_bytes_u32()?)?,
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
    pub root_handle: u64,
    pub root_revision: u64,
    pub path_model: u8,
    pub case_behavior: u8,
    pub canonical_path: Vec<u8>,
    pub extensions: Extensions,
}

impl Encode for OpenResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.root_handle, "zero FS root handle")?;
        revision(self.root_revision, "zero FS root revision")?;
        if self.path_model > crate::schema::fs::PATH_WINDOWS_UTF8 as u8
            || self.case_behavior > crate::schema::fs::CASE_PRESERVING_INSENSITIVE as u8
            || self.canonical_path.is_empty()
            || self.canonical_path.len() > MAX_PATH_BYTES
            || self.canonical_path.contains(&0)
        {
            return Err(Error::Invalid("FS root path metadata"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.root_handle);
        put_u64(out, self.root_revision);
        out.push(self.path_model);
        out.push(self.case_behavior);
        put_u16(out, 0);
        put_bytes_u32(out, &self.canonical_path)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for OpenResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let root_handle = decoder.u64()?;
        let root_revision = decoder.u64()?;
        let path_model = decoder.u8()?;
        let case_behavior = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS OPEN result reserved field"));
        }
        let value = Self {
            root_handle,
            root_revision,
            path_model,
            case_behavior,
            canonical_path: decoder.len_bytes_u32()?.to_vec(),
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
    pub root_handle: u64,
    pub extensions: Extensions,
}

impl Encode for Close {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.root_handle, "zero FS root handle")?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.root_handle);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Close {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            root_handle: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    pub root_handle: u64,
    pub flags: u16,
    pub settle_ms: u16,
    pub inline_max: u32,
    pub ignore_patterns: String,
    pub state: StateWatch,
}

impl Encode for Watch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.root_handle, "zero FS root handle")?;
        if self.flags & !(crate::schema::fs::WATCH_FLAGS as u16) != 0
            || self.inline_max as usize > MAX_INLINE_BYTES
            || u64::from(self.settle_ms) > crate::schema::fs::MAX_WATCH_SETTLE_MS
            || self.ignore_patterns.len() > crate::schema::fs::MAX_IGNORE_PATTERN_BYTES as usize
            || self.ignore_patterns.as_bytes().contains(&0)
        {
            return Err(Error::Invalid("FS WATCH policy or inline limit"));
        }
        put_u64(out, self.root_handle);
        put_u16(out, self.flags);
        put_u16(out, self.settle_ms);
        put_u32(out, self.inline_max);
        put_string_u32(out, &self.ignore_patterns)?;
        put_bytes_u32(out, &self.state.encode()?)
    }
}

impl Decode for Watch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let root_handle = decoder.u64()?;
        let flags = decoder.u16()?;
        let settle_ms = decoder.u16()?;
        let value = Self {
            root_handle,
            flags,
            settle_ms,
            inline_max: decoder.u32()?,
            ignore_patterns: decoder.string_u32()?,
            state: StateWatch::decode(decoder.len_bytes_u32()?)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryBody {
    File {
        byte_len: u64,
        content_hash: [u8; 32],
        inline_content: Option<Vec<u8>>,
    },
    Directory,
    Symlink {
        content_hash: [u8; 32],
        target: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryRecord {
    pub path: Path,
    pub entry_revision: u64,
    pub flags: u8,
    pub mode: u32,
    pub modified_unix_ns: i64,
    pub body: EntryBody,
    pub extensions: Extensions,
}

impl EntryRecord {
    fn validate(&self) -> Result<()> {
        self.path.validate()?;
        revision(self.entry_revision, "zero FS entry revision")?;
        if self.flags & !(crate::schema::fs::ENTRY_FLAGS as u8) != 0 {
            return Err(Error::Invalid("FS entry flags"));
        }
        let unreadable = self.flags & crate::schema::fs::ENTRY_UNREADABLE as u8 != 0;
        let unstable = self.flags & crate::schema::fs::ENTRY_UNSTABLE as u8 != 0;
        let symlink_directory = self.flags & crate::schema::fs::ENTRY_SYMLINK_DIRECTORY as u8 != 0;
        let directory_filtered =
            self.flags & crate::schema::fs::ENTRY_DIRECTORY_FILTERED as u8 != 0;
        match &self.body {
            EntryBody::File {
                byte_len,
                inline_content,
                ..
            } => {
                if let Some(content) = inline_content
                    && (content.len() > MAX_INLINE_BYTES || content.len() as u64 != *byte_len)
                {
                    return Err(Error::Invalid("FS inline file content"));
                }
                if (unreadable || unstable) && inline_content.is_some() {
                    return Err(Error::Invalid("FS unavailable file content"));
                }
                if symlink_directory || directory_filtered {
                    return Err(Error::Invalid("FS file-only entry flags"));
                }
            }
            EntryBody::Directory => {
                if unreadable || unstable || symlink_directory {
                    return Err(Error::Invalid("FS directory entry flags"));
                }
            }
            EntryBody::Symlink { target, .. } => {
                if target.is_empty() || target.len() > MAX_PATH_BYTES || target.contains(&0) {
                    return Err(Error::Invalid("FS symlink target"));
                }
                if unstable || (directory_filtered && !symlink_directory) {
                    return Err(Error::Invalid("FS symlink entry flags"));
                }
            }
        }
        reject_unknown_required(
            &self.extensions,
            &[crate::schema::fs::ENTRY_OPERATION_ID_EXTENSION as u16],
        )?;
        if let Some(extension) = self.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::fs::ENTRY_OPERATION_ID_EXTENSION as u16
        }) && (extension.value.len() != 16 || extension.value.iter().all(|byte| *byte == 0))
        {
            return Err(Error::Invalid("FS entry operation ID extension"));
        }
        Ok(())
    }

    pub fn operation_id(&self) -> Result<Option<[u8; 16]>> {
        let Some(extension) = self.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::fs::ENTRY_OPERATION_ID_EXTENSION as u16
        }) else {
            return Ok(None);
        };
        let value: [u8; 16] = extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("FS entry operation ID extension"))?;
        operation_id(&value)?;
        Ok(Some(value))
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("FS complete state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for EntryRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u64(out, self.entry_revision);
        let kind = match &self.body {
            EntryBody::File { .. } => crate::schema::fs::ENTRY_FILE as u8,
            EntryBody::Directory => crate::schema::fs::ENTRY_DIRECTORY as u8,
            EntryBody::Symlink { .. } => crate::schema::fs::ENTRY_SYMLINK as u8,
        };
        out.push(kind);
        out.push(self.flags);
        put_u16(out, 0);
        put_u32(out, self.mode);
        put_i64(out, self.modified_unix_ns);
        match &self.body {
            EntryBody::File {
                byte_len,
                content_hash,
                inline_content,
            } => {
                put_u64(out, *byte_len);
                out.extend_from_slice(content_hash);
                match inline_content {
                    None => {
                        out.push(crate::schema::fs::CONTENT_NONE as u8);
                        out.extend_from_slice(&[0; 3]);
                    }
                    Some(content) => {
                        out.push(crate::schema::fs::CONTENT_INLINE as u8);
                        out.extend_from_slice(&[0; 3]);
                        put_bytes_u32(out, content)?;
                    }
                }
            }
            EntryBody::Directory => {}
            EntryBody::Symlink {
                content_hash,
                target,
            } => {
                out.extend_from_slice(content_hash);
                put_bytes_u32(out, target)?;
            }
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for EntryRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let path = Path::decode(decoder.len_bytes_u32()?)?;
        let entry_revision = decoder.u64()?;
        let kind = decoder.u8()?;
        let flags = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS entry reserved field"));
        }
        let mode = decoder.u32()?;
        let modified_unix_ns = decoder.i64()?;
        let body = match kind {
            value if value == crate::schema::fs::ENTRY_FILE as u8 => {
                let byte_len = decoder.u64()?;
                let content_hash = decoder.array_32()?;
                let delivery = decoder.u8()?;
                if decoder.take(3)? != [0; 3] {
                    return Err(Error::Invalid("FS entry content reserved bytes"));
                }
                let inline_content = match delivery {
                    value if value == crate::schema::fs::CONTENT_NONE as u8 => None,
                    value if value == crate::schema::fs::CONTENT_INLINE as u8 => {
                        Some(decoder.len_bytes_u32()?.to_vec())
                    }
                    _ => return Err(Error::Invalid("FS entry content kind")),
                };
                EntryBody::File {
                    byte_len,
                    content_hash,
                    inline_content,
                }
            }
            value if value == crate::schema::fs::ENTRY_DIRECTORY as u8 => EntryBody::Directory,
            value if value == crate::schema::fs::ENTRY_SYMLINK as u8 => EntryBody::Symlink {
                content_hash: decoder.array_32()?,
                target: decoder.len_bytes_u32()?.to_vec(),
            },
            _ => return Err(Error::Invalid("FS entry kind")),
        };
        let value = Self {
            path,
            entry_revision,
            flags,
            mode,
            modified_unix_ns,
            body,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

pub fn operation_id_extension(value: [u8; 16]) -> Result<Extension> {
    operation_id(&value)?;
    Ok(Extension {
        tag: crate::schema::fs::ENTRY_OPERATION_ID_EXTENSION as u16,
        required: false,
        value: value.to_vec(),
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntryPatch {
    pub path: Path,
    pub observed_revision: u64,
    pub fields: u16,
    pub replacement: EntryRecord,
}

impl Encode for EntryPatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.path.validate()?;
        revision(self.observed_revision, "zero FS observed entry revision")?;
        if self.fields == 0 || self.fields & !(crate::schema::fs::PATCH_FIELDS as u16) != 0 {
            return Err(Error::Invalid("FS PATCH fields"));
        }
        if self.path != self.replacement.path {
            return Err(Error::Invalid("FS PATCH replacement path"));
        }
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u64(out, self.observed_revision);
        put_u16(out, self.fields);
        put_u16(out, 0);
        put_bytes_u32(out, &self.replacement.encode()?)
    }
}

impl Decode for EntryPatch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let path = Path::decode(decoder.len_bytes_u32()?)?;
        let observed_revision = decoder.u64()?;
        let fields = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS PATCH reserved field"));
        }
        let value = Self {
            path,
            observed_revision,
            fields,
            replacement: EntryRecord::decode(decoder.len_bytes_u32()?)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveRecord {
    pub from: Path,
    pub to: Path,
    pub operation_id: Option<[u8; 16]>,
}

impl Encode for MoveRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.from.validate()?;
        self.to.validate()?;
        if self.from == self.to {
            return Err(Error::Invalid("FS MOVE identical paths"));
        }
        put_bytes_u32(out, &self.from.encode()?)?;
        put_bytes_u32(out, &self.to.encode()?)?;
        out.push(u8::from(self.operation_id.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(id) = self.operation_id {
            operation_id(&id)?;
            out.extend_from_slice(&id);
        }
        Ok(())
    }
}

impl Decode for MoveRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let from = Path::decode(decoder.len_bytes_u32()?)?;
        let to = Path::decode(decoder.len_bytes_u32()?)?;
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("FS MOVE operation presence"));
        }
        let value = Self {
            from,
            to,
            operation_id: if present != 0 {
                Some(decoder.array_16()?)
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
pub struct RemoveRecord {
    pub path: Path,
    pub removed_revision: u64,
    pub operation_id: Option<[u8; 16]>,
}

impl Encode for RemoveRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.path.validate()?;
        revision(self.removed_revision, "zero FS removed revision")?;
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u64(out, self.removed_revision);
        out.push(u8::from(self.operation_id.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(id) = self.operation_id {
            operation_id(&id)?;
            out.extend_from_slice(&id);
        }
        Ok(())
    }
}

impl Decode for RemoveRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let path = Path::decode(decoder.len_bytes_u32()?)?;
        let removed_revision = decoder.u64()?;
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("FS REMOVE operation presence"));
        }
        let value = Self {
            path,
            removed_revision,
            operation_id: if present != 0 {
                Some(decoder.array_16()?)
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
pub enum StateMutation {
    Complete(EntryRecord),
    Patch(EntryPatch),
    Remove(RemoveRecord),
    Move(MoveRecord),
}

impl StateMutation {
    pub fn state_record(&self, complete_kind: RecordKind) -> Result<Record> {
        let (kind, body) = match self {
            Self::Complete(value) => {
                if !matches!(complete_kind, RecordKind::Add | RecordKind::Replace) {
                    return Err(Error::Invalid("FS complete state record kind"));
                }
                (complete_kind, value.encode()?)
            }
            Self::Patch(value) => (RecordKind::Patch, value.encode()?),
            Self::Remove(value) => (RecordKind::Remove, value.encode()?),
            Self::Move(value) => (
                RecordKind::family(crate::schema::fs::RECORD_MOVE as u16)?,
                value.encode()?,
            ),
        };
        Ok(Record {
            kind,
            required: false,
            body,
        })
    }

    pub fn decode_record(record: &Record) -> Result<Self> {
        match record.kind {
            RecordKind::Add | RecordKind::Replace => {
                Ok(Self::Complete(EntryRecord::decode(&record.body)?))
            }
            RecordKind::Patch => Ok(Self::Patch(EntryPatch::decode(&record.body)?)),
            RecordKind::Remove => Ok(Self::Remove(RemoveRecord::decode(&record.body)?)),
            RecordKind::Family(kind) if kind == crate::schema::fs::RECORD_MOVE as u16 => {
                Ok(Self::Move(MoveRecord::decode(&record.body)?))
            }
            _ => Err(Error::Invalid("FS state record kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetch {
    pub root_handle: u64,
    pub path: Path,
    pub expected_hash: Option<[u8; 32]>,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Fetch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.root_handle, "zero FS root handle")?;
        self.path.validate()?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.root_handle);
        put_bytes_u32(out, &self.path.encode()?)?;
        out.push(u8::from(self.expected_hash.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(hash) = self.expected_hash {
            out.extend_from_slice(&hash);
        }
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Fetch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let root_handle = decoder.u64()?;
        let path = Path::decode(decoder.len_bytes_u32()?)?;
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("FS FETCH hash presence"));
        }
        let value = Self {
            root_handle,
            path,
            expected_hash: if present != 0 {
                Some(decoder.array_32()?)
            } else {
                None
            },
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

fn validate_transfer(
    descriptor: &Descriptor,
    content_kind: u16,
    mode: Mode,
    direction: Direction,
) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != mode
        || descriptor.direction != direction
        || descriptor.content_family != crate::family::FS
        || descriptor.content_kind != content_kind
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("FS Transfer descriptor"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentResult {
    pub content: InlineOrTransfer,
    pub extensions: Extensions,
}

impl ContentResult {
    fn validate(&self) -> Result<()> {
        match &self.content.delivery {
            Delivery::Inline(bytes) if bytes.len() <= MAX_INLINE_BYTES => {}
            Delivery::Inline(bytes) => {
                return Err(limit(
                    "FS inline bytes",
                    bytes.len() as u64,
                    MAX_INLINE_BYTES as u64,
                ));
            }
            Delivery::Transfer(descriptor) => validate_transfer(
                descriptor,
                crate::schema::fs::FILE_CONTENT_KIND as u16,
                Mode::Byte,
                Direction::SENDER_TO_RECEIVER,
            )?,
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for ContentResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_bytes_u32(out, &self.content.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for ContentResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            content: InlineOrTransfer::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadQuestion {
    pub kind: u16,
    pub flags: u16,
    pub path: Path,
}

impl ReadQuestion {
    fn validate(&self) -> Result<()> {
        if self.kind > crate::schema::fs::READ_CONTENT as u16
            || self.flags & !(crate::schema::fs::READ_FLAGS as u16) != 0
        {
            return Err(Error::Invalid("FS READ question"));
        }
        self.path.validate()
    }

    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.kind);
        put_u16(out, self.flags);
        put_bytes_u32(out, &self.path.encode()?)
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            kind: decoder.u16()?,
            flags: decoder.u16()?,
            path: Path::decode(decoder.len_bytes_u32()?)?,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub root_handle: u64,
    pub initial_receive_credit: u64,
    pub questions: Vec<ReadQuestion>,
    pub extensions: Extensions,
}

impl Encode for Read {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.root_handle, "zero FS root handle")?;
        if self.questions.is_empty() || self.questions.len() > MAX_QUERY_RECORDS {
            return Err(Error::Invalid("FS READ question count"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.root_handle);
        put_u64(out, self.initial_receive_credit);
        put_len_u16(out, self.questions.len())?;
        put_u16(out, 0);
        for question in &self.questions {
            question.encode_into(out)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Read {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let root_handle = decoder.u64()?;
        let initial_receive_credit = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count == 0
            || count > MAX_QUERY_RECORDS
            || count > decoder.remaining() / 8
        {
            return Err(Error::Invalid("FS READ question count"));
        }
        let mut questions = Vec::with_capacity(count);
        for _ in 0..count {
            questions.push(ReadQuestion::decode_from(&mut decoder)?);
        }
        let value = Self {
            root_handle,
            initial_receive_credit,
            questions,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub root_handle: u64,
    pub flags: u16,
    pub max_results: u16,
    pub query: Vec<u8>,
    pub cursor: Vec<u8>,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

fn validate_query_request(
    root_handle: u64,
    flags: u16,
    known_flags: u16,
    query: &[u8],
    cursor: &[u8],
    extensions: &Extensions,
) -> Result<()> {
    handle(root_handle, "zero FS root handle")?;
    if flags & !known_flags != 0
        || query.is_empty()
        || query.len() > crate::schema::fs::MAX_QUERY_TEXT_BYTES as usize
        || cursor.len() > crate::schema::fs::MAX_CURSOR_BYTES as usize
        || query.contains(&0)
    {
        return Err(Error::Invalid("FS query request"));
    }
    reject_unknown_required(extensions, &[])
}

impl Encode for Search {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_request(
            self.root_handle,
            self.flags,
            crate::schema::fs::SEARCH_FLAGS as u16,
            &self.query,
            &self.cursor,
            &self.extensions,
        )?;
        put_u64(out, self.root_handle);
        put_u16(out, self.flags);
        put_u16(out, self.max_results);
        put_bytes_u16(out, &self.query)?;
        put_bytes_u16(out, &self.cursor)?;
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Search {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            root_handle: decoder.u64()?,
            flags: decoder.u16()?,
            max_results: decoder.u16()?,
            query: decoder.len_bytes_u16()?.to_vec(),
            cursor: decoder.len_bytes_u16()?.to_vec(),
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    pub root_handle: u64,
    pub flags: u16,
    pub max_results: u16,
    pub cursor: Vec<u8>,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Index {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_request(
            self.root_handle,
            self.flags,
            crate::schema::fs::INDEX_FLAGS as u16,
            b"index",
            &self.cursor,
            &self.extensions,
        )?;
        put_u64(out, self.root_handle);
        put_u16(out, self.flags);
        put_u16(out, self.max_results);
        put_bytes_u16(out, &self.cursor)?;
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Index {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            root_handle: decoder.u64()?,
            flags: decoder.u16()?,
            max_results: decoder.u16()?,
            cursor: decoder.len_bytes_u16()?.to_vec(),
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grep {
    pub root_handle: u64,
    pub flags: u16,
    pub max_results: u16,
    pub max_per_file: u16,
    pub query: Vec<u8>,
    pub cursor: Vec<u8>,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Grep {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_request(
            self.root_handle,
            self.flags,
            crate::schema::fs::GREP_FLAGS as u16,
            &self.query,
            &self.cursor,
            &self.extensions,
        )?;
        put_u64(out, self.root_handle);
        put_u16(out, self.flags);
        put_u16(out, self.max_results);
        put_u16(out, self.max_per_file);
        put_u16(out, 0);
        put_bytes_u32(out, &self.query)?;
        put_bytes_u16(out, &self.cursor)?;
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Grep {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let root_handle = decoder.u64()?;
        let flags = decoder.u16()?;
        let max_results = decoder.u16()?;
        let max_per_file = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS GREP reserved field"));
        }
        let value = Self {
            root_handle,
            flags,
            max_results,
            max_per_file,
            query: decoder.len_bytes_u32()?.to_vec(),
            cursor: decoder.len_bytes_u16()?.to_vec(),
            initial_receive_credit: decoder.u64()?,
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
        let body = decoder.len_bytes_u32()?;
        let mut record = Decoder::new(body);
        let kind = record.u16()?;
        let flags = record.u16()?;
        if flags & !1 != 0 {
            return Err(Error::Invalid("FS typed record flags"));
        }
        Ok(Self {
            kind,
            required: flags & 1 != 0,
            body: record.rest().to_vec(),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryReadRecord {
    pub question_index: u16,
    pub status: u16,
    pub path: Option<Path>,
    pub content: Vec<u8>,
}

impl Encode for QueryReadRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.status > crate::schema::core::status::INTERNAL
            || (self.status == crate::schema::core::status::OK && self.path.is_none())
            || (self.status != crate::schema::core::status::OK && !self.content.is_empty())
            || self.content.len() > MAX_QUERY_BYTES
        {
            return Err(Error::Invalid("FS READ result record"));
        }
        put_u16(out, self.question_index);
        put_u16(out, self.status);
        out.push(u8::from(self.path.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(path) = &self.path {
            put_bytes_u32(out, &path.encode()?)?;
        }
        put_bytes_u32(out, &self.content)
    }
}

impl Decode for QueryReadRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let question_index = decoder.u16()?;
        let status = decoder.u16()?;
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("FS READ result path presence"));
        }
        let value = Self {
            question_index,
            status,
            path: if present != 0 {
                Some(Path::decode(decoder.len_bytes_u32()?)?)
            } else {
                None
            },
            content: decoder.len_bytes_u32()?.to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryPathRecord {
    pub path: Path,
    pub flags: u16,
}

impl Encode for QueryPathRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.flags & !(crate::schema::fs::QUERY_PATH_FLAGS as u16) != 0 {
            return Err(Error::Invalid("FS query path flags"));
        }
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        Ok(())
    }
}

impl Decode for QueryPathRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            path: Path::decode(decoder.len_bytes_u32()?)?,
            flags: decoder.u16()?,
        };
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS query path reserved field"));
        }
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryGrepFileRecord {
    pub file_index: u32,
    pub match_count: u32,
    pub flags: u16,
    pub path: Path,
}

impl Encode for QueryGrepFileRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.flags & !(crate::schema::fs::QUERY_GREP_FILE_FLAGS as u16) != 0
            || self.match_count as usize > MAX_QUERY_RECORDS
        {
            return Err(Error::Invalid("FS GREP file record"));
        }
        put_u32(out, self.file_index);
        put_u32(out, self.match_count);
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_bytes_u32(out, &self.path.encode()?)
    }
}

impl Decode for QueryGrepFileRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let file_index = decoder.u32()?;
        let match_count = decoder.u32()?;
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS GREP file reserved field"));
        }
        let value = Self {
            file_index,
            match_count,
            flags,
            path: Path::decode(decoder.len_bytes_u32()?)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryGrepMatchRecord {
    pub file_index: u32,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub text: String,
}

impl Encode for QueryGrepMatchRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if (self.end_line, self.end_column) < (self.line, self.column)
            || self.text.len() > crate::schema::fs::MAX_GREP_LINE_BYTES as usize
            || self.text.as_bytes().contains(&0)
        {
            return Err(Error::Invalid("FS GREP match record"));
        }
        put_u32(out, self.file_index);
        put_u32(out, self.line);
        put_u32(out, self.column);
        put_u32(out, self.end_line);
        put_u32(out, self.end_column);
        put_string_u32(out, &self.text)
    }
}

impl Decode for QueryGrepMatchRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            file_index: decoder.u32()?,
            line: decoder.u32()?,
            column: decoder.u32()?,
            end_line: decoder.u32()?,
            end_column: decoder.u32()?,
            text: decoder.string_u32()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryRecord {
    Read(QueryReadRecord),
    Path(QueryPathRecord),
    GrepFile(QueryGrepFileRecord),
    GrepMatch(QueryGrepMatchRecord),
    UnknownOptional { kind: u16, body: Vec<u8> },
}

impl QueryRecord {
    pub fn to_typed_record(&self) -> Result<TypedRecord> {
        let (kind, body) = match self {
            Self::Read(value) => (crate::schema::fs::QUERY_RECORD_READ as u16, value.encode()?),
            Self::Path(value) => (crate::schema::fs::QUERY_RECORD_PATH as u16, value.encode()?),
            Self::GrepFile(value) => (
                crate::schema::fs::QUERY_RECORD_GREP_FILE as u16,
                value.encode()?,
            ),
            Self::GrepMatch(value) => (
                crate::schema::fs::QUERY_RECORD_GREP_MATCH as u16,
                value.encode()?,
            ),
            Self::UnknownOptional { kind, body } => (*kind, body.clone()),
        };
        Ok(TypedRecord {
            kind,
            required: false,
            body,
        })
    }

    pub fn from_typed_record(record: &TypedRecord) -> Result<Self> {
        match record.kind {
            value if value == crate::schema::fs::QUERY_RECORD_READ as u16 => {
                Ok(Self::Read(QueryReadRecord::decode(&record.body)?))
            }
            value if value == crate::schema::fs::QUERY_RECORD_PATH as u16 => {
                Ok(Self::Path(QueryPathRecord::decode(&record.body)?))
            }
            value if value == crate::schema::fs::QUERY_RECORD_GREP_FILE as u16 => {
                Ok(Self::GrepFile(QueryGrepFileRecord::decode(&record.body)?))
            }
            value if value == crate::schema::fs::QUERY_RECORD_GREP_MATCH as u16 => {
                Ok(Self::GrepMatch(QueryGrepMatchRecord::decode(&record.body)?))
            }
            _ if record.required => Err(Error::Invalid("unknown required FS query record")),
            kind => Ok(Self::UnknownOptional {
                kind,
                body: record.body.clone(),
            }),
        }
    }
}

fn decode_query_records(records: &[TypedRecord], complete_page: bool) -> Result<Vec<QueryRecord>> {
    let mut values = Vec::with_capacity(records.len());
    let mut category = None;
    let mut grep_expected = Vec::<u32>::new();
    let mut grep_actual = Vec::<u32>::new();
    for record in records {
        let value = QueryRecord::from_typed_record(record)?;
        let current_category = match &value {
            QueryRecord::Read(_) => Some(0),
            QueryRecord::Path(_) => Some(1),
            QueryRecord::GrepFile(file) => {
                if file.file_index as usize != grep_expected.len() {
                    return Err(Error::Invalid("FS GREP file index order"));
                }
                grep_expected.push(file.match_count);
                grep_actual.push(0);
                Some(2)
            }
            QueryRecord::GrepMatch(value) => {
                let index = value.file_index as usize;
                let actual = grep_actual
                    .get_mut(index)
                    .ok_or(Error::Invalid("FS GREP match file index"))?;
                *actual = actual.checked_add(1).ok_or(Error::LengthOverflow)?;
                Some(2)
            }
            QueryRecord::UnknownOptional { .. } => None,
        };
        if let Some(current) = current_category {
            if category.is_some_and(|previous| previous != current) {
                return Err(Error::Invalid("mixed FS query record categories"));
            }
            category = Some(current);
        }
        values.push(value);
    }
    if complete_page && grep_expected != grep_actual {
        return Err(Error::Invalid("FS GREP match count"));
    }
    Ok(values)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryRecordBatch {
    pub first_record_index: u32,
    pub records: Vec<TypedRecord>,
}

impl Encode for QueryRecordBatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.records.is_empty() || self.records.len() > MAX_QUERY_RECORDS {
            return Err(Error::Invalid("FS query batch record count"));
        }
        decode_query_records(&self.records, false)?;
        let encoded = encode_records(&self.records)?;
        if encoded.len() > MAX_QUERY_BYTES {
            return Err(limit(
                "FS query batch bytes",
                encoded.len() as u64,
                MAX_QUERY_BYTES as u64,
            ));
        }
        put_u32(out, self.first_record_index);
        put_len_u16(out, self.records.len())?;
        put_u16(out, 0);
        put_bytes_u32(out, &encoded)
    }
}

impl Decode for QueryRecordBatch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let first_record_index = decoder.u32()?;
        let count = usize::from(decoder.u16()?);
        if count == 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("FS query batch count or reserved field"));
        }
        let value = Self {
            first_record_index,
            records: decode_records(decoder.len_bytes_u32()?, count)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageDelivery {
    Inline(Vec<TypedRecord>),
    Transfer(Descriptor),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryPage {
    pub next_cursor: Vec<u8>,
    pub total_hint: u64,
    pub flags: u16,
    pub delivery: PageDelivery,
    pub extensions: Extensions,
}

impl QueryPage {
    pub fn inline_records(&self) -> Result<Option<Vec<QueryRecord>>> {
        match &self.delivery {
            PageDelivery::Inline(records) => decode_query_records(records, true).map(Some),
            PageDelivery::Transfer(_) => Ok(None),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.next_cursor.len() > crate::schema::fs::MAX_CURSOR_BYTES as usize
            || self.flags & !(crate::schema::fs::PAGE_FLAGS as u16) != 0
        {
            return Err(Error::Invalid("FS query cursor"));
        }
        match &self.delivery {
            PageDelivery::Inline(records) => {
                if records.len() > MAX_QUERY_RECORDS {
                    return Err(limit(
                        "FS query records",
                        records.len() as u64,
                        MAX_QUERY_RECORDS as u64,
                    ));
                }
                decode_query_records(records, true)?;
                let encoded = encode_records(records)?;
                if encoded.len() > MAX_QUERY_BYTES {
                    return Err(limit(
                        "FS query bytes",
                        encoded.len() as u64,
                        MAX_QUERY_BYTES as u64,
                    ));
                }
            }
            PageDelivery::Transfer(descriptor) => validate_transfer(
                descriptor,
                crate::schema::fs::QUERY_CONTENT_KIND as u16,
                Mode::Message,
                Direction::SENDER_TO_RECEIVER,
            )?,
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

fn encode_records(records: &[TypedRecord]) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for record in records {
        record.encode_into(&mut bytes)?;
    }
    Ok(bytes)
}

fn decode_records(bytes: &[u8], count: usize) -> Result<Vec<TypedRecord>> {
    if count > MAX_QUERY_RECORDS || count > bytes.len() / 8 {
        return Err(Error::Invalid("FS query record count"));
    }
    let mut decoder = Decoder::new(bytes);
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        records.push(TypedRecord::decode_from(&mut decoder)?);
    }
    decoder.finish()?;
    Ok(records)
}

impl Encode for QueryPage {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_bytes_u16(out, &self.next_cursor)?;
        put_u64(out, self.total_hint);
        put_u16(out, self.flags);
        put_u16(out, 0);
        match &self.delivery {
            PageDelivery::Inline(records) => {
                out.push(crate::schema::fs::PAGE_INLINE as u8);
                out.extend_from_slice(&[0; 3]);
                put_len_u16(out, records.len())?;
                put_u16(out, 0);
                put_bytes_u32(out, &encode_records(records)?)?;
            }
            PageDelivery::Transfer(descriptor) => {
                out.push(crate::schema::fs::PAGE_TRANSFER as u8);
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
        let next_cursor = decoder.len_bytes_u16()?.to_vec();
        let total_hint = decoder.u64()?;
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS query page reserved field"));
        }
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("FS query delivery reserved bytes"));
        }
        let delivery = match kind {
            value if value == crate::schema::fs::PAGE_INLINE as u8 => {
                let count = usize::from(decoder.u16()?);
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("FS query record reserved field"));
                }
                PageDelivery::Inline(decode_records(decoder.len_bytes_u32()?, count)?)
            }
            value if value == crate::schema::fs::PAGE_TRANSFER as u8 => {
                PageDelivery::Transfer(Descriptor::decode(decoder.len_bytes_u32()?)?)
            }
            _ => return Err(Error::Invalid("FS query delivery")),
        };
        let value = Self {
            next_cursor,
            total_hint,
            flags,
            delivery,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Precondition {
    Any,
    Absent,
    Revision(u64),
    Hash([u8; 32]),
}

impl Encode for Precondition {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let kind = match self {
            Self::Any => crate::schema::fs::PRECONDITION_ANY,
            Self::Absent => crate::schema::fs::PRECONDITION_ABSENT,
            Self::Revision(_) => crate::schema::fs::PRECONDITION_REVISION,
            Self::Hash(_) => crate::schema::fs::PRECONDITION_HASH,
        } as u8;
        out.push(kind);
        out.extend_from_slice(&[0; 3]);
        match self {
            Self::Revision(value) => {
                revision(*value, "zero FS precondition revision")?;
                put_u64(out, *value);
            }
            Self::Hash(value) => out.extend_from_slice(value),
            Self::Any | Self::Absent => {}
        }
        Ok(())
    }
}

impl Decode for Precondition {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("FS precondition reserved bytes"));
        }
        let value = match kind {
            value if value == crate::schema::fs::PRECONDITION_ANY as u8 => Self::Any,
            value if value == crate::schema::fs::PRECONDITION_ABSENT as u8 => Self::Absent,
            value if value == crate::schema::fs::PRECONDITION_REVISION as u8 => {
                Self::Revision(decoder.u64()?)
            }
            value if value == crate::schema::fs::PRECONDITION_HASH as u8 => {
                Self::Hash(decoder.array_32()?)
            }
            _ => return Err(Error::Invalid("FS precondition kind")),
        };
        decoder.finish()?;
        if let Self::Revision(value) = value {
            revision(value, "zero FS precondition revision")?;
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictDetail {
    pub path: Path,
    pub current_present: bool,
    pub current_entry_revision: u64,
    pub modified_unix_ns: i64,
    pub current_hash: Option<[u8; 32]>,
}

impl ConflictDetail {
    /// Encode this conflict as the family-defined entry in Core
    /// `ResultPrefix.detail`. Failed Results never carry a family body.
    pub fn result_extension(&self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::fs::RESULT_CONFLICT_DETAIL_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }

    pub fn result_detail(&self) -> Result<Extensions> {
        Ok(Extensions(vec![self.result_extension()?]))
    }

    /// Unknown optional Result-detail extensions are skipped. Core forbids
    /// required extensions in Results, including a known FS detail tag.
    pub fn from_result_detail(detail: &Extensions) -> Result<Option<Self>> {
        detail.validate()?;
        if detail.0.iter().any(|extension| extension.required) {
            return Err(Error::Invalid("required FS Result detail extension"));
        }
        detail
            .0
            .iter()
            .find(|extension| {
                extension.tag == crate::schema::fs::RESULT_CONFLICT_DETAIL_EXTENSION as u16
            })
            .map(|extension| Self::decode(&extension.value))
            .transpose()
    }
}

impl Encode for ConflictDetail {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.path.validate()?;
        if (self.current_present && self.current_entry_revision == 0)
            || (!self.current_present
                && (self.current_entry_revision != 0
                    || self.modified_unix_ns != 0
                    || self.current_hash.is_some()))
        {
            return Err(Error::Invalid("FS conflict current entry"));
        }
        put_bytes_u32(out, &self.path.encode()?)?;
        out.push(u8::from(self.current_present));
        out.push(u8::from(self.current_hash.is_some()));
        put_u16(out, 0);
        put_u64(out, self.current_entry_revision);
        put_i64(out, self.modified_unix_ns);
        if let Some(hash) = self.current_hash {
            out.extend_from_slice(&hash);
        }
        Ok(())
    }
}

impl Decode for ConflictDetail {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let path = Path::decode(decoder.len_bytes_u32()?)?;
        let current_present = decoder.u8()?;
        let hash_present = decoder.u8()?;
        if current_present > 1 || hash_present > 1 || decoder.u16()? != 0 {
            return Err(Error::Invalid("FS conflict presence or reserved field"));
        }
        let value = Self {
            path,
            current_present: current_present != 0,
            current_entry_revision: decoder.u64()?,
            modified_unix_ns: decoder.i64()?,
            current_hash: if hash_present != 0 {
                Some(decoder.array_32()?)
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
pub struct StageWrite {
    pub root_handle: u64,
    pub path: Path,
    pub precondition: Precondition,
    pub flags: u16,
    pub mode: u32,
    pub byte_len: u64,
    pub content_hash: [u8; 32],
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for StageWrite {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.root_handle, "zero FS root handle")?;
        self.path.validate()?;
        if self.byte_len > crate::schema::fs::MAX_STAGED_BYTES {
            return Err(limit(
                "FS staged bytes",
                self.byte_len,
                crate::schema::fs::MAX_STAGED_BYTES,
            ));
        }
        if self.flags & !(crate::schema::fs::STAGE_FLAGS as u16) != 0 {
            return Err(Error::Invalid("FS STAGE_WRITE flags"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.root_handle);
        put_bytes_u32(out, &self.path.encode()?)?;
        put_bytes_u32(out, &self.precondition.encode()?)?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_u32(out, self.mode);
        put_u64(out, self.byte_len);
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for StageWrite {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let root_handle = decoder.u64()?;
        let path = Path::decode(decoder.len_bytes_u32()?)?;
        let precondition = Precondition::decode(decoder.len_bytes_u32()?)?;
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS STAGE_WRITE reserved field"));
        }
        let value = Self {
            root_handle,
            path,
            precondition,
            flags,
            mode: decoder.u32()?,
            byte_len: decoder.u64()?,
            content_hash: decoder.array_32()?,
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageWriteResult {
    pub staging_handle: u64,
    pub descriptor: Descriptor,
    pub extensions: Extensions,
}

impl StageWriteResult {
    fn validate(&self) -> Result<()> {
        handle(self.staging_handle, "zero FS staging handle")?;
        validate_transfer(
            &self.descriptor,
            crate::schema::fs::STAGED_WRITE_CONTENT_KIND as u16,
            Mode::Byte,
            Direction::RECEIVER_TO_SENDER,
        )?;
        self.descriptor.require_upload_stage(self.staging_handle)?;
        reject_unknown_required(&self.extensions, &[])
    }

    /// Return the FS stage discarded when `reset` targets its upload.
    pub fn stage_discarded_by(&self, reset: &Reset) -> Result<Option<UploadStage>> {
        self.validate()?;
        reset.disposed_upload_stage_from(self.staging_handle, core::iter::once(&self.descriptor))
    }
}

impl Encode for StageWriteResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.staging_handle);
        put_bytes_u32(out, &self.descriptor.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for StageWriteResult {
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
pub struct Commit {
    pub staging_handle: u64,
    pub operation_id: [u8; 16],
    pub flags: u16,
    pub extensions: Extensions,
}

impl Encode for Commit {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.staging_handle, "zero FS staging handle")?;
        operation_id(&self.operation_id)?;
        if self.flags & !(crate::schema::fs::COMMIT_FLAGS as u16) != 0 {
            return Err(Error::Invalid("FS COMMIT flags"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.staging_handle);
        out.extend_from_slice(&self.operation_id);
        put_u16(out, self.flags);
        put_u16(out, 0);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Commit {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let staging_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("FS COMMIT reserved field"));
        }
        let value = Self {
            staging_handle,
            operation_id,
            flags,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitResult {
    pub root_revision: u64,
    pub entry_revision: u64,
    pub modified_unix_ns: i64,
    pub content_hash: [u8; 32],
}

impl Encode for CommitResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        revision(self.root_revision, "zero FS root revision")?;
        revision(self.entry_revision, "zero FS entry revision")?;
        put_u64(out, self.root_revision);
        put_u64(out, self.entry_revision);
        put_i64(out, self.modified_unix_ns);
        out.extend_from_slice(&self.content_hash);
        Ok(())
    }
}

impl Decode for CommitResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            root_revision: decoder.u64()?,
            entry_revision: decoder.u64()?,
            modified_unix_ns: decoder.i64()?,
            content_hash: decoder.array_32()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyItem {
    WriteInline {
        path: Path,
        precondition: Precondition,
        create_parents: bool,
        mode: u32,
        content: Vec<u8>,
    },
    Mkdir {
        path: Path,
        precondition: Precondition,
        create_parents: bool,
        mode: u32,
    },
    Remove {
        path: Path,
        precondition: Precondition,
        flags: u16,
    },
    Rename {
        from: Path,
        to: Path,
        precondition: Precondition,
        create_parents: bool,
    },
    Symlink {
        path: Path,
        target: Vec<u8>,
        precondition: Precondition,
        create_parents: bool,
    },
    Hardlink {
        source: Path,
        target: Path,
        precondition: Precondition,
        create_parents: bool,
    },
}

impl ApplyItem {
    fn encode_body(&self) -> Result<(u16, Vec<u8>)> {
        let mut out = Vec::new();
        let kind = match self {
            Self::WriteInline {
                path,
                precondition,
                mode,
                content,
                ..
            } => {
                if content.len() > MAX_INLINE_BYTES {
                    return Err(limit(
                        "FS inline apply bytes",
                        content.len() as u64,
                        MAX_INLINE_BYTES as u64,
                    ));
                }
                put_bytes_u32(&mut out, &path.encode()?)?;
                put_bytes_u32(&mut out, &precondition.encode()?)?;
                put_u32(&mut out, *mode);
                put_bytes_u32(&mut out, content)?;
                crate::schema::fs::APPLY_WRITE_INLINE as u16
            }
            Self::Mkdir {
                path,
                precondition,
                mode,
                ..
            } => {
                put_bytes_u32(&mut out, &path.encode()?)?;
                put_bytes_u32(&mut out, &precondition.encode()?)?;
                put_u32(&mut out, *mode);
                crate::schema::fs::APPLY_MKDIR as u16
            }
            Self::Remove {
                path,
                precondition,
                flags,
            } => {
                if *flags & !(crate::schema::fs::REMOVE_FLAGS as u16) != 0 {
                    return Err(Error::Invalid("FS APPLY remove flags"));
                }
                put_bytes_u32(&mut out, &path.encode()?)?;
                put_bytes_u32(&mut out, &precondition.encode()?)?;
                put_u16(&mut out, *flags);
                put_u16(&mut out, 0);
                crate::schema::fs::APPLY_REMOVE as u16
            }
            Self::Rename {
                from,
                to,
                precondition,
                ..
            } => {
                if from == to {
                    return Err(Error::Invalid("FS APPLY identical rename paths"));
                }
                put_bytes_u32(&mut out, &from.encode()?)?;
                put_bytes_u32(&mut out, &to.encode()?)?;
                put_bytes_u32(&mut out, &precondition.encode()?)?;
                crate::schema::fs::APPLY_RENAME as u16
            }
            Self::Symlink {
                path,
                target,
                precondition,
                ..
            } => {
                if target.is_empty() || target.len() > MAX_PATH_BYTES || target.contains(&0) {
                    return Err(Error::Invalid("FS APPLY symlink target"));
                }
                put_bytes_u32(&mut out, &path.encode()?)?;
                put_bytes_u32(&mut out, target)?;
                put_bytes_u32(&mut out, &precondition.encode()?)?;
                crate::schema::fs::APPLY_SYMLINK as u16
            }
            Self::Hardlink {
                source,
                target,
                precondition,
                ..
            } => {
                if source == target {
                    return Err(Error::Invalid("FS APPLY identical hardlink paths"));
                }
                put_bytes_u32(&mut out, &source.encode()?)?;
                put_bytes_u32(&mut out, &target.encode()?)?;
                put_bytes_u32(&mut out, &precondition.encode()?)?;
                crate::schema::fs::APPLY_HARDLINK as u16
            }
        };
        Ok((kind, out))
    }

    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        let (kind, body) = self.encode_body()?;
        let create_parents = match self {
            Self::WriteInline { create_parents, .. }
            | Self::Mkdir { create_parents, .. }
            | Self::Rename { create_parents, .. }
            | Self::Symlink { create_parents, .. }
            | Self::Hardlink { create_parents, .. } => *create_parents,
            Self::Remove { .. } => false,
        };
        let len = 4usize
            .checked_add(body.len())
            .ok_or(Error::LengthOverflow)?;
        put_len_u32(out, len)?;
        put_u16(out, kind);
        put_u16(
            out,
            if create_parents {
                crate::schema::fs::APPLY_ITEM_CREATE_PARENTS as u16
            } else {
                0
            },
        );
        out.extend_from_slice(&body);
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let bytes = decoder.len_bytes_u32()?;
        let mut item = Decoder::new(bytes);
        let kind = item.u16()?;
        let item_flags = item.u16()?;
        if item_flags & !(crate::schema::fs::APPLY_ITEM_FLAGS as u16) != 0 {
            return Err(Error::Invalid("FS APPLY item flags"));
        }
        let create_parents = item_flags & crate::schema::fs::APPLY_ITEM_CREATE_PARENTS as u16 != 0;
        let value = match kind {
            value if value == crate::schema::fs::APPLY_WRITE_INLINE as u16 => Self::WriteInline {
                path: Path::decode(item.len_bytes_u32()?)?,
                precondition: Precondition::decode(item.len_bytes_u32()?)?,
                create_parents,
                mode: item.u32()?,
                content: item.len_bytes_u32()?.to_vec(),
            },
            value if value == crate::schema::fs::APPLY_MKDIR as u16 => Self::Mkdir {
                path: Path::decode(item.len_bytes_u32()?)?,
                precondition: Precondition::decode(item.len_bytes_u32()?)?,
                create_parents,
                mode: item.u32()?,
            },
            value if value == crate::schema::fs::APPLY_REMOVE as u16 => {
                if create_parents {
                    return Err(Error::Invalid("FS APPLY remove create-parents flag"));
                }
                let path = Path::decode(item.len_bytes_u32()?)?;
                let precondition = Precondition::decode(item.len_bytes_u32()?)?;
                let flags = item.u16()?;
                if item.u16()? != 0 {
                    return Err(Error::Invalid("FS APPLY remove reserved field"));
                }
                Self::Remove {
                    path,
                    precondition,
                    flags,
                }
            }
            value if value == crate::schema::fs::APPLY_RENAME as u16 => Self::Rename {
                from: Path::decode(item.len_bytes_u32()?)?,
                to: Path::decode(item.len_bytes_u32()?)?,
                precondition: Precondition::decode(item.len_bytes_u32()?)?,
                create_parents,
            },
            value if value == crate::schema::fs::APPLY_SYMLINK as u16 => Self::Symlink {
                path: Path::decode(item.len_bytes_u32()?)?,
                target: item.len_bytes_u32()?.to_vec(),
                precondition: Precondition::decode(item.len_bytes_u32()?)?,
                create_parents,
            },
            value if value == crate::schema::fs::APPLY_HARDLINK as u16 => Self::Hardlink {
                source: Path::decode(item.len_bytes_u32()?)?,
                target: Path::decode(item.len_bytes_u32()?)?,
                precondition: Precondition::decode(item.len_bytes_u32()?)?,
                create_parents,
            },
            _ => return Err(Error::Invalid("FS APPLY item kind")),
        };
        item.finish()?;
        let mut ignored = Vec::new();
        value.encode_into(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Apply {
    pub root_handle: u64,
    pub operation_id: [u8; 16],
    pub flags: u16,
    pub items: Vec<ApplyItem>,
    pub extensions: Extensions,
}

impl Encode for Apply {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.root_handle, "zero FS root handle")?;
        operation_id(&self.operation_id)?;
        if self.flags & !(crate::schema::fs::APPLY_FLAGS as u16) != 0
            || self.items.is_empty()
            || self.items.len() > MAX_BATCH_ITEMS
        {
            return Err(Error::Invalid("FS APPLY flags or item count"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.root_handle);
        out.extend_from_slice(&self.operation_id);
        put_u16(out, self.flags);
        put_len_u16(out, self.items.len())?;
        for item in &self.items {
            item.encode_into(out)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Apply {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let root_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let flags = decoder.u16()?;
        let count = usize::from(decoder.u16()?);
        if count == 0 || count > MAX_BATCH_ITEMS || count > decoder.remaining() / 8 {
            return Err(Error::Invalid("FS APPLY item count"));
        }
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(ApplyItem::decode_from(&mut decoder)?);
        }
        let value = Self {
            root_handle,
            operation_id,
            flags,
            items,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyItemResult {
    pub index: u16,
    pub status: u16,
    pub entry_revision: u64,
    pub modified_unix_ns: i64,
    pub content_hash: Option<[u8; 32]>,
    pub detail: String,
}

impl ApplyItemResult {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.status > crate::schema::core::status::INTERNAL
            || (self.status == crate::schema::core::status::OK && self.entry_revision == 0)
            || self.detail.len() > 4096
        {
            return Err(Error::Invalid("FS APPLY result detail"));
        }
        put_u16(out, self.index);
        put_u16(out, self.status);
        put_u64(out, self.entry_revision);
        put_i64(out, self.modified_unix_ns);
        out.push(u8::from(self.content_hash.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(hash) = self.content_hash {
            out.extend_from_slice(&hash);
        }
        put_bytes_u16(out, self.detail.as_bytes())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let index = decoder.u16()?;
        let status = decoder.u16()?;
        let entry_revision = decoder.u64()?;
        let modified_unix_ns = decoder.i64()?;
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("FS APPLY result hash presence"));
        }
        let content_hash = if present != 0 {
            Some(decoder.array_32()?)
        } else {
            None
        };
        let detail = decoder.string_u16()?;
        let value = Self {
            index,
            status,
            entry_revision,
            modified_unix_ns,
            content_hash,
            detail,
        };
        let mut ignored = Vec::new();
        value.encode_into(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApplyResult {
    pub root_revision: u64,
    pub items: Vec<ApplyItemResult>,
    pub extensions: Extensions,
}

impl Encode for ApplyResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        revision(self.root_revision, "zero FS root revision")?;
        if self.items.is_empty() || self.items.len() > MAX_BATCH_ITEMS {
            return Err(Error::Invalid("FS APPLY result count"));
        }
        let mut indices = BTreeSet::new();
        if self.items.iter().any(|item| !indices.insert(item.index)) {
            return Err(Error::Invalid("duplicate FS APPLY result index"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.root_revision);
        put_len_u16(out, self.items.len())?;
        put_u16(out, 0);
        for item in &self.items {
            item.encode_into(out)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for ApplyResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let root_revision = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count == 0
            || count > MAX_BATCH_ITEMS
            || count > decoder.remaining() / 26
        {
            return Err(Error::Invalid("FS APPLY result count"));
        }
        let mut items = Vec::with_capacity(count);
        for _ in 0..count {
            items.push(ApplyItemResult::decode_from(&mut decoder)?);
        }
        let value = Self {
            root_revision,
            items,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn staged_write_descriptor(staging_handle: u64) -> Descriptor {
        let stage = UploadStage {
            staging_handle,
            expires_server_ns: 1,
        };
        Descriptor {
            transfer_id: 2,
            mode: Mode::Byte,
            direction: Direction::RECEIVER_TO_SENDER,
            receiver_send_credit: 4096,
            sender_send_credit: 0,
            max_item_bytes: 0,
            max_chunk_bytes: 1024,
            content_family: crate::family::FS,
            content_kind: crate::schema::fs::STAGED_WRITE_CONTENT_KIND as u16,
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

    fn truncations<T: Decode>(bytes: &[u8]) {
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
        T::decode(bytes).unwrap();
    }

    fn path(name: &[u8]) -> Path {
        Path {
            components: vec![name.to_vec()],
        }
    }

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        assert_eq!(Limits::HARD.max_catalog_entries, 1_000_000);
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);
        let mut invalid = Limits::HARD;
        invalid.max_query_concurrency = 0;
        assert!(invalid.to_extensions().is_err());
    }

    #[test]
    fn root_sources_and_watch_policy_are_exact() {
        let terminal = RootSource::TerminalCwd {
            terminal_handle: 7,
            suffix: Path {
                components: vec![b"src".to_vec(), b"app".to_vec()],
            },
        };
        truncations::<RootSource>(&terminal.encode().unwrap());
        assert_eq!(
            RootSource::decode(&terminal.encode().unwrap()).unwrap(),
            terminal
        );
        assert_eq!(
            RootSource::decode(&RootSource::Staging.encode().unwrap()).unwrap(),
            RootSource::Staging
        );

        let watch = Watch {
            root_handle: 1,
            flags: (crate::schema::fs::WATCH_RECURSIVE
                | crate::schema::fs::WATCH_GITIGNORE
                | crate::schema::fs::WATCH_DOT_IGNORE
                | crate::schema::fs::WATCH_EXCLUDE_GIT) as u16,
            settle_ms: 20,
            inline_max: 1024,
            ignore_patterns: "target/\n!target/keep".into(),
            state: StateWatch {
                initial_credit: 4096,
                resume: None,
                extensions: Extensions::default(),
            },
        };
        truncations::<Watch>(&watch.encode().unwrap());
        assert_eq!(Watch::decode(&watch.encode().unwrap()).unwrap(), watch);

        let stage = StageWriteResult {
            staging_handle: 11,
            descriptor: staged_write_descriptor(11),
            extensions: Extensions::default(),
        };
        truncations::<StageWriteResult>(&stage.encode().unwrap());
        let reset = Reset {
            transfer_id: 2,
            status: crate::schema::core::status::CANCELLED,
            detail: Vec::new(),
        };
        assert_eq!(
            stage.stage_discarded_by(&reset).unwrap(),
            stage.descriptor.upload_stage().unwrap()
        );
    }

    #[test]
    fn entry_flags_symlink_hash_and_operation_id_are_exact() {
        let operation = [9; 16];
        let entry = EntryRecord {
            path: path(b"link"),
            entry_revision: 2,
            flags: (crate::schema::fs::ENTRY_SYMLINK_DIRECTORY
                | crate::schema::fs::ENTRY_DIRECTORY_FILTERED) as u8,
            mode: 0o777,
            modified_unix_ns: 3,
            body: EntryBody::Symlink {
                content_hash: [4; 32],
                target: b"directory".to_vec(),
            },
            extensions: Extensions(vec![operation_id_extension(operation).unwrap()]),
        };
        let bytes = entry.encode().unwrap();
        truncations::<EntryRecord>(&bytes);
        let decoded = EntryRecord::decode(&bytes).unwrap();
        assert_eq!(decoded, entry);
        assert_eq!(decoded.operation_id().unwrap(), Some(operation));
    }

    #[test]
    fn typed_query_records_and_page_are_exact() {
        let path_record = QueryRecord::Path(QueryPathRecord {
            path: Path {
                components: vec![b"src".to_vec(), b"main.rs".to_vec()],
            },
            flags: 0,
        })
        .to_typed_record()
        .unwrap();
        assert!(matches!(
            QueryRecord::from_typed_record(&path_record).unwrap(),
            QueryRecord::Path(_)
        ));
        let records = vec![
            QueryRecord::GrepFile(QueryGrepFileRecord {
                file_index: 0,
                match_count: 1,
                flags: crate::schema::fs::QUERY_GREP_FILE_IGNORED as u16,
                path: path(b"ignored.rs"),
            })
            .to_typed_record()
            .unwrap(),
            QueryRecord::GrepMatch(QueryGrepMatchRecord {
                file_index: 0,
                line: 3,
                column: 4,
                end_line: 3,
                end_column: 7,
                text: "yas".into(),
            })
            .to_typed_record()
            .unwrap(),
        ];
        for record in &records {
            assert!(!matches!(
                QueryRecord::from_typed_record(record).unwrap(),
                QueryRecord::UnknownOptional { .. }
            ));
        }
        let page = QueryPage {
            next_cursor: b"next".to_vec(),
            total_hint: 9,
            flags: crate::schema::fs::PAGE_TRUNCATED as u16,
            delivery: PageDelivery::Inline(records),
            extensions: Extensions::default(),
        };
        truncations::<QueryPage>(&page.encode().unwrap());
        assert_eq!(QueryPage::decode(&page.encode().unwrap()).unwrap(), page);
    }

    #[test]
    fn conflict_and_mutation_results_preserve_current_metadata() {
        let conflict = ConflictDetail {
            path: path(b"file"),
            current_present: true,
            current_entry_revision: 8,
            modified_unix_ns: 9,
            current_hash: Some([10; 32]),
        };
        truncations::<ConflictDetail>(&conflict.encode().unwrap());
        assert_eq!(
            ConflictDetail::decode(&conflict.encode().unwrap()).unwrap(),
            conflict
        );
        let detail = conflict.result_detail().unwrap();
        assert_eq!(
            ConflictDetail::from_result_detail(&detail).unwrap(),
            Some(conflict.clone())
        );
        let prefix = crate::core::ResultPrefix {
            status: crate::core::Status::Conflict,
            detail,
            body: Vec::new(),
        };
        truncations::<crate::core::ResultPrefix>(&prefix.encode().unwrap());
        let decoded = crate::core::ResultPrefix::decode(&prefix.encode().unwrap()).unwrap();
        assert_eq!(
            ConflictDetail::from_result_detail(&decoded.detail).unwrap(),
            Some(conflict.clone())
        );

        let commit = CommitResult {
            root_revision: 1,
            entry_revision: 2,
            modified_unix_ns: 3,
            content_hash: [4; 32],
        };
        truncations::<CommitResult>(&commit.encode().unwrap());

        let result = ApplyResult {
            root_revision: 5,
            items: vec![ApplyItemResult {
                index: 0,
                status: crate::schema::core::status::CONFLICT,
                entry_revision: 6,
                modified_unix_ns: 7,
                content_hash: Some([8; 32]),
                detail: "changed".into(),
            }],
            extensions: Extensions::default(),
        };
        truncations::<ApplyResult>(&result.encode().unwrap());
        assert_eq!(
            ApplyResult::decode(&result.encode().unwrap()).unwrap(),
            result
        );
    }

    #[test]
    fn open_paths_and_queries_round_trip() {
        let open = Open {
            flags: crate::schema::fs::OPEN_READ_ONLY as u16,
            source: RootSource::PlatformPath(b"/tmp/raw".to_vec()),
            extensions: Extensions::default(),
        };
        truncations::<Open>(&open.encode().unwrap());
        let search = Search {
            root_handle: 1,
            flags: crate::schema::fs::SEARCH_CASE_SENSITIVE as u16,
            max_results: 10,
            query: b"src".to_vec(),
            cursor: vec![],
            initial_receive_credit: 1024,
            extensions: Extensions::default(),
        };
        truncations::<Search>(&search.encode().unwrap());
    }

    #[test]
    fn state_records_preserve_move_and_raw_paths() {
        let mutation = StateMutation::Move(MoveRecord {
            from: path(&[0xff]),
            to: path(b"renamed"),
            operation_id: Some([1; 16]),
        });
        let record = mutation.state_record(RecordKind::Add).unwrap();
        assert_eq!(StateMutation::decode_record(&record).unwrap(), mutation);
    }

    #[test]
    fn staged_and_apply_mutations_reject_truncation() {
        let stage = StageWrite {
            root_handle: 1,
            path: path(b"file"),
            precondition: Precondition::Absent,
            flags: crate::schema::fs::STAGE_CREATE_PARENTS as u16,
            mode: 0o644,
            byte_len: 3,
            content_hash: [2; 32],
            initial_receive_credit: 1024,
            extensions: Extensions::default(),
        };
        truncations::<StageWrite>(&stage.encode().unwrap());
        let apply = Apply {
            root_handle: 1,
            operation_id: [3; 16],
            flags: 0,
            items: vec![ApplyItem::WriteInline {
                path: path(b"file"),
                precondition: Precondition::Absent,
                create_parents: true,
                mode: 0o644,
                content: b"yas".to_vec(),
            }],
            extensions: Extensions::default(),
        };
        truncations::<Apply>(&apply.encode().unwrap());
    }
}
