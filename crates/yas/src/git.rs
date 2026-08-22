//! YAS Git family version 1 wire values.

use crate::prelude::*;

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, limit_u32, put_bytes_u16,
    put_bytes_u32, put_i16, put_i64, put_len_u16, put_len_u32, put_string_u16, put_string_u32,
    put_u16, put_u32, put_u64, read_limit_u32,
};
use crate::fs::Path as FsPath;
use crate::state::{Record, RecordKind, StateEvent, Watch as StateWatch};
use crate::transfer::{Descriptor, Direction, Mode};

pub const VERSION: u16 = crate::schema::git::VERSION;
pub const MAX_QUERY_RECORDS: usize = crate::schema::git::MAX_QUERY_RECORDS as usize;
pub const MAX_QUERY_BYTES: usize = crate::schema::git::MAX_QUERY_BYTES as usize;
pub const MAX_CURSOR_BYTES: usize = crate::schema::git::MAX_CURSOR_BYTES as usize;
pub const MAX_SPEC_BYTES: usize = crate::schema::git::MAX_SPEC_BYTES as usize;

pub mod request_kind {
    pub use crate::schema::git::request::*;
}

pub mod event_kind {
    pub use crate::schema::git::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_repositories_per_session: u32,
    pub max_watches_per_repository: u32,
    pub max_watched_queries_per_repository: u32,
    pub max_query_records: u32,
    pub max_query_bytes: u32,
    pub max_cursor_bytes: u32,
    pub max_spec_bytes: u32,
    pub max_inline_bytes: u32,
    pub max_concurrent_queries: u32,
    pub max_concurrent_fetches: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_repositories_per_session: crate::schema::git::MAX_REPOSITORIES_PER_SESSION as u32,
        max_watches_per_repository: crate::schema::git::MAX_WATCHES_PER_REPOSITORY as u32,
        max_watched_queries_per_repository: crate::schema::git::MAX_WATCHED_QUERIES_PER_REPOSITORY
            as u32,
        max_query_records: crate::schema::git::MAX_QUERY_RECORDS as u32,
        max_query_bytes: crate::schema::git::MAX_QUERY_BYTES as u32,
        max_cursor_bytes: crate::schema::git::MAX_CURSOR_BYTES as u32,
        max_spec_bytes: crate::schema::git::MAX_SPEC_BYTES as u32,
        max_inline_bytes: crate::schema::git::MAX_INLINE_BYTES as u32,
        max_concurrent_queries: crate::schema::git::MAX_CONCURRENT_QUERIES as u32,
        max_concurrent_fetches: crate::schema::git::MAX_CONCURRENT_FETCHES as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        if [
            (
                self.max_repositories_per_session,
                hard.max_repositories_per_session,
            ),
            (
                self.max_watches_per_repository,
                hard.max_watches_per_repository,
            ),
            (
                self.max_watched_queries_per_repository,
                hard.max_watched_queries_per_repository,
            ),
            (self.max_query_records, hard.max_query_records),
            (self.max_query_bytes, hard.max_query_bytes),
            (self.max_cursor_bytes, hard.max_cursor_bytes),
            (self.max_spec_bytes, hard.max_spec_bytes),
            (self.max_inline_bytes, hard.max_inline_bytes),
            (self.max_concurrent_queries, hard.max_concurrent_queries),
            (self.max_concurrent_fetches, hard.max_concurrent_fetches),
        ]
        .into_iter()
        .any(|(value, maximum)| value == 0 || value > maximum)
        {
            return Err(Error::Invalid("Git family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::git::LIMIT_MAX_REPOSITORIES_PER_SESSION,
                self.max_repositories_per_session,
            ),
            limit_u32(
                crate::schema::git::LIMIT_MAX_WATCHES_PER_REPOSITORY,
                self.max_watches_per_repository,
            ),
            limit_u32(
                crate::schema::git::LIMIT_MAX_WATCHED_QUERIES_PER_REPOSITORY,
                self.max_watched_queries_per_repository,
            ),
            limit_u32(
                crate::schema::git::LIMIT_MAX_QUERY_RECORDS,
                self.max_query_records,
            ),
            limit_u32(
                crate::schema::git::LIMIT_MAX_QUERY_BYTES,
                self.max_query_bytes,
            ),
            limit_u32(
                crate::schema::git::LIMIT_MAX_CURSOR_BYTES,
                self.max_cursor_bytes,
            ),
            limit_u32(
                crate::schema::git::LIMIT_MAX_SPEC_BYTES,
                self.max_spec_bytes,
            ),
            limit_u32(
                crate::schema::git::LIMIT_MAX_INLINE_BYTES,
                self.max_inline_bytes,
            ),
            limit_u32(
                crate::schema::git::LIMIT_MAX_CONCURRENT_QUERIES,
                self.max_concurrent_queries,
            ),
            limit_u32(
                crate::schema::git::LIMIT_MAX_CONCURRENT_FETCHES,
                self.max_concurrent_fetches,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required(
            extensions,
            &[
                crate::schema::git::LIMIT_MAX_REPOSITORIES_PER_SESSION as u16,
                crate::schema::git::LIMIT_MAX_WATCHES_PER_REPOSITORY as u16,
                crate::schema::git::LIMIT_MAX_WATCHED_QUERIES_PER_REPOSITORY as u16,
                crate::schema::git::LIMIT_MAX_QUERY_RECORDS as u16,
                crate::schema::git::LIMIT_MAX_QUERY_BYTES as u16,
                crate::schema::git::LIMIT_MAX_CURSOR_BYTES as u16,
                crate::schema::git::LIMIT_MAX_SPEC_BYTES as u16,
                crate::schema::git::LIMIT_MAX_INLINE_BYTES as u16,
                crate::schema::git::LIMIT_MAX_CONCURRENT_QUERIES as u16,
                crate::schema::git::LIMIT_MAX_CONCURRENT_FETCHES as u16,
            ],
        )?;
        let value = Self {
            max_repositories_per_session: read_limit_u32(
                extensions,
                crate::schema::git::LIMIT_MAX_REPOSITORIES_PER_SESSION,
            )?,
            max_watches_per_repository: read_limit_u32(
                extensions,
                crate::schema::git::LIMIT_MAX_WATCHES_PER_REPOSITORY,
            )?,
            max_watched_queries_per_repository: read_limit_u32(
                extensions,
                crate::schema::git::LIMIT_MAX_WATCHED_QUERIES_PER_REPOSITORY,
            )?,
            max_query_records: read_limit_u32(
                extensions,
                crate::schema::git::LIMIT_MAX_QUERY_RECORDS,
            )?,
            max_query_bytes: read_limit_u32(extensions, crate::schema::git::LIMIT_MAX_QUERY_BYTES)?,
            max_cursor_bytes: read_limit_u32(
                extensions,
                crate::schema::git::LIMIT_MAX_CURSOR_BYTES,
            )?,
            max_spec_bytes: read_limit_u32(extensions, crate::schema::git::LIMIT_MAX_SPEC_BYTES)?,
            max_inline_bytes: read_limit_u32(
                extensions,
                crate::schema::git::LIMIT_MAX_INLINE_BYTES,
            )?,
            max_concurrent_queries: read_limit_u32(
                extensions,
                crate::schema::git::LIMIT_MAX_CONCURRENT_QUERIES,
            )?,
            max_concurrent_fetches: read_limit_u32(
                extensions,
                crate::schema::git::LIMIT_MAX_CONCURRENT_FETCHES,
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
        Err(Error::Invalid("zero Git operation ID"))
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
        return Err(Error::Invalid("unknown required Git extension"));
    }
    Ok(())
}

fn limit(name: &'static str, actual: u64, maximum: u64) -> Error {
    Error::LimitExceeded {
        limit: name,
        actual,
        maximum,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObjectId {
    pub algorithm: u8,
    pub bytes: Vec<u8>,
}

impl ObjectId {
    fn validate(&self) -> Result<()> {
        let expected = match self.algorithm {
            value if value == crate::schema::git::OBJECT_SHA1 as u8 => 20,
            value if value == crate::schema::git::OBJECT_SHA256 as u8 => 32,
            _ => return Err(Error::Invalid("Git object algorithm")),
        };
        if self.bytes.len() != expected {
            return Err(Error::Invalid("Git object ID length"));
        }
        Ok(())
    }

    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.algorithm);
        out.push(self.bytes.len() as u8);
        put_u16(out, 0);
        out.extend_from_slice(&self.bytes);
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let algorithm = decoder.u8()?;
        let len = usize::from(decoder.u8()?);
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git object ID reserved field"));
        }
        let value = Self {
            algorithm,
            bytes: decoder.take(len)?.to_vec(),
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for ObjectId {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_into(out)
    }
}

impl Decode for ObjectId {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectRecord {
    pub role: u8,
    pub object: ObjectId,
}

impl Encode for ObjectRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.role > crate::schema::git::OBJECT_ROLE_HIDE as u8 {
            return Err(Error::Invalid("Git object result role"));
        }
        out.push(self.role);
        out.extend_from_slice(&[0; 3]);
        self.object.encode_into(out)
    }
}

impl Decode for ObjectRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let role = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git object result reserved bytes"));
        }
        let value = Self {
            role,
            object: ObjectId::decode_from(&mut decoder)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RepositorySource {
    PlatformPath(Vec<u8>),
    Fs {
        root_handle: u64,
        path: FsPath,
    },
    Submodule {
        parent_repository: u64,
        path: FsPath,
    },
    TerminalCwd {
        terminal_handle: u64,
        suffix: FsPath,
    },
}

impl RepositorySource {
    fn validate(&self) -> Result<()> {
        match self {
            Self::PlatformPath(path)
                if !path.is_empty()
                    && path.len() <= crate::schema::git::MAX_PATH_BYTES as usize
                    && !path.contains(&0) =>
            {
                Ok(())
            }
            Self::PlatformPath(_) => Err(Error::Invalid("Git platform path")),
            Self::Fs { root_handle, path } => {
                handle(*root_handle, "zero Git FS root handle")?;
                path.encode().map(|_| ())
            }
            Self::Submodule {
                parent_repository,
                path,
            } => {
                handle(*parent_repository, "zero parent Git repository handle")?;
                if path.components.is_empty() {
                    return Err(Error::Invalid("empty Git submodule path"));
                }
                path.encode().map(|_| ())
            }
            Self::TerminalCwd {
                terminal_handle,
                suffix,
            } => {
                handle(*terminal_handle, "zero Git Terminal handle")?;
                suffix.encode().map(|_| ())
            }
        }
    }
}

impl Encode for RepositorySource {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        match self {
            Self::PlatformPath(path) => {
                out.push(crate::schema::git::SOURCE_PLATFORM_PATH as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, path)?;
            }
            Self::Fs { root_handle, path } => {
                out.push(crate::schema::git::SOURCE_FS as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *root_handle);
                put_bytes_u32(out, &path.encode()?)?;
            }
            Self::Submodule {
                parent_repository,
                path,
            } => {
                out.push(crate::schema::git::SOURCE_SUBMODULE as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *parent_repository);
                put_bytes_u32(out, &path.encode()?)?;
            }
            Self::TerminalCwd {
                terminal_handle,
                suffix,
            } => {
                out.push(crate::schema::git::SOURCE_TERMINAL_CWD as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *terminal_handle);
                put_bytes_u32(out, &suffix.encode()?)?;
            }
        }
        Ok(())
    }
}

impl Decode for RepositorySource {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git repository source reserved bytes"));
        }
        let value = match kind {
            value if value == crate::schema::git::SOURCE_PLATFORM_PATH as u8 => {
                Self::PlatformPath(decoder.len_bytes_u32()?.to_vec())
            }
            value if value == crate::schema::git::SOURCE_FS as u8 => Self::Fs {
                root_handle: decoder.u64()?,
                path: FsPath::decode(decoder.len_bytes_u32()?)?,
            },
            value if value == crate::schema::git::SOURCE_SUBMODULE as u8 => Self::Submodule {
                parent_repository: decoder.u64()?,
                path: FsPath::decode(decoder.len_bytes_u32()?)?,
            },
            value if value == crate::schema::git::SOURCE_TERMINAL_CWD as u8 => Self::TerminalCwd {
                terminal_handle: decoder.u64()?,
                suffix: FsPath::decode(decoder.len_bytes_u32()?)?,
            },
            _ => return Err(Error::Invalid("Git repository source kind")),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub source: RepositorySource,
    pub extensions: Extensions,
}

impl Encode for Open {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        reject_unknown_required(&self.extensions, &[])?;
        put_bytes_u32(out, &self.source.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Open {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            source: RepositorySource::decode(decoder.len_bytes_u32()?)?,
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
    pub repository_handle: u64,
    pub repository_revision: u64,
    pub object_algorithm: u8,
    pub repository_flags: u16,
    pub canonical_worktree_path: Vec<u8>,
    pub canonical_git_dir: Vec<u8>,
    pub extensions: Extensions,
}

impl Encode for OpenResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.repository_handle, "zero Git repository handle")?;
        revision(self.repository_revision, "zero Git repository revision")?;
        if !matches!(
            self.object_algorithm,
            value if value == crate::schema::git::OBJECT_SHA1 as u8
                || value == crate::schema::git::OBJECT_SHA256 as u8
        ) || self.repository_flags & !(crate::schema::git::REPOSITORY_FLAGS as u16) != 0
            || (self.repository_flags & crate::schema::git::REPOSITORY_BARE as u16 == 0
                && self.canonical_worktree_path.is_empty())
            || (self.repository_flags & crate::schema::git::REPOSITORY_BARE as u16 != 0
                && !self.canonical_worktree_path.is_empty())
            || self.canonical_worktree_path.len() > crate::schema::git::MAX_PATH_BYTES as usize
            || self.canonical_worktree_path.contains(&0)
            || self.canonical_git_dir.is_empty()
            || self.canonical_git_dir.len() > crate::schema::git::MAX_PATH_BYTES as usize
            || self.canonical_git_dir.contains(&0)
        {
            return Err(Error::Invalid("Git OPEN result metadata"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.repository_handle);
        put_u64(out, self.repository_revision);
        out.push(self.object_algorithm);
        out.push(0);
        put_u16(out, self.repository_flags);
        put_bytes_u32(out, &self.canonical_worktree_path)?;
        put_bytes_u32(out, &self.canonical_git_dir)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for OpenResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let repository_handle = decoder.u64()?;
        let repository_revision = decoder.u64()?;
        let object_algorithm = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Git OPEN result reserved bytes"));
        }
        let repository_flags = decoder.u16()?;
        let value = Self {
            repository_handle,
            repository_revision,
            object_algorithm,
            repository_flags,
            canonical_worktree_path: decoder.len_bytes_u32()?.to_vec(),
            canonical_git_dir: decoder.len_bytes_u32()?.to_vec(),
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
    pub repository_handle: u64,
    pub extensions: Extensions,
}

impl Encode for Close {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.repository_handle, "zero Git repository handle")?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.repository_handle);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Close {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            repository_handle: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WatchOptions {
    pub refs_settle_ms: u16,
    pub status_settle_ms: u16,
    pub ref_prefixes: Vec<Vec<u8>>,
}

impl WatchOptions {
    pub fn validate(&self) -> Result<()> {
        if self.ref_prefixes.len() > crate::schema::git::MAX_REF_PREFIXES as usize {
            return Err(limit(
                "Git ref prefixes",
                self.ref_prefixes.len() as u64,
                crate::schema::git::MAX_REF_PREFIXES,
            ));
        }
        let mut previous: Option<&[u8]> = None;
        for prefix in &self.ref_prefixes {
            validate_spec(prefix)?;
            if previous.is_some_and(|value| value >= prefix.as_slice()) {
                return Err(Error::Invalid("Git ref prefix order"));
            }
            previous = Some(prefix);
        }
        Ok(())
    }

    pub fn to_extensions(&self) -> Result<Extensions> {
        self.validate()?;
        let mut values = Vec::new();
        if self.refs_settle_ms != 0 {
            values.push(Extension {
                tag: crate::schema::git::WATCH_REFS_SETTLE_MS_EXTENSION as u16,
                required: false,
                value: self.refs_settle_ms.to_le_bytes().to_vec(),
            });
        }
        if self.status_settle_ms != 0 {
            values.push(Extension {
                tag: crate::schema::git::WATCH_STATUS_SETTLE_MS_EXTENSION as u16,
                required: false,
                value: self.status_settle_ms.to_le_bytes().to_vec(),
            });
        }
        if !self.ref_prefixes.is_empty() {
            let mut value = Vec::new();
            put_len_u16(&mut value, self.ref_prefixes.len())?;
            for prefix in &self.ref_prefixes {
                put_bytes_u16(&mut value, prefix)?;
            }
            values.push(Extension {
                tag: crate::schema::git::WATCH_REF_PREFIXES_EXTENSION as u16,
                required: false,
                value,
            });
        }
        Ok(Extensions(values))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required(
            extensions,
            &[
                crate::schema::git::WATCH_REFS_SETTLE_MS_EXTENSION as u16,
                crate::schema::git::WATCH_STATUS_SETTLE_MS_EXTENSION as u16,
                crate::schema::git::WATCH_REF_PREFIXES_EXTENSION as u16,
            ],
        )?;
        let settle = |tag: u64| -> Result<u16> {
            let Some(extension) = extensions
                .0
                .iter()
                .find(|extension| extension.tag == tag as u16)
            else {
                return Ok(0);
            };
            extension
                .value
                .as_slice()
                .try_into()
                .map(u16::from_le_bytes)
                .map_err(|_| Error::Invalid("Git settle extension"))
        };
        let mut ref_prefixes = Vec::new();
        if let Some(extension) = extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::git::WATCH_REF_PREFIXES_EXTENSION as u16
        }) {
            let mut decoder = Decoder::new(&extension.value);
            let count = usize::from(decoder.u16()?);
            if count > crate::schema::git::MAX_REF_PREFIXES as usize
                || count > decoder.remaining() / 2
            {
                return Err(Error::Invalid("Git ref prefix count"));
            }
            ref_prefixes.reserve(count);
            for _ in 0..count {
                ref_prefixes.push(decoder.len_bytes_u16()?.to_vec());
            }
            decoder.finish()?;
        }
        let value = Self {
            refs_settle_ms: settle(crate::schema::git::WATCH_REFS_SETTLE_MS_EXTENSION)?,
            status_settle_ms: settle(crate::schema::git::WATCH_STATUS_SETTLE_MS_EXTENSION)?,
            ref_prefixes,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Watch {
    pub repository_handle: u64,
    pub datasets: u16,
    pub state: StateWatch,
}

impl Encode for Watch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.repository_handle, "zero Git repository handle")?;
        if self.datasets == 0 || self.datasets & !(crate::schema::git::WATCH_DATASETS as u16) != 0 {
            return Err(Error::Invalid("Git WATCH datasets"));
        }
        WatchOptions::from_extensions(&self.state.extensions)?;
        put_u64(out, self.repository_handle);
        put_u16(out, self.datasets);
        put_u16(out, 0);
        put_bytes_u32(out, &self.state.encode()?)
    }
}

impl Watch {
    pub fn options(&self) -> Result<WatchOptions> {
        WatchOptions::from_extensions(&self.state.extensions)
    }
}

impl Decode for Watch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let repository_handle = decoder.u64()?;
        let datasets = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git WATCH reserved field"));
        }
        let value = Self {
            repository_handle,
            datasets,
            state: StateWatch::decode(decoder.len_bytes_u32()?)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

fn validate_spec(spec: &[u8]) -> Result<()> {
    if spec.is_empty() || spec.len() > MAX_SPEC_BYTES || spec.contains(&0) {
        Err(Error::Invalid("Git revision specification"))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryEndpoint {
    Empty,
    Commit(ObjectId),
    Tree(ObjectId),
    Index,
    Worktree,
    MergeBase(ObjectId),
}

impl QueryEndpoint {
    fn kind(&self) -> u8 {
        match self {
            Self::Empty => crate::schema::git::ENDPOINT_EMPTY as u8,
            Self::Commit(_) => crate::schema::git::ENDPOINT_COMMIT as u8,
            Self::Tree(_) => crate::schema::git::ENDPOINT_TREE as u8,
            Self::Index => crate::schema::git::ENDPOINT_INDEX as u8,
            Self::Worktree => crate::schema::git::ENDPOINT_WORKTREE as u8,
            Self::MergeBase(_) => crate::schema::git::ENDPOINT_MERGE_BASE as u8,
        }
    }

    fn object(&self) -> Option<&ObjectId> {
        match self {
            Self::Commit(value) | Self::Tree(value) | Self::MergeBase(value) => Some(value),
            Self::Empty | Self::Index | Self::Worktree => None,
        }
    }

    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        out.push(self.kind());
        out.extend_from_slice(&[0; 3]);
        out.push(u8::from(self.object().is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(object) = self.object() {
            object.encode_into(out)?;
        }
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git endpoint reserved bytes"));
        }
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git endpoint presence or reserved bytes"));
        }
        let object = if present != 0 {
            Some(ObjectId::decode_from(decoder)?)
        } else {
            None
        };
        match (kind, object) {
            (value, None) if value == crate::schema::git::ENDPOINT_EMPTY as u8 => Ok(Self::Empty),
            (value, Some(object)) if value == crate::schema::git::ENDPOINT_COMMIT as u8 => {
                Ok(Self::Commit(object))
            }
            (value, Some(object)) if value == crate::schema::git::ENDPOINT_TREE as u8 => {
                Ok(Self::Tree(object))
            }
            (value, None) if value == crate::schema::git::ENDPOINT_INDEX as u8 => Ok(Self::Index),
            (value, None) if value == crate::schema::git::ENDPOINT_WORKTREE as u8 => {
                Ok(Self::Worktree)
            }
            (value, Some(object)) if value == crate::schema::git::ENDPOINT_MERGE_BASE as u8 => {
                Ok(Self::MergeBase(object))
            }
            _ => Err(Error::Invalid("Git endpoint kind or object presence")),
        }
    }
}

impl Encode for QueryEndpoint {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_into(out)
    }
}

impl Decode for QueryEndpoint {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum QueryCursor {
    #[default]
    Start,
    LogFrontier(Vec<ObjectId>),
    Path(FsPath),
    PlatformPath(Vec<u8>),
    Patch {
        path: FsPath,
        position: u64,
    },
    Position(u64),
}

impl Encode for QueryCursor {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Start => return Ok(()),
            Self::LogFrontier(objects) => {
                if objects.is_empty()
                    || objects.len() > crate::schema::git::MAX_QUERY_ENDPOINTS as usize
                {
                    return Err(Error::Invalid("Git log frontier count"));
                }
                out.push(crate::schema::git::CURSOR_LOG_FRONTIER as u8);
                out.extend_from_slice(&[0; 3]);
                put_len_u16(out, objects.len())?;
                put_u16(out, 0);
                let mut seen = BTreeSet::new();
                for object in objects {
                    object.validate()?;
                    if !seen.insert(object) {
                        return Err(Error::Invalid("duplicate Git log frontier object"));
                    }
                    object.encode_into(out)?;
                }
            }
            Self::Path(path) => {
                if path.components.is_empty() {
                    return Err(Error::Invalid("empty Git path cursor"));
                }
                out.push(crate::schema::git::CURSOR_PATH as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, &path.encode()?)?;
            }
            Self::PlatformPath(path) => {
                validate_platform_path(path)?;
                out.push(crate::schema::git::CURSOR_PLATFORM_PATH as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, path)?;
            }
            Self::Patch { path, position } => {
                if path.components.is_empty() {
                    return Err(Error::Invalid("empty Git patch cursor path"));
                }
                out.push(crate::schema::git::CURSOR_PATCH as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, &path.encode()?)?;
                put_u64(out, *position);
            }
            Self::Position(position) => {
                out.push(crate::schema::git::CURSOR_POSITION as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *position);
            }
        }
        Ok(())
    }
}

impl Decode for QueryCursor {
    fn decode(input: &[u8]) -> Result<Self> {
        if input.is_empty() {
            return Ok(Self::Start);
        }
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git query cursor reserved bytes"));
        }
        let value = match kind {
            value if value == crate::schema::git::CURSOR_LOG_FRONTIER as u8 => {
                let count = usize::from(decoder.u16()?);
                if decoder.u16()? != 0
                    || count == 0
                    || count > crate::schema::git::MAX_QUERY_ENDPOINTS as usize
                    || count > decoder.remaining() / 24
                {
                    return Err(Error::Invalid("Git log frontier count"));
                }
                let mut objects = Vec::with_capacity(count);
                for _ in 0..count {
                    objects.push(ObjectId::decode_from(&mut decoder)?);
                }
                Self::LogFrontier(objects)
            }
            value if value == crate::schema::git::CURSOR_PATH as u8 => {
                Self::Path(FsPath::decode(decoder.len_bytes_u32()?)?)
            }
            value if value == crate::schema::git::CURSOR_PLATFORM_PATH as u8 => {
                Self::PlatformPath(decoder.len_bytes_u32()?.to_vec())
            }
            value if value == crate::schema::git::CURSOR_PATCH as u8 => Self::Patch {
                path: FsPath::decode(decoder.len_bytes_u32()?)?,
                position: decoder.u64()?,
            },
            value if value == crate::schema::git::CURSOR_POSITION as u8 => {
                Self::Position(decoder.u64()?)
            }
            _ => return Err(Error::Invalid("Git query cursor kind")),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryBody {
    Resolve {
        spec: Vec<u8>,
    },
    MergeBase {
        objects: Vec<ObjectId>,
    },
    Log {
        spec: Vec<u8>,
        tips: Vec<ObjectId>,
        hides: Vec<ObjectId>,
        path: Option<FsPath>,
        flags: u16,
    },
    Tree {
        tree: ObjectId,
        path: FsPath,
    },
    Blob {
        object: ObjectId,
        path: Option<FsPath>,
        offset: u64,
        max_bytes: u32,
        flags: u16,
    },
    Diff {
        left: QueryEndpoint,
        right: QueryEndpoint,
        path: Option<FsPath>,
        rename_threshold: u8,
        flags: u16,
    },
    Patch {
        left: QueryEndpoint,
        right: QueryEndpoint,
        path: Option<FsPath>,
        context_lines: u8,
        rename_threshold: u8,
        max_bytes: u32,
        flags: u16,
    },
    Index {
        path: Option<FsPath>,
        flags: u16,
    },
    Discover {
        source: RepositorySource,
        max_depth: u16,
        flags: u16,
    },
    Blame {
        object: ObjectId,
        path: FsPath,
        start_line: u32,
        line_count: u32,
        flags: u16,
    },
    Reflog {
        name: Vec<u8>,
        flags: u16,
    },
    Worktrees,
}

impl QueryBody {
    fn accepts_cursor(&self, cursor: &QueryCursor) -> bool {
        matches!(cursor, QueryCursor::Start)
            || matches!(
                (self, cursor),
                (Self::Log { .. }, QueryCursor::LogFrontier(_))
                    | (
                        Self::Tree { .. } | Self::Diff { .. } | Self::Index { .. },
                        QueryCursor::Path(_)
                    )
                    | (Self::Patch { .. }, QueryCursor::Patch { .. })
                    | (Self::Discover { .. }, QueryCursor::PlatformPath(_))
                    | (
                        Self::Blame { .. } | Self::Reflog { .. } | Self::Worktrees,
                        QueryCursor::Position(_)
                    )
            )
    }

    fn encode_body(&self) -> Result<(u16, Vec<u8>)> {
        let mut out = Vec::new();
        let kind = match self {
            Self::Resolve { spec } => {
                validate_spec(spec)?;
                put_bytes_u16(&mut out, spec)?;
                crate::schema::git::QUERY_RESOLVE
            }
            Self::MergeBase { objects } => {
                if !(2..=crate::schema::git::MAX_QUERY_ENDPOINTS as usize).contains(&objects.len())
                {
                    return Err(Error::Invalid("Git MERGE_BASE object count"));
                }
                put_len_u16(&mut out, objects.len())?;
                put_u16(&mut out, 0);
                for object in objects {
                    object.encode_into(&mut out)?;
                }
                crate::schema::git::QUERY_MERGE_BASE
            }
            Self::Log {
                spec,
                tips,
                hides,
                path,
                flags,
            } => {
                if *flags & !(crate::schema::git::LOG_FLAGS as u16) != 0 {
                    return Err(Error::Invalid("Git LOG flags"));
                }
                if tips.len() > crate::schema::git::MAX_QUERY_ENDPOINTS as usize
                    || hides.len() > crate::schema::git::MAX_QUERY_ENDPOINTS as usize
                    || (!spec.is_empty() && (!tips.is_empty() || !hides.is_empty()))
                    || (spec.is_empty() && tips.is_empty() && !hides.is_empty())
                {
                    return Err(Error::Invalid("Git LOG seed endpoints"));
                }
                if !spec.is_empty() {
                    validate_spec(spec)?;
                }
                put_u16(&mut out, *flags);
                put_u16(&mut out, 0);
                put_bytes_u16(&mut out, spec)?;
                put_len_u16(&mut out, tips.len())?;
                put_len_u16(&mut out, hides.len())?;
                for object in tips.iter().chain(hides) {
                    object.encode_into(&mut out)?;
                }
                optional_path(&mut out, path)?;
                crate::schema::git::QUERY_LOG
            }
            Self::Tree { tree, path } => {
                tree.encode_into(&mut out)?;
                put_bytes_u32(&mut out, &path.encode()?)?;
                crate::schema::git::QUERY_TREE
            }
            Self::Blob {
                object,
                path,
                offset,
                max_bytes,
                flags,
            } => {
                if *flags & !(crate::schema::git::BLOB_FLAGS as u16) != 0 {
                    return Err(Error::Invalid("Git BLOB flags"));
                }
                put_u16(&mut out, *flags);
                put_u16(&mut out, 0);
                object.encode_into(&mut out)?;
                optional_path(&mut out, path)?;
                put_u64(&mut out, *offset);
                put_u32(&mut out, *max_bytes);
                crate::schema::git::QUERY_BLOB
            }
            Self::Diff {
                left,
                right,
                path,
                rename_threshold,
                flags,
            } => {
                if *flags & !(crate::schema::git::DIFF_FLAGS as u16) != 0
                    || *rename_threshold > crate::schema::git::RENAME_THRESHOLD_MAX as u8
                    || matches!(right, QueryEndpoint::MergeBase(_))
                    || matches!((left, right), (QueryEndpoint::Empty, QueryEndpoint::Empty))
                {
                    return Err(Error::Invalid("Git DIFF flags"));
                }
                put_u16(&mut out, *flags);
                out.push(*rename_threshold);
                out.push(0);
                left.encode_into(&mut out)?;
                right.encode_into(&mut out)?;
                optional_path(&mut out, path)?;
                crate::schema::git::QUERY_DIFF
            }
            Self::Patch {
                left,
                right,
                path,
                context_lines,
                rename_threshold,
                max_bytes,
                flags,
            } => {
                if *flags & !(crate::schema::git::PATCH_FLAGS as u16) != 0
                    || *rename_threshold > crate::schema::git::RENAME_THRESHOLD_MAX as u8
                    || matches!(right, QueryEndpoint::MergeBase(_))
                    || matches!((left, right), (QueryEndpoint::Empty, QueryEndpoint::Empty))
                {
                    return Err(Error::Invalid("Git PATCH flags"));
                }
                put_u16(&mut out, *flags);
                out.push(*context_lines);
                out.push(*rename_threshold);
                put_u32(&mut out, *max_bytes);
                left.encode_into(&mut out)?;
                right.encode_into(&mut out)?;
                optional_path(&mut out, path)?;
                crate::schema::git::QUERY_PATCH
            }
            Self::Index { path, flags } => {
                if *flags & !(crate::schema::git::INDEX_FLAGS as u16) != 0 {
                    return Err(Error::Invalid("Git INDEX flags"));
                }
                put_u16(&mut out, *flags);
                put_u16(&mut out, 0);
                optional_path(&mut out, path)?;
                crate::schema::git::QUERY_INDEX
            }
            Self::Discover {
                source,
                max_depth,
                flags,
            } => {
                if *flags & !(crate::schema::git::DISCOVER_QUERY_FLAGS as u16) != 0 {
                    return Err(Error::Invalid("Git DISCOVER flags"));
                }
                put_u16(&mut out, *flags);
                put_u16(&mut out, *max_depth);
                put_bytes_u32(&mut out, &source.encode()?)?;
                crate::schema::git::QUERY_DISCOVER
            }
            Self::Blame {
                object,
                path,
                start_line,
                line_count,
                flags,
            } => {
                if *start_line == 0 || *flags & !(crate::schema::git::BLAME_FLAGS as u16) != 0 {
                    return Err(Error::Invalid("Git BLAME line range or flags"));
                }
                put_u16(&mut out, *flags);
                put_u16(&mut out, 0);
                object.encode_into(&mut out)?;
                put_bytes_u32(&mut out, &path.encode()?)?;
                put_u32(&mut out, *start_line);
                put_u32(&mut out, *line_count);
                crate::schema::git::QUERY_BLAME
            }
            Self::Reflog { name, flags } => {
                if *flags & !(crate::schema::git::REFLOG_FLAGS as u16) != 0 {
                    return Err(Error::Invalid("Git REFLOG flags"));
                }
                if !name.is_empty() {
                    validate_spec(name)?;
                }
                put_u16(&mut out, *flags);
                put_u16(&mut out, 0);
                put_bytes_u16(&mut out, name)?;
                crate::schema::git::QUERY_REFLOG
            }
            Self::Worktrees => crate::schema::git::QUERY_WORKTREES,
        };
        Ok((kind as u16, out))
    }

    fn decode_kind(kind: u16, body: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(body);
        let value = match kind {
            value if value == crate::schema::git::QUERY_RESOLVE as u16 => Self::Resolve {
                spec: decoder.len_bytes_u16()?.to_vec(),
            },
            value if value == crate::schema::git::QUERY_MERGE_BASE as u16 => {
                let count = usize::from(decoder.u16()?);
                if decoder.u16()? != 0
                    || !(2..=crate::schema::git::MAX_QUERY_ENDPOINTS as usize).contains(&count)
                    || count > decoder.remaining() / 24
                {
                    return Err(Error::Invalid(
                        "Git MERGE_BASE object count or reserved field",
                    ));
                }
                let mut objects = Vec::with_capacity(count);
                for _ in 0..count {
                    objects.push(ObjectId::decode_from(&mut decoder)?);
                }
                Self::MergeBase { objects }
            }
            value if value == crate::schema::git::QUERY_LOG as u16 => {
                let flags = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Git LOG reserved field"));
                }
                let spec = decoder.len_bytes_u16()?.to_vec();
                let tip_count = usize::from(decoder.u16()?);
                let hide_count = usize::from(decoder.u16()?);
                if tip_count > crate::schema::git::MAX_QUERY_ENDPOINTS as usize
                    || hide_count > crate::schema::git::MAX_QUERY_ENDPOINTS as usize
                    || tip_count.saturating_add(hide_count) > decoder.remaining() / 24
                {
                    return Err(Error::Invalid("Git LOG endpoint count"));
                }
                let mut tips = Vec::with_capacity(tip_count);
                let mut hides = Vec::with_capacity(hide_count);
                for _ in 0..tip_count {
                    tips.push(ObjectId::decode_from(&mut decoder)?);
                }
                for _ in 0..hide_count {
                    hides.push(ObjectId::decode_from(&mut decoder)?);
                }
                let path = decoder.len_bytes_u32()?;
                Self::Log {
                    spec,
                    tips,
                    hides,
                    path: if path.is_empty() {
                        None
                    } else {
                        Some(FsPath::decode(path)?)
                    },
                    flags,
                }
            }
            value if value == crate::schema::git::QUERY_TREE as u16 => Self::Tree {
                tree: ObjectId::decode_from(&mut decoder)?,
                path: FsPath::decode(decoder.len_bytes_u32()?)?,
            },
            value if value == crate::schema::git::QUERY_BLOB as u16 => {
                let flags = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Git BLOB reserved field"));
                }
                Self::Blob {
                    object: ObjectId::decode_from(&mut decoder)?,
                    path: decode_optional_path(&mut decoder)?,
                    offset: decoder.u64()?,
                    max_bytes: decoder.u32()?,
                    flags,
                }
            }
            value if value == crate::schema::git::QUERY_DIFF as u16 => {
                let flags = decoder.u16()?;
                let rename_threshold = decoder.u8()?;
                if decoder.u8()? != 0 {
                    return Err(Error::Invalid("Git DIFF reserved field"));
                }
                let left = QueryEndpoint::decode_from(&mut decoder)?;
                let right = QueryEndpoint::decode_from(&mut decoder)?;
                let path = decoder.len_bytes_u32()?;
                Self::Diff {
                    left,
                    right,
                    path: if path.is_empty() {
                        None
                    } else {
                        Some(FsPath::decode(path)?)
                    },
                    rename_threshold,
                    flags,
                }
            }
            value if value == crate::schema::git::QUERY_PATCH as u16 => {
                let flags = decoder.u16()?;
                let context_lines = decoder.u8()?;
                let rename_threshold = decoder.u8()?;
                let max_bytes = decoder.u32()?;
                Self::Patch {
                    left: QueryEndpoint::decode_from(&mut decoder)?,
                    right: QueryEndpoint::decode_from(&mut decoder)?,
                    path: decode_optional_path(&mut decoder)?,
                    context_lines,
                    rename_threshold,
                    max_bytes,
                    flags,
                }
            }
            value if value == crate::schema::git::QUERY_INDEX as u16 => {
                let flags = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Git INDEX reserved field"));
                }
                Self::Index {
                    path: decode_optional_path(&mut decoder)?,
                    flags,
                }
            }
            value if value == crate::schema::git::QUERY_DISCOVER as u16 => Self::Discover {
                flags: decoder.u16()?,
                max_depth: decoder.u16()?,
                source: RepositorySource::decode(decoder.len_bytes_u32()?)?,
            },
            value if value == crate::schema::git::QUERY_BLAME as u16 => {
                let flags = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Git BLAME reserved field"));
                }
                Self::Blame {
                    object: ObjectId::decode_from(&mut decoder)?,
                    path: FsPath::decode(decoder.len_bytes_u32()?)?,
                    start_line: decoder.u32()?,
                    line_count: decoder.u32()?,
                    flags,
                }
            }
            value if value == crate::schema::git::QUERY_REFLOG as u16 => {
                let flags = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Git REFLOG reserved field"));
                }
                Self::Reflog {
                    name: decoder.len_bytes_u16()?.to_vec(),
                    flags,
                }
            }
            value if value == crate::schema::git::QUERY_WORKTREES as u16 => Self::Worktrees,
            _ => return Err(Error::Invalid("Git query kind")),
        };
        decoder.finish()?;
        let (encoded_kind, _) = value.encode_body()?;
        if encoded_kind != kind {
            return Err(Error::Invalid("Git query kind mismatch"));
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
            return Err(Error::Invalid("Git query flags"));
        }
        Self::decode_kind(kind, decoder.rest())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub repository_handle: u64,
    pub max_records: u16,
    pub cursor: QueryCursor,
    pub initial_receive_credit: u64,
    pub body: QueryBody,
    pub extensions: Extensions,
}

impl Encode for Query {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let is_discover = matches!(self.body, QueryBody::Discover { .. });
        if is_discover != (self.repository_handle == 0) {
            return Err(Error::Invalid("Git QUERY repository scope"));
        }
        if !is_discover {
            handle(self.repository_handle, "zero Git repository handle")?;
        }
        if !self.body.accepts_cursor(&self.cursor) {
            return Err(Error::Invalid("Git query cursor kind"));
        }
        let cursor = self.cursor.encode()?;
        if cursor.len() > MAX_CURSOR_BYTES {
            return Err(limit(
                "Git query cursor bytes",
                cursor.len() as u64,
                MAX_CURSOR_BYTES as u64,
            ));
        }
        if usize::from(self.max_records) > MAX_QUERY_RECORDS {
            return Err(limit(
                "Git query records",
                u64::from(self.max_records),
                MAX_QUERY_RECORDS as u64,
            ));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.repository_handle);
        put_u16(out, self.max_records);
        put_u16(out, 0);
        put_bytes_u16(out, &cursor)?;
        put_u64(out, self.initial_receive_credit);
        put_bytes_u32(out, &self.body.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Query {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let repository_handle = decoder.u64()?;
        let max_records = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git QUERY reserved field"));
        }
        let cursor = QueryCursor::decode(decoder.len_bytes_u16()?)?;
        let initial_receive_credit = decoder.u64()?;
        let body = QueryBody::decode(decoder.len_bytes_u32()?)?;
        let value = Self {
            repository_handle,
            max_records,
            cursor,
            initial_receive_credit,
            body,
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
            return Err(Error::Invalid("Git typed record flags"));
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
            return Err(limit(
                "Git query message bytes",
                out.len() as u64,
                MAX_QUERY_BYTES as u64,
            ));
        }
        Ok(out)
    }

    /// Decode one complete MESSAGE item from a query Transfer.
    pub fn decode_message(input: &[u8]) -> Result<Self> {
        if input.len() > MAX_QUERY_BYTES {
            return Err(limit(
                "Git query message bytes",
                input.len() as u64,
                MAX_QUERY_BYTES as u64,
            ));
        }
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        QueryRecord::decode_typed(&value)?;
        Ok(value)
    }
}

fn optional_path(out: &mut Vec<u8>, path: &Option<FsPath>) -> Result<()> {
    match path {
        Some(path) => put_bytes_u32(out, &path.encode()?),
        None => put_bytes_u32(out, &[]),
    }
}

fn decode_optional_path(decoder: &mut Decoder<'_>) -> Result<Option<FsPath>> {
    let bytes = decoder.len_bytes_u32()?;
    if bytes.is_empty() {
        Ok(None)
    } else {
        FsPath::decode(bytes).map(Some)
    }
}

fn validate_platform_path(path: &[u8]) -> Result<()> {
    if path.is_empty()
        || path.len() > crate::schema::git::MAX_PATH_BYTES as usize
        || path.contains(&0)
    {
        return Err(Error::Invalid("Git platform path"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommitRecord {
    pub flags: u16,
    pub object: ObjectId,
    pub tree: ObjectId,
    pub parents: Vec<ObjectId>,
    pub authored_unix_seconds: i64,
    pub author_timezone_minutes: i16,
    pub committed_unix_seconds: i64,
    pub committer_timezone_minutes: i16,
    pub author_name: Vec<u8>,
    pub author_email: Vec<u8>,
    pub committer_name: Vec<u8>,
    pub committer_email: Vec<u8>,
    pub message: Vec<u8>,
}

impl Encode for CommitRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.flags & !(crate::schema::git::COMMIT_FLAGS as u16) != 0
            || self.parents.len() > crate::schema::git::MAX_COMMIT_PARENTS as usize
            || self.author_name.len() > crate::schema::git::MAX_IDENTITY_BYTES as usize
            || self.author_email.len() > crate::schema::git::MAX_IDENTITY_BYTES as usize
            || self.committer_name.len() > crate::schema::git::MAX_IDENTITY_BYTES as usize
            || self.committer_email.len() > crate::schema::git::MAX_IDENTITY_BYTES as usize
            || self.message.len() > crate::schema::git::MAX_MESSAGE_BYTES as usize
        {
            return Err(Error::Invalid("Git commit record limits"));
        }
        self.object.validate()?;
        self.tree.validate()?;
        if self.tree.algorithm != self.object.algorithm
            || self
                .parents
                .iter()
                .any(|parent| parent.algorithm != self.object.algorithm)
        {
            return Err(Error::Invalid("Git commit object algorithm"));
        }
        put_u16(out, self.flags);
        put_u16(out, 0);
        self.object.encode_into(out)?;
        self.tree.encode_into(out)?;
        put_len_u16(out, self.parents.len())?;
        put_u16(out, 0);
        for parent in &self.parents {
            parent.encode_into(out)?;
        }
        put_i64(out, self.authored_unix_seconds);
        put_i16(out, self.author_timezone_minutes);
        put_i64(out, self.committed_unix_seconds);
        put_i16(out, self.committer_timezone_minutes);
        put_bytes_u16(out, &self.author_name)?;
        put_bytes_u16(out, &self.author_email)?;
        put_bytes_u16(out, &self.committer_name)?;
        put_bytes_u16(out, &self.committer_email)?;
        put_bytes_u32(out, &self.message)
    }
}

impl Decode for CommitRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git commit reserved field"));
        }
        let object = ObjectId::decode_from(&mut decoder)?;
        let tree = ObjectId::decode_from(&mut decoder)?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count > crate::schema::git::MAX_COMMIT_PARENTS as usize
            || count > decoder.remaining() / 24
        {
            return Err(Error::Invalid("Git commit parent count"));
        }
        let mut parents = Vec::with_capacity(count);
        for _ in 0..count {
            parents.push(ObjectId::decode_from(&mut decoder)?);
        }
        let value = Self {
            flags,
            object,
            tree,
            parents,
            authored_unix_seconds: decoder.i64()?,
            author_timezone_minutes: decoder.i16()?,
            committed_unix_seconds: decoder.i64()?,
            committer_timezone_minutes: decoder.i16()?,
            author_name: decoder.len_bytes_u16()?.to_vec(),
            author_email: decoder.len_bytes_u16()?.to_vec(),
            committer_name: decoder.len_bytes_u16()?.to_vec(),
            committer_email: decoder.len_bytes_u16()?.to_vec(),
            message: decoder.len_bytes_u32()?.to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogPathRecord {
    pub entry_kind: u8,
    pub mode: u32,
    pub object: Option<ObjectId>,
    pub path: FsPath,
}

impl Encode for LogPathRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.entry_kind > crate::schema::git::TREE_COMMIT as u8
            || self.path.components.is_empty()
            || (self.object.is_none()
                && (self.entry_kind != crate::schema::git::TREE_BLOB as u8 || self.mode != 0))
        {
            return Err(Error::Invalid("Git LOG path record"));
        }
        let path = self.path.encode()?;
        out.push(self.entry_kind);
        out.push(u8::from(self.object.is_some()));
        put_u16(out, 0);
        put_u32(out, self.mode);
        if let Some(object) = &self.object {
            object.encode_into(out)?;
        }
        put_bytes_u32(out, &path)
    }
}

impl Decode for LogPathRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let entry_kind = decoder.u8()?;
        let object_present = decoder.u8()?;
        if object_present > 1 || decoder.u16()? != 0 {
            return Err(Error::Invalid("Git LOG path presence or reserved field"));
        }
        let value = Self {
            entry_kind,
            mode: decoder.u32()?,
            object: if object_present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
            } else {
                None
            },
            path: FsPath::decode(decoder.len_bytes_u32()?)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEntryRecord {
    pub entry_kind: u8,
    pub mode: u32,
    pub name: Vec<u8>,
    pub object: ObjectId,
}

impl Encode for TreeEntryRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.entry_kind > crate::schema::git::TREE_COMMIT as u8 {
            return Err(Error::Invalid("Git tree entry kind"));
        }
        FsPath {
            components: vec![self.name.clone()],
        }
        .encode()?;
        out.push(self.entry_kind);
        out.extend_from_slice(&[0; 3]);
        put_u32(out, self.mode);
        put_bytes_u16(out, &self.name)?;
        self.object.encode_into(out)
    }
}

impl Decode for TreeEntryRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let entry_kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git tree entry reserved bytes"));
        }
        let value = Self {
            entry_kind,
            mode: decoder.u32()?,
            name: decoder.len_bytes_u16()?.to_vec(),
            object: ObjectId::decode_from(&mut decoder)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ContentDelivery {
    Inline(Vec<u8>),
    Transfer(Descriptor),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentRecord {
    pub object: ObjectId,
    pub byte_len: u64,
    pub offset: u64,
    pub next_offset: u64,
    pub delivery: ContentDelivery,
}

impl ContentRecord {
    pub fn encode_blob(&self) -> Result<Vec<u8>> {
        self.encode_for(crate::schema::git::BLOB_CONTENT_KIND as u16)
    }

    pub fn decode_blob(input: &[u8]) -> Result<Self> {
        Self::decode_for(input, crate::schema::git::BLOB_CONTENT_KIND as u16)
    }

    pub fn encode_patch(&self) -> Result<Vec<u8>> {
        self.encode_for(crate::schema::git::PATCH_CONTENT_KIND as u16)
    }

    pub fn decode_patch(input: &[u8]) -> Result<Self> {
        Self::decode_for(input, crate::schema::git::PATCH_CONTENT_KIND as u16)
    }

    fn validate(&self, content_kind: u16) -> Result<()> {
        self.object.validate()?;
        if self.offset > self.next_offset || self.next_offset > self.byte_len {
            return Err(Error::Invalid("Git content window"));
        }
        let delivered = self.next_offset - self.offset;
        match &self.delivery {
            ContentDelivery::Inline(bytes)
                if bytes.len() <= crate::schema::git::MAX_INLINE_BYTES as usize
                    && bytes.len() as u64 == delivered => {}
            ContentDelivery::Inline(_) => return Err(Error::Invalid("Git inline content")),
            ContentDelivery::Transfer(descriptor) => {
                descriptor.validate()?;
                if descriptor.mode != Mode::Byte
                    || descriptor.direction != Direction::SENDER_TO_RECEIVER
                    || descriptor.content_family != crate::family::GIT
                    || descriptor.content_kind != content_kind
                    || descriptor.content_version != VERSION
                    || !descriptor.sensitive_content()?
                {
                    return Err(Error::Invalid("Git content Transfer descriptor"));
                }
            }
        }
        Ok(())
    }

    fn encode_for(&self, content_kind: u16) -> Result<Vec<u8>> {
        self.validate(content_kind)?;
        let mut out = Vec::new();
        self.object.encode_into(&mut out)?;
        put_u64(&mut out, self.byte_len);
        put_u64(&mut out, self.offset);
        put_u64(&mut out, self.next_offset);
        match &self.delivery {
            ContentDelivery::Inline(bytes) => {
                out.push(crate::schema::git::CONTENT_INLINE as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(&mut out, bytes)?;
            }
            ContentDelivery::Transfer(descriptor) => {
                out.push(crate::schema::git::CONTENT_TRANSFER as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(&mut out, &descriptor.encode()?)?;
            }
        }
        Ok(out)
    }

    fn decode_for(input: &[u8], content_kind: u16) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let object = ObjectId::decode_from(&mut decoder)?;
        let byte_len = decoder.u64()?;
        let offset = decoder.u64()?;
        let next_offset = decoder.u64()?;
        let delivery = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git content reserved bytes"));
        }
        let delivery = match delivery {
            value if value == crate::schema::git::CONTENT_INLINE as u8 => {
                ContentDelivery::Inline(decoder.len_bytes_u32()?.to_vec())
            }
            value if value == crate::schema::git::CONTENT_TRANSFER as u8 => {
                ContentDelivery::Transfer(Descriptor::decode(decoder.len_bytes_u32()?)?)
            }
            _ => return Err(Error::Invalid("Git content delivery")),
        };
        decoder.finish()?;
        let value = Self {
            object,
            byte_len,
            offset,
            next_offset,
            delivery,
        };
        value.validate(content_kind)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffRecord {
    pub status: u8,
    pub similarity_percent: u8,
    pub flags: u16,
    pub old_path: Option<FsPath>,
    pub new_path: Option<FsPath>,
    pub old_mode: u32,
    pub new_mode: u32,
    pub old_object: Option<ObjectId>,
    pub new_object: Option<ObjectId>,
}

impl Encode for DiffRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.status > crate::schema::git::DIFF_COPIED as u8
            || self.similarity_percent > 100
            || self.flags & !(crate::schema::git::DIFF_RECORD_FLAGS as u16) != 0
            || self.old_path.is_none() && self.new_path.is_none()
        {
            return Err(Error::Invalid("Git diff record"));
        }
        out.push(self.status);
        out.push(self.similarity_percent);
        put_u16(out, self.flags);
        optional_path(out, &self.old_path)?;
        optional_path(out, &self.new_path)?;
        put_u32(out, self.old_mode);
        put_u32(out, self.new_mode);
        out.push(u8::from(self.old_object.is_some()));
        out.push(u8::from(self.new_object.is_some()));
        put_u16(out, 0);
        if let Some(object) = &self.old_object {
            object.encode_into(out)?;
        }
        if let Some(object) = &self.new_object {
            object.encode_into(out)?;
        }
        Ok(())
    }
}

impl Decode for DiffRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let status = decoder.u8()?;
        let similarity_percent = decoder.u8()?;
        let flags = decoder.u16()?;
        let old_path = decode_optional_path(&mut decoder)?;
        let new_path = decode_optional_path(&mut decoder)?;
        let old_mode = decoder.u32()?;
        let new_mode = decoder.u32()?;
        let old_present = decoder.u8()?;
        let new_present = decoder.u8()?;
        if old_present > 1 || new_present > 1 || decoder.u16()? != 0 {
            return Err(Error::Invalid("Git diff object presence"));
        }
        let value = Self {
            status,
            similarity_percent,
            flags,
            old_path,
            new_path,
            old_mode,
            new_mode,
            old_object: if old_present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
            } else {
                None
            },
            new_object: if new_present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
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
pub struct PatchFileRecord {
    pub status: u8,
    pub similarity_percent: u8,
    pub flags: u16,
    pub old_path: Option<FsPath>,
    pub new_path: Option<FsPath>,
}

impl PatchFileRecord {
    fn validate(&self) -> Result<()> {
        let paths_valid = match self.status {
            value if value == crate::schema::git::DIFF_ADDED as u8 => {
                self.old_path.is_none() && self.new_path.is_some()
            }
            value if value == crate::schema::git::DIFF_DELETED as u8 => {
                self.old_path.is_some() && self.new_path.is_none()
            }
            value
                if value == crate::schema::git::DIFF_MODIFIED as u8
                    || value == crate::schema::git::DIFF_RENAMED as u8
                    || value == crate::schema::git::DIFF_COPIED as u8 =>
            {
                self.old_path.is_some() && self.new_path.is_some()
            }
            _ => false,
        };
        if !paths_valid
            || self.similarity_percent > 100
            || self.flags & !(crate::schema::git::PATCH_FILE_FLAGS as u16) != 0
            || self
                .old_path
                .iter()
                .chain(self.new_path.iter())
                .any(|path| path.components.is_empty() || path.encode().is_err())
        {
            return Err(Error::Invalid("Git patch file record"));
        }
        Ok(())
    }
}

impl Encode for PatchFileRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.status);
        out.push(self.similarity_percent);
        put_u16(out, self.flags);
        optional_path(out, &self.old_path)?;
        optional_path(out, &self.new_path)
    }
}

impl Decode for PatchFileRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            status: decoder.u8()?,
            similarity_percent: decoder.u8()?,
            flags: decoder.u16()?,
            old_path: decode_optional_path(&mut decoder)?,
            new_path: decode_optional_path(&mut decoder)?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchSpan {
    pub start: u32,
    pub length: u32,
}

fn validate_patch_spans(spans: &[PatchSpan], text_len: usize) -> Result<()> {
    if spans.len() > crate::schema::git::MAX_PATCH_SPANS as usize {
        return Err(limit(
            "Git patch spans",
            spans.len() as u64,
            crate::schema::git::MAX_PATCH_SPANS,
        ));
    }
    let mut previous_end = 0u64;
    for span in spans {
        let start = u64::from(span.start);
        let end = start
            .checked_add(u64::from(span.length))
            .ok_or(Error::LengthOverflow)?;
        if span.length == 0 || start < previous_end || end > text_len as u64 {
            return Err(Error::Invalid("Git patch span"));
        }
        previous_end = end;
    }
    Ok(())
}

fn put_patch_spans(out: &mut Vec<u8>, spans: &[PatchSpan]) -> Result<()> {
    put_len_u16(out, spans.len())?;
    put_u16(out, 0);
    for span in spans {
        put_u32(out, span.start);
        put_u32(out, span.length);
    }
    Ok(())
}

fn decode_patch_spans(decoder: &mut Decoder<'_>) -> Result<Vec<PatchSpan>> {
    let count = usize::from(decoder.u16()?);
    if decoder.u16()? != 0
        || count > crate::schema::git::MAX_PATCH_SPANS as usize
        || count > decoder.remaining() / 8
    {
        return Err(Error::Invalid("Git patch span count"));
    }
    let mut spans = Vec::with_capacity(count);
    for _ in 0..count {
        spans.push(PatchSpan {
            start: decoder.u32()?,
            length: decoder.u32()?,
        });
    }
    Ok(spans)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchRowRecord {
    pub old_line: u32,
    pub new_line: u32,
    pub old_text: Vec<u8>,
    pub new_text: Vec<u8>,
    pub old_spans: Vec<PatchSpan>,
    pub new_spans: Vec<PatchSpan>,
}

impl PatchRowRecord {
    fn validate(&self) -> Result<()> {
        let bytes = self
            .old_text
            .len()
            .checked_add(self.new_text.len())
            .ok_or(Error::LengthOverflow)?;
        if self.old_line == 0 && self.new_line == 0
            || bytes > MAX_QUERY_BYTES
            || (self.old_line == 0 && (!self.old_text.is_empty() || !self.old_spans.is_empty()))
            || (self.new_line == 0 && (!self.new_text.is_empty() || !self.new_spans.is_empty()))
        {
            return Err(Error::Invalid("Git patch row record"));
        }
        validate_patch_spans(&self.old_spans, self.old_text.len())?;
        validate_patch_spans(&self.new_spans, self.new_text.len())
    }
}

impl Encode for PatchRowRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.old_line);
        put_u32(out, self.new_line);
        put_bytes_u32(out, &self.old_text)?;
        put_bytes_u32(out, &self.new_text)?;
        put_patch_spans(out, &self.old_spans)?;
        put_patch_spans(out, &self.new_spans)
    }
}

impl Decode for PatchRowRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let old_line = decoder.u32()?;
        let new_line = decoder.u32()?;
        let old_text = decoder.len_bytes_u32()?.to_vec();
        let new_text = decoder.len_bytes_u32()?.to_vec();
        let value = Self {
            old_line,
            new_line,
            old_spans: decode_patch_spans(&mut decoder)?,
            new_spans: decode_patch_spans(&mut decoder)?,
            old_text,
            new_text,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchGapRecord {
    pub old_line: u32,
    pub new_line: u32,
}

impl Encode for PatchGapRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.old_line == 0 && self.new_line == 0 {
            return Err(Error::Invalid("Git patch gap record"));
        }
        put_u32(out, self.old_line);
        put_u32(out, self.new_line);
        Ok(())
    }
}

impl Decode for PatchGapRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            old_line: decoder.u32()?,
            new_line: decoder.u32()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchBaseRecord {
    pub object: ObjectId,
}

impl Encode for PatchBaseRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.object.encode_into(out)
    }
}

impl Decode for PatchBaseRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            object: ObjectId::decode_from(&mut decoder)?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexEntryRecord {
    pub stage: u8,
    pub status: u8,
    pub flags: u16,
    pub path: FsPath,
    pub mode: u32,
    pub size: u64,
    pub modified_unix_ns: i64,
    pub object: ObjectId,
}

impl Encode for IndexEntryRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.stage > 3
            || self.status > crate::schema::git::INDEX_STATUS_DELETED as u8
            || self.flags & !(crate::schema::git::INDEX_ENTRY_FLAGS as u16) != 0
            || self.path.components.is_empty()
        {
            return Err(Error::Invalid("Git index entry"));
        }
        out.push(self.stage);
        out.push(self.status);
        put_u16(out, self.flags);
        put_bytes_u32(out, &self.path.encode()?)?;
        put_u32(out, self.mode);
        put_u64(out, self.size);
        put_i64(out, self.modified_unix_ns);
        self.object.encode_into(out)
    }
}

impl Decode for IndexEntryRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let stage = decoder.u8()?;
        let status = decoder.u8()?;
        let flags = decoder.u16()?;
        let path = FsPath::decode(decoder.len_bytes_u32()?)?;
        let mode = decoder.u32()?;
        let value = Self {
            stage,
            status,
            flags,
            path,
            mode,
            size: decoder.u64()?,
            modified_unix_ns: decoder.i64()?,
            object: ObjectId::decode_from(&mut decoder)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryRecord {
    pub flags: u16,
    pub object_algorithm: u8,
    pub worktree_path: Vec<u8>,
    pub git_dir: Vec<u8>,
}

impl Encode for DiscoveryRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let bare = self.flags & crate::schema::git::DISCOVERY_BARE as u16 != 0;
        if self.flags & !(crate::schema::git::DISCOVERY_FLAGS as u16) != 0
            || !matches!(
                self.object_algorithm,
                value if value == crate::schema::git::OBJECT_SHA1 as u8
                    || value == crate::schema::git::OBJECT_SHA256 as u8
            )
            || bare != self.worktree_path.is_empty()
        {
            return Err(Error::Invalid("Git discovery record"));
        }
        if !bare {
            validate_platform_path(&self.worktree_path)?;
        }
        validate_platform_path(&self.git_dir)?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        out.push(self.object_algorithm);
        out.extend_from_slice(&[0; 3]);
        put_bytes_u32(out, &self.worktree_path)?;
        put_bytes_u32(out, &self.git_dir)
    }
}

impl Decode for DiscoveryRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git discovery reserved field"));
        }
        let object_algorithm = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git discovery reserved bytes"));
        }
        let value = Self {
            flags,
            object_algorithm,
            worktree_path: decoder.len_bytes_u32()?.to_vec(),
            git_dir: decoder.len_bytes_u32()?.to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlameRecord {
    pub flags: u16,
    pub start_line: u32,
    pub end_line: u32,
    pub original_start_line: u32,
    pub commit: ObjectId,
    pub original_path: Option<FsPath>,
    pub author: Vec<u8>,
    pub summary: Vec<u8>,
}

impl Encode for BlameRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.flags & !(crate::schema::git::BLAME_RECORD_FLAGS as u16) != 0
            || self.start_line == 0
            || self.start_line >= self.end_line
            || self.original_start_line == 0
            || self.author.len() > crate::schema::git::MAX_IDENTITY_BYTES as usize
            || self.summary.len() > crate::schema::git::MAX_SUMMARY_BYTES as usize
        {
            return Err(Error::Invalid("Git blame record"));
        }
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_u32(out, self.start_line);
        put_u32(out, self.end_line);
        put_u32(out, self.original_start_line);
        self.commit.encode_into(out)?;
        optional_path(out, &self.original_path)?;
        put_bytes_u16(out, &self.author)?;
        put_bytes_u16(out, &self.summary)
    }
}

impl Decode for BlameRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git blame reserved field"));
        }
        let value = Self {
            flags,
            start_line: decoder.u32()?,
            end_line: decoder.u32()?,
            original_start_line: decoder.u32()?,
            commit: ObjectId::decode_from(&mut decoder)?,
            original_path: decode_optional_path(&mut decoder)?,
            author: decoder.len_bytes_u16()?.to_vec(),
            summary: decoder.len_bytes_u16()?.to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReflogRecord {
    pub flags: u16,
    pub index: u64,
    pub old_object: ObjectId,
    pub new_object: ObjectId,
    pub committer: Vec<u8>,
    pub committed_unix_seconds: i64,
    pub timezone_minutes: i16,
    pub message: Vec<u8>,
}

impl Encode for ReflogRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.flags & !(crate::schema::git::REFLOG_RECORD_FLAGS as u16) != 0
            || self.committer.len() > crate::schema::git::MAX_IDENTITY_BYTES as usize
            || self.message.len() > crate::schema::git::MAX_MESSAGE_BYTES as usize
        {
            return Err(Error::Invalid("Git reflog record limits"));
        }
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_u64(out, self.index);
        self.old_object.encode_into(out)?;
        self.new_object.encode_into(out)?;
        put_bytes_u16(out, &self.committer)?;
        put_i64(out, self.committed_unix_seconds);
        put_i16(out, self.timezone_minutes);
        put_u16(out, 0);
        put_bytes_u32(out, &self.message)
    }
}

impl Decode for ReflogRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git reflog reserved field"));
        }
        let value = Self {
            flags,
            index: decoder.u64()?,
            old_object: ObjectId::decode_from(&mut decoder)?,
            new_object: ObjectId::decode_from(&mut decoder)?,
            committer: decoder.len_bytes_u16()?.to_vec(),
            committed_unix_seconds: decoder.i64()?,
            timezone_minutes: decoder.i16()?,
            message: {
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Git reflog reserved timezone field"));
                }
                decoder.len_bytes_u32()?.to_vec()
            },
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeRecord {
    pub flags: u16,
    pub path: Vec<u8>,
    pub head: Option<ObjectId>,
    pub branch: Vec<u8>,
    pub lock_reason: String,
}

impl Encode for WorktreeRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let bare = self.flags & crate::schema::git::WORKTREE_BARE as u16 != 0;
        let detached = self.flags & crate::schema::git::WORKTREE_DETACHED as u16 != 0;
        let locked = self.flags & crate::schema::git::WORKTREE_LOCKED as u16 != 0;
        if self.flags & !(crate::schema::git::WORKTREE_FLAGS as u16) != 0
            || bare != self.path.is_empty()
            || (bare || detached) != self.branch.is_empty()
            || (!locked && !self.lock_reason.is_empty())
            || self.branch.len() > MAX_SPEC_BYTES
            || self.branch.contains(&0)
            || self.lock_reason.len() > crate::schema::git::MAX_SUMMARY_BYTES as usize
            || self.lock_reason.as_bytes().contains(&0)
        {
            return Err(Error::Invalid("Git worktree flags"));
        }
        if !bare {
            validate_platform_path(&self.path)?;
        }
        if let Some(head) = &self.head {
            head.validate()?;
        }
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_bytes_u32(out, &self.path)?;
        out.push(u8::from(self.head.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(head) = &self.head {
            head.encode_into(out)?;
        }
        put_bytes_u16(out, &self.branch)?;
        put_string_u16(out, &self.lock_reason)
    }
}

impl Decode for WorktreeRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git worktree reserved field"));
        }
        let path = decoder.len_bytes_u32()?.to_vec();
        let head_present = decoder.u8()?;
        if head_present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid(
                "Git worktree head presence or reserved bytes",
            ));
        }
        let value = Self {
            flags,
            path,
            head: if head_present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
            } else {
                None
            },
            branch: decoder.len_bytes_u16()?.to_vec(),
            lock_reason: decoder.string_u16()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryRecord {
    Object(ObjectRecord),
    Commit(CommitRecord),
    LogPath(LogPathRecord),
    TreeEntry(TreeEntryRecord),
    Blob(ContentRecord),
    Diff(DiffRecord),
    PatchContent(ContentRecord),
    PatchFile(PatchFileRecord),
    PatchRow(PatchRowRecord),
    PatchGap(PatchGapRecord),
    PatchBase(PatchBaseRecord),
    IndexEntry(IndexEntryRecord),
    Discovery(DiscoveryRecord),
    Blame(BlameRecord),
    Reflog(ReflogRecord),
    Worktree(WorktreeRecord),
}

impl QueryRecord {
    pub fn encode_typed(&self) -> Result<TypedRecord> {
        let (kind, body) = match self {
            Self::Object(value) => (crate::schema::git::RESULT_OBJECT, value.encode()?),
            Self::Commit(value) => (crate::schema::git::RESULT_COMMIT, value.encode()?),
            Self::LogPath(value) => (crate::schema::git::RESULT_LOG_PATH, value.encode()?),
            Self::TreeEntry(value) => (crate::schema::git::RESULT_TREE_ENTRY, value.encode()?),
            Self::Blob(value) => (
                crate::schema::git::RESULT_BLOB,
                value.encode_for(crate::schema::git::BLOB_CONTENT_KIND as u16)?,
            ),
            Self::Diff(value) => (crate::schema::git::RESULT_DIFF, value.encode()?),
            Self::PatchContent(value) => (
                crate::schema::git::RESULT_PATCH,
                value.encode_for(crate::schema::git::PATCH_CONTENT_KIND as u16)?,
            ),
            Self::PatchFile(value) => (crate::schema::git::RESULT_PATCH_FILE, value.encode()?),
            Self::PatchRow(value) => (crate::schema::git::RESULT_PATCH_ROW, value.encode()?),
            Self::PatchGap(value) => (crate::schema::git::RESULT_PATCH_GAP, value.encode()?),
            Self::PatchBase(value) => (crate::schema::git::RESULT_PATCH_BASE, value.encode()?),
            Self::IndexEntry(value) => (crate::schema::git::RESULT_INDEX_ENTRY, value.encode()?),
            Self::Discovery(value) => (crate::schema::git::RESULT_DISCOVERY, value.encode()?),
            Self::Blame(value) => (crate::schema::git::RESULT_BLAME, value.encode()?),
            Self::Reflog(value) => (crate::schema::git::RESULT_REFLOG, value.encode()?),
            Self::Worktree(value) => (crate::schema::git::RESULT_WORKTREE, value.encode()?),
        };
        Ok(TypedRecord {
            kind: kind as u16,
            required: false,
            body,
        })
    }

    pub fn decode_typed(value: &TypedRecord) -> Result<Option<Self>> {
        let decoded = match value.kind {
            kind if kind == crate::schema::git::RESULT_OBJECT as u16 => {
                Self::Object(ObjectRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_COMMIT as u16 => {
                Self::Commit(CommitRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_LOG_PATH as u16 => {
                Self::LogPath(LogPathRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_TREE_ENTRY as u16 => {
                Self::TreeEntry(TreeEntryRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_BLOB as u16 => {
                Self::Blob(ContentRecord::decode_for(
                    &value.body,
                    crate::schema::git::BLOB_CONTENT_KIND as u16,
                )?)
            }
            kind if kind == crate::schema::git::RESULT_DIFF as u16 => {
                Self::Diff(DiffRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_PATCH as u16 => {
                Self::PatchContent(ContentRecord::decode_for(
                    &value.body,
                    crate::schema::git::PATCH_CONTENT_KIND as u16,
                )?)
            }
            kind if kind == crate::schema::git::RESULT_PATCH_FILE as u16 => {
                Self::PatchFile(PatchFileRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_PATCH_ROW as u16 => {
                Self::PatchRow(PatchRowRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_PATCH_GAP as u16 => {
                Self::PatchGap(PatchGapRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_PATCH_BASE as u16 => {
                Self::PatchBase(PatchBaseRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_INDEX_ENTRY as u16 => {
                Self::IndexEntry(IndexEntryRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_DISCOVERY as u16 => {
                Self::Discovery(DiscoveryRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_BLAME as u16 => {
                Self::Blame(BlameRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_REFLOG as u16 => {
                Self::Reflog(ReflogRecord::decode(&value.body)?)
            }
            kind if kind == crate::schema::git::RESULT_WORKTREE as u16 => {
                Self::Worktree(WorktreeRecord::decode(&value.body)?)
            }
            _ if !value.required => return Ok(None),
            _ => return Err(Error::Invalid("unknown required Git query record")),
        };
        Ok(Some(decoded))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageDelivery {
    Inline(Vec<TypedRecord>),
    Transfer(Descriptor),
}

fn validate_page_descriptor(descriptor: &Descriptor) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Message
        || descriptor.direction != Direction::SENDER_TO_RECEIVER
        || descriptor.content_family != crate::family::GIT
        || descriptor.content_kind != crate::schema::git::QUERY_CONTENT_KIND as u16
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("Git query Transfer descriptor"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryPage {
    pub next_cursor: QueryCursor,
    pub total_hint: u64,
    pub flags: u16,
    pub delivery: PageDelivery,
    pub extensions: Extensions,
}

impl QueryPage {
    fn validate(&self) -> Result<()> {
        let has_more = !matches!(self.next_cursor, QueryCursor::Start);
        if self.next_cursor.encode()?.len() > MAX_CURSOR_BYTES
            || self.flags & !(crate::schema::git::QUERY_PAGE_FLAGS as u16) != 0
            || (self.flags & crate::schema::git::QUERY_PAGE_MORE as u16 != 0) != has_more
        {
            return Err(Error::Invalid("Git page cursor"));
        }
        match &self.delivery {
            PageDelivery::Inline(records) => {
                if records.len() > MAX_QUERY_RECORDS {
                    return Err(limit(
                        "Git query records",
                        records.len() as u64,
                        MAX_QUERY_RECORDS as u64,
                    ));
                }
                let mut bytes = Vec::new();
                for record in records {
                    QueryRecord::decode_typed(record)?;
                    record.encode_into(&mut bytes)?;
                }
                if bytes.len() > MAX_QUERY_BYTES {
                    return Err(limit(
                        "Git query bytes",
                        bytes.len() as u64,
                        MAX_QUERY_BYTES as u64,
                    ));
                }
            }
            PageDelivery::Transfer(descriptor) => validate_page_descriptor(descriptor)?,
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for QueryPage {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_bytes_u16(out, &self.next_cursor.encode()?)?;
        put_u64(out, self.total_hint);
        put_u16(out, self.flags);
        put_u16(out, 0);
        match &self.delivery {
            PageDelivery::Inline(records) => {
                out.push(crate::schema::git::PAGE_INLINE as u8);
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
                out.push(crate::schema::git::PAGE_TRANSFER as u8);
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
        let next_cursor = QueryCursor::decode(decoder.len_bytes_u16()?)?;
        let total_hint = decoder.u64()?;
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git page reserved field"));
        }
        let delivery = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git page delivery reserved bytes"));
        }
        let delivery = match delivery {
            value if value == crate::schema::git::PAGE_INLINE as u8 => {
                let count = usize::from(decoder.u16()?);
                if decoder.u16()? != 0 || count > MAX_QUERY_RECORDS {
                    return Err(Error::Invalid("Git page record count"));
                }
                let bytes = decoder.len_bytes_u32()?;
                if count > bytes.len() / 8 {
                    return Err(Error::Invalid("Git page record count"));
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
            value if value == crate::schema::git::PAGE_TRANSFER as u8 => {
                PageDelivery::Transfer(Descriptor::decode(decoder.len_bytes_u32()?)?)
            }
            _ => return Err(Error::Invalid("Git page delivery")),
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
pub struct WatchQuery {
    pub repository_handle: u64,
    pub max_records: u16,
    pub body: QueryBody,
    pub state: StateWatch,
}

impl Encode for WatchQuery {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.repository_handle, "zero Git repository handle")?;
        if usize::from(self.max_records) > MAX_QUERY_RECORDS {
            return Err(limit(
                "Git watched query records",
                u64::from(self.max_records),
                MAX_QUERY_RECORDS as u64,
            ));
        }
        put_u64(out, self.repository_handle);
        put_u16(out, self.max_records);
        put_u16(out, 0);
        put_bytes_u32(out, &self.body.encode()?)?;
        put_bytes_u32(out, &self.state.encode()?)
    }
}

impl Decode for WatchQuery {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let repository_handle = decoder.u64()?;
        let max_records = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git WATCH_QUERY reserved field"));
        }
        let value = Self {
            repository_handle,
            max_records,
            body: QueryBody::decode(decoder.len_bytes_u32()?)?,
            state: StateWatch::decode(decoder.len_bytes_u32()?)?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryState {
    pub query_subscription_id: u32,
    pub event: StateEvent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchedQueryValue {
    pub status: crate::core::Status,
    pub detail: String,
    pub page: Option<QueryPage>,
}

impl WatchedQueryValue {
    fn validate(&self) -> Result<()> {
        if self.status.is_ok() {
            if !self.detail.is_empty() || self.page.is_none() {
                return Err(Error::Invalid("Git watched query OK value"));
            }
        } else if self.detail.is_empty() || self.page.is_some() {
            return Err(Error::Invalid("Git watched query failure value"));
        }
        if let Some(page) = &self.page {
            page.validate()?;
            if !matches!(page.delivery, PageDelivery::Inline(_)) {
                return Err(Error::Invalid("Git watched query Transfer delivery"));
            }
        }
        Ok(())
    }
}

impl Encode for WatchedQueryValue {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.status.code());
        put_u16(out, 0);
        put_string_u32(out, &self.detail)?;
        match &self.page {
            Some(page) => put_bytes_u32(out, &page.encode()?)?,
            None => put_u32(out, 0),
        }
        Ok(())
    }
}

impl Decode for WatchedQueryValue {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let status = crate::core::Status::from_code(decoder.u16()?);
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git watched query reserved field"));
        }
        let detail = decoder.string_u32()?;
        let page = decoder.len_bytes_u32()?;
        let value = Self {
            status,
            detail,
            page: if page.is_empty() {
                None
            } else {
                Some(QueryPage::decode(page)?)
            },
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

impl QueryState {
    fn validate(&self) -> Result<()> {
        if self.query_subscription_id == 0
            || self.query_subscription_id != self.event.subscription_id
        {
            return Err(Error::Invalid("Git query subscription ID mismatch"));
        }
        self.event.validate()?;
        if self.event.flags != 0 {
            return Err(Error::Invalid("Git QUERY_STATE flags"));
        }
        self.value().map(|_| ())
    }

    pub fn value(&self) -> Result<Option<WatchedQueryValue>> {
        let (expected_kind, record) = match self.event.phase {
            crate::state::Phase::SnapshotRecords => {
                (RecordKind::Add, self.event.records.as_slice())
            }
            crate::state::Phase::Delta => (RecordKind::Replace, self.event.records.as_slice()),
            crate::state::Phase::SnapshotBegin
            | crate::state::Phase::SnapshotEnd
            | crate::state::Phase::Reset => return Ok(None),
        };
        let [record] = record else {
            return Err(Error::Invalid("Git QUERY_STATE page count"));
        };
        if record.kind != expected_kind || record.required {
            return Err(Error::Invalid("Git QUERY_STATE record kind or flags"));
        }
        Ok(Some(WatchedQueryValue::decode(&record.body)?))
    }

    pub fn page(&self) -> Result<Option<QueryPage>> {
        Ok(self.value()?.and_then(|value| value.page))
    }
}

impl Encode for QueryState {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.query_subscription_id);
        put_bytes_u32(out, &self.event.encode()?)
    }
}

impl Decode for QueryState {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            query_subscription_id: decoder.u32()?,
            event: StateEvent::decode(decoder.len_bytes_u32()?)?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetch {
    pub repository_handle: u64,
    pub operation_id: [u8; 16],
    pub flags: u16,
    pub timeout_ms: u32,
    pub remote: Vec<u8>,
    pub refspecs: Vec<Vec<u8>>,
    pub extensions: Extensions,
}

impl Encode for Fetch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.repository_handle, "zero Git repository handle")?;
        operation_id(&self.operation_id)?;
        if self.flags & !(crate::schema::git::FETCH_FLAGS as u16) != 0
            || self.remote.len() > crate::schema::git::MAX_REMOTE_BYTES as usize
            || self.remote.contains(&0)
            || self.refspecs.len() > crate::schema::git::MAX_REFSPECS as usize
        {
            return Err(Error::Invalid("Git FETCH metadata"));
        }
        let mut unique = BTreeSet::new();
        for refspec in &self.refspecs {
            if refspec.is_empty()
                || refspec.len() > MAX_SPEC_BYTES
                || refspec.contains(&0)
                || !unique.insert(refspec)
            {
                return Err(Error::Invalid("Git FETCH refspec"));
            }
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.repository_handle);
        out.extend_from_slice(&self.operation_id);
        put_u16(out, self.flags);
        put_len_u16(out, self.refspecs.len())?;
        put_u32(out, self.timeout_ms);
        put_bytes_u16(out, &self.remote)?;
        for refspec in &self.refspecs {
            put_bytes_u16(out, refspec)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Fetch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let repository_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let flags = decoder.u16()?;
        let count = usize::from(decoder.u16()?);
        let timeout_ms = decoder.u32()?;
        let remote = decoder.len_bytes_u16()?.to_vec();
        if count > crate::schema::git::MAX_REFSPECS as usize || count > decoder.remaining() / 2 {
            return Err(Error::Invalid("Git FETCH refspec count"));
        }
        let mut refspecs = Vec::with_capacity(count);
        for _ in 0..count {
            refspecs.push(decoder.len_bytes_u16()?.to_vec());
        }
        let value = Self {
            repository_handle,
            operation_id,
            flags,
            timeout_ms,
            remote,
            refspecs,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchRefResult {
    pub flags: u16,
    pub status: u16,
    pub old: Option<ObjectId>,
    pub new: Option<ObjectId>,
    pub name: Vec<u8>,
    pub detail: String,
}

impl Encode for FetchRefResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let pruned = self.flags & crate::schema::git::FETCH_REF_PRUNED as u16 != 0;
        let new_ref = self.flags & crate::schema::git::FETCH_REF_NEW as u16 != 0;
        if self.flags & !(crate::schema::git::FETCH_REF_FLAGS as u16) != 0
            || (pruned && self.new.is_some())
            || (new_ref && self.old.is_some())
            || matches!(
                crate::core::Status::from_code(self.status),
                crate::core::Status::Unknown(_)
            )
            || self.detail.len() > crate::schema::git::MAX_SUMMARY_BYTES as usize
            || self.detail.as_bytes().contains(&0)
        {
            return Err(Error::Invalid("Git FETCH ref result"));
        }
        validate_spec(&self.name)?;
        if let Some(old) = &self.old {
            old.validate()?;
        }
        if let Some(new) = &self.new {
            new.validate()?;
        }
        put_u16(out, self.flags);
        put_u16(out, self.status);
        out.push(u8::from(self.old.is_some()));
        out.push(u8::from(self.new.is_some()));
        put_u16(out, 0);
        if let Some(old) = &self.old {
            old.encode_into(out)?;
        }
        if let Some(new) = &self.new {
            new.encode_into(out)?;
        }
        put_bytes_u16(out, &self.name)?;
        put_string_u16(out, &self.detail)
    }
}

impl Decode for FetchRefResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        let status = decoder.u16()?;
        let old_present = decoder.u8()?;
        let new_present = decoder.u8()?;
        if old_present > 1 || new_present > 1 || decoder.u16()? != 0 {
            return Err(Error::Invalid("Git FETCH ref presence or reserved field"));
        }
        let value = Self {
            flags,
            status,
            old: if old_present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
            } else {
                None
            },
            new: if new_present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
            } else {
                None
            },
            name: decoder.len_bytes_u16()?.to_vec(),
            detail: decoder.string_u16()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FetchResult {
    pub repository_revision: u64,
    pub refs: Vec<FetchRefResult>,
    pub extensions: Extensions,
}

impl Encode for FetchResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        revision(self.repository_revision, "zero Git repository revision")?;
        if self.refs.len() > crate::schema::git::MAX_REFSPECS as usize {
            return Err(limit(
                "Git FETCH ref results",
                self.refs.len() as u64,
                crate::schema::git::MAX_REFSPECS,
            ));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.repository_revision);
        put_len_u16(out, self.refs.len())?;
        put_u16(out, 0);
        for value in &self.refs {
            put_bytes_u32(out, &value.encode()?)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for FetchResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let repository_revision = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0
            || count > crate::schema::git::MAX_REFSPECS as usize
            || count > decoder.remaining() / 4
        {
            return Err(Error::Invalid("Git FETCH result reserved field"));
        }
        let mut refs = Vec::with_capacity(count);
        for _ in 0..count {
            refs.push(FetchRefResult::decode(decoder.len_bytes_u32()?)?);
        }
        let value = Self {
            repository_revision,
            refs,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub operation_id: [u8; 16],
    pub phase: u8,
    pub flags: u8,
    pub current: u64,
    pub total: u64,
    pub message: String,
}

impl Encode for Progress {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        operation_id(&self.operation_id)?;
        if self.phase > crate::schema::git::PROGRESS_UPDATE_REFS as u8
            || self.flags & !(crate::schema::git::PROGRESS_FLAGS as u8) != 0
            || self.message.len() > crate::schema::git::MAX_PROGRESS_MESSAGE_BYTES as usize
        {
            return Err(Error::Invalid("Git progress"));
        }
        out.extend_from_slice(&self.operation_id);
        out.push(self.phase);
        out.push(self.flags);
        put_u16(out, 0);
        put_u64(out, self.current);
        put_u64(out, self.total);
        put_string_u16(out, &self.message)
    }
}

impl Decode for Progress {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let operation_id = decoder.array_16()?;
        let phase = decoder.u8()?;
        let flags = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git progress reserved field"));
        }
        let value = Self {
            operation_id,
            phase,
            flags,
            current: decoder.u64()?,
            total: decoder.u64()?,
            message: decoder.string_u16()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Closed {
    pub repository_handle: u64,
    pub repository_revision: u64,
    pub reason: u8,
    pub detail: String,
}

impl Encode for Closed {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        handle(self.repository_handle, "zero Git repository handle")?;
        revision(
            self.repository_revision,
            "zero Git CLOSED repository revision",
        )?;
        if self.reason > crate::schema::git::CLOSED_RESOURCE_LIMIT as u8
            || self.detail.len() > crate::schema::git::MAX_SUMMARY_BYTES as usize
            || self.detail.as_bytes().contains(&0)
        {
            return Err(Error::Invalid("Git CLOSED event"));
        }
        put_u64(out, self.repository_handle);
        put_u64(out, self.repository_revision);
        out.push(self.reason);
        out.extend_from_slice(&[0; 3]);
        put_string_u16(out, &self.detail)
    }
}

impl Decode for Closed {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let repository_handle = decoder.u64()?;
        let repository_revision = decoder.u64()?;
        let reason = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git CLOSED reserved bytes"));
        }
        let value = Self {
            repository_handle,
            repository_revision,
            reason,
            detail: decoder.string_u16()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadEntityBody {
    pub flags: u16,
    pub object: Option<ObjectId>,
    pub symbolic_target: Vec<u8>,
}

impl HeadEntityBody {
    fn validate(&self) -> Result<()> {
        if self.flags & !(crate::schema::git::HEAD_FLAGS as u16) != 0
            || self.flags & crate::schema::git::HEAD_FLAGS as u16
                == crate::schema::git::HEAD_FLAGS as u16
            || self.symbolic_target.len() > MAX_SPEC_BYTES
            || self.symbolic_target.contains(&0)
        {
            return Err(Error::Invalid("Git HEAD entity"));
        }
        let detached = self.flags & crate::schema::git::HEAD_DETACHED as u16 != 0;
        let unborn = self.flags & crate::schema::git::HEAD_UNBORN as u16 != 0;
        match (
            detached,
            unborn,
            self.object.is_some(),
            self.symbolic_target.is_empty(),
        ) {
            (true, false, true, true)
            | (false, true, false, false)
            | (false, false, true, false) => Ok(()),
            _ => Err(Error::Invalid("Git HEAD entity state")),
        }
    }
}

impl Encode for HeadEntityBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        out.push(u8::from(self.object.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(object) = &self.object {
            object.encode_into(out)?;
        }
        put_bytes_u16(out, &self.symbolic_target)
    }
}

impl Decode for HeadEntityBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git HEAD entity reserved field"));
        }
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git HEAD entity presence or reserved bytes"));
        }
        let value = Self {
            flags,
            object: if present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
            } else {
                None
            },
            symbolic_target: decoder.len_bytes_u16()?.to_vec(),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefEntityBody {
    pub flags: u16,
    pub object: ObjectId,
    pub peeled: Option<ObjectId>,
    pub symbolic_target: Vec<u8>,
}

impl RefEntityBody {
    fn validate(&self) -> Result<()> {
        if self.flags & !(crate::schema::git::REF_FLAGS as u16) != 0
            || (self.flags & crate::schema::git::REF_PEELED as u16 != 0) != self.peeled.is_some()
            || (self.flags & crate::schema::git::REF_SYMBOLIC as u16 != 0)
                != !self.symbolic_target.is_empty()
            || self.symbolic_target.len() > MAX_SPEC_BYTES
            || self.symbolic_target.contains(&0)
        {
            return Err(Error::Invalid("Git ref entity"));
        }
        self.object.validate()?;
        if let Some(peeled) = &self.peeled {
            peeled.validate()?;
        }
        Ok(())
    }
}

impl Encode for RefEntityBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        self.object.encode_into(out)?;
        out.push(u8::from(self.peeled.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(peeled) = &self.peeled {
            peeled.encode_into(out)?;
        }
        put_bytes_u16(out, &self.symbolic_target)
    }
}

impl Decode for RefEntityBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git ref entity reserved field"));
        }
        let object = ObjectId::decode_from(&mut decoder)?;
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Git ref entity presence or reserved bytes"));
        }
        let value = Self {
            flags,
            object,
            peeled: if present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
            } else {
                None
            },
            symbolic_target: decoder.len_bytes_u16()?.to_vec(),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoteEntityBody {
    pub flags: u16,
    pub fetch_url: Vec<u8>,
    pub push_url: Vec<u8>,
}

impl RemoteEntityBody {
    fn validate(&self) -> Result<()> {
        if self.flags & !(crate::schema::git::REMOTE_FLAGS as u16) != 0
            || self.fetch_url.is_empty()
            || self.fetch_url.len() > crate::schema::git::MAX_REMOTE_BYTES as usize
            || self.push_url.len() > crate::schema::git::MAX_REMOTE_BYTES as usize
            || self.fetch_url.contains(&0)
            || self.push_url.contains(&0)
            || self.fetch_url == self.push_url
        {
            return Err(Error::Invalid("Git remote entity"));
        }
        Ok(())
    }
}

impl Encode for RemoteEntityBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_bytes_u32(out, &self.fetch_url)?;
        put_bytes_u32(out, &self.push_url)
    }
}

impl Decode for RemoteEntityBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git remote entity reserved field"));
        }
        let value = Self {
            flags,
            fetch_url: decoder.len_bytes_u32()?.to_vec(),
            push_url: decoder.len_bytes_u32()?.to_vec(),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperationEntityBody {
    pub operation_kind: u8,
    pub flags: u8,
    pub head: Option<ObjectId>,
    pub detail: String,
}

impl OperationEntityBody {
    fn validate(&self) -> Result<()> {
        if self.operation_kind < crate::schema::git::OPERATION_MERGE as u8
            || self.operation_kind > crate::schema::git::OPERATION_BISECT as u8
            || self.flags & !(crate::schema::git::OPERATION_FLAGS as u8) != 0
            || (self.flags & crate::schema::git::OPERATION_HEAD_PRESENT as u8 != 0)
                != self.head.is_some()
            || self.detail.len() > crate::schema::git::MAX_SUMMARY_BYTES as usize
            || self.detail.as_bytes().contains(&0)
        {
            return Err(Error::Invalid("Git operation entity"));
        }
        if let Some(head) = &self.head {
            head.validate()?;
        }
        Ok(())
    }
}

impl Encode for OperationEntityBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.operation_kind);
        out.push(self.flags);
        put_u16(out, 0);
        out.push(u8::from(self.head.is_some()));
        out.extend_from_slice(&[0; 3]);
        if let Some(head) = &self.head {
            head.encode_into(out)?;
        }
        put_string_u16(out, &self.detail)
    }
}

impl Decode for OperationEntityBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let operation_kind = decoder.u8()?;
        let flags = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git operation entity reserved field"));
        }
        let present = decoder.u8()?;
        if present > 1 || decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid(
                "Git operation entity presence or reserved bytes",
            ));
        }
        let value = Self {
            operation_kind,
            flags,
            head: if present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
            } else {
                None
            },
            detail: decoder.string_u16()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusEntityBody {
    pub index_status: u8,
    pub worktree_status: u8,
    pub flags: u16,
    pub content: Option<ObjectId>,
    pub old_path: Option<FsPath>,
}

impl StatusEntityBody {
    fn validate(&self) -> Result<()> {
        if self.index_status > crate::schema::git::WORKTREE_STATUS_IGNORED as u8
            || self.worktree_status > crate::schema::git::WORKTREE_STATUS_IGNORED as u8
            || self.flags & !(crate::schema::git::STATE_STATUS_FLAGS as u16) != 0
            || (self.flags & crate::schema::git::STATE_STATUS_CONTENT_PRESENT as u16 != 0)
                != self.content.is_some()
            || (self.flags & crate::schema::git::STATE_STATUS_OLD_PATH_PRESENT as u16 != 0)
                != self.old_path.is_some()
        {
            return Err(Error::Invalid("Git status entity"));
        }
        if let Some(content) = &self.content {
            content.validate()?;
        }
        if let Some(path) = &self.old_path {
            if path.components.is_empty() {
                return Err(Error::Invalid("empty Git status old path"));
            }
            path.encode()?;
        }
        Ok(())
    }
}

impl Encode for StatusEntityBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.index_status);
        out.push(self.worktree_status);
        put_u16(out, self.flags);
        out.push(u8::from(self.content.is_some()));
        out.push(u8::from(self.old_path.is_some()));
        put_u16(out, 0);
        if let Some(content) = &self.content {
            content.encode_into(out)?;
        }
        if let Some(path) = &self.old_path {
            put_bytes_u32(out, &path.encode()?)?;
        }
        Ok(())
    }
}

impl Decode for StatusEntityBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let index_status = decoder.u8()?;
        let worktree_status = decoder.u8()?;
        let flags = decoder.u16()?;
        let content_present = decoder.u8()?;
        let old_path_present = decoder.u8()?;
        if content_present > 1 || old_path_present > 1 || decoder.u16()? != 0 {
            return Err(Error::Invalid(
                "Git status entity presence or reserved field",
            ));
        }
        let value = Self {
            index_status,
            worktree_status,
            flags,
            content: if content_present != 0 {
                Some(ObjectId::decode_from(&mut decoder)?)
            } else {
                None
            },
            old_path: if old_path_present != 0 {
                Some(FsPath::decode(decoder.len_bytes_u32()?)?)
            } else {
                None
            },
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamEntityBody {
    pub flags: u16,
    pub ahead: u32,
    pub behind: u32,
    pub upstream: Vec<u8>,
}

impl UpstreamEntityBody {
    fn validate(&self) -> Result<()> {
        let gone = self.flags & crate::schema::git::UPSTREAM_GONE as u16 != 0;
        let counts_valid = self.flags & crate::schema::git::UPSTREAM_COUNTS_VALID as u16 != 0;
        if self.flags & !(crate::schema::git::UPSTREAM_FLAGS as u16) != 0
            || gone && counts_valid
            || !counts_valid && (self.ahead != 0 || self.behind != 0)
        {
            return Err(Error::Invalid("Git upstream entity"));
        }
        validate_spec(&self.upstream)
    }
}

impl Encode for UpstreamEntityBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_u32(out, self.ahead);
        put_u32(out, self.behind);
        put_bytes_u16(out, &self.upstream)
    }
}

impl Decode for UpstreamEntityBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git upstream reserved field"));
        }
        let value = Self {
            flags,
            ahead: decoder.u32()?,
            behind: decoder.u32()?,
            upstream: decoder.len_bytes_u16()?.to_vec(),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StashEntityBody {
    pub object: ObjectId,
    pub created_unix_seconds: i64,
    pub timezone_minutes: i16,
    pub message: Vec<u8>,
}

impl Encode for StashEntityBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.message.len() > crate::schema::git::MAX_MESSAGE_BYTES as usize {
            return Err(limit(
                "Git stash message bytes",
                self.message.len() as u64,
                crate::schema::git::MAX_MESSAGE_BYTES,
            ));
        }
        self.object.encode_into(out)?;
        put_i64(out, self.created_unix_seconds);
        put_i16(out, self.timezone_minutes);
        put_u16(out, 0);
        put_bytes_u32(out, &self.message)
    }
}

impl Decode for StashEntityBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let object = ObjectId::decode_from(&mut decoder)?;
        let created_unix_seconds = decoder.i64()?;
        let timezone_minutes = decoder.i16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git stash reserved field"));
        }
        let value = Self {
            object,
            created_unix_seconds,
            timezone_minutes,
            message: decoder.len_bytes_u32()?.to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorktreeGenerationEntityBody {
    pub count: u32,
    pub digest: u64,
}

impl Encode for WorktreeGenerationEntityBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        put_u32(out, self.count);
        put_u32(out, 0);
        put_u64(out, self.digest);
        Ok(())
    }
}

impl Decode for WorktreeGenerationEntityBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = decoder.u32()?;
        if decoder.u32()? != 0 {
            return Err(Error::Invalid("Git worktree generation reserved field"));
        }
        let value = Self {
            count,
            digest: decoder.u64()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityBody {
    Head(HeadEntityBody),
    Ref(RefEntityBody),
    Remote(RemoteEntityBody),
    Operation(OperationEntityBody),
    Status(StatusEntityBody),
    Upstream(UpstreamEntityBody),
    Stash(StashEntityBody),
    WorktreeGeneration(WorktreeGenerationEntityBody),
}

impl EntityBody {
    pub const fn entity_kind(&self) -> u16 {
        match self {
            Self::Head(_) => crate::schema::git::ENTITY_HEAD as u16,
            Self::Ref(_) => crate::schema::git::ENTITY_REF as u16,
            Self::Remote(_) => crate::schema::git::ENTITY_REMOTE as u16,
            Self::Operation(_) => crate::schema::git::ENTITY_OPERATION as u16,
            Self::Status(_) => crate::schema::git::ENTITY_STATUS as u16,
            Self::Upstream(_) => crate::schema::git::ENTITY_UPSTREAM as u16,
            Self::Stash(_) => crate::schema::git::ENTITY_STASH as u16,
            Self::WorktreeGeneration(_) => crate::schema::git::ENTITY_WORKTREE_GENERATION as u16,
        }
    }

    fn decode_for(entity_kind: u16, input: &[u8]) -> Result<Self> {
        match entity_kind {
            value if value == crate::schema::git::ENTITY_HEAD as u16 => {
                HeadEntityBody::decode(input).map(Self::Head)
            }
            value if value == crate::schema::git::ENTITY_REF as u16 => {
                RefEntityBody::decode(input).map(Self::Ref)
            }
            value if value == crate::schema::git::ENTITY_REMOTE as u16 => {
                RemoteEntityBody::decode(input).map(Self::Remote)
            }
            value if value == crate::schema::git::ENTITY_OPERATION as u16 => {
                OperationEntityBody::decode(input).map(Self::Operation)
            }
            value if value == crate::schema::git::ENTITY_STATUS as u16 => {
                StatusEntityBody::decode(input).map(Self::Status)
            }
            value if value == crate::schema::git::ENTITY_UPSTREAM as u16 => {
                UpstreamEntityBody::decode(input).map(Self::Upstream)
            }
            value if value == crate::schema::git::ENTITY_STASH as u16 => {
                StashEntityBody::decode(input).map(Self::Stash)
            }
            value if value == crate::schema::git::ENTITY_WORKTREE_GENERATION as u16 => {
                WorktreeGenerationEntityBody::decode(input).map(Self::WorktreeGeneration)
            }
            _ => Err(Error::Invalid("Git state entity kind")),
        }
    }
}

impl Encode for EntityBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Head(value) => value.encode_to(out),
            Self::Ref(value) => value.encode_to(out),
            Self::Remote(value) => value.encode_to(out),
            Self::Operation(value) => value.encode_to(out),
            Self::Status(value) => value.encode_to(out),
            Self::Upstream(value) => value.encode_to(out),
            Self::Stash(value) => value.encode_to(out),
            Self::WorktreeGeneration(value) => value.encode_to(out),
        }
    }
}

fn validate_entity_key(entity_kind: u16, key: &[u8]) -> Result<()> {
    if key.len() > MAX_SPEC_BYTES {
        return Err(Error::Invalid("Git state entity key"));
    }
    match entity_kind {
        value
            if value == crate::schema::git::ENTITY_HEAD as u16
                && key == b"HEAD"
                && !key.contains(&0) =>
        {
            Ok(())
        }
        value
            if value == crate::schema::git::ENTITY_OPERATION as u16
                && key == b"operation"
                && !key.contains(&0) =>
        {
            Ok(())
        }
        value
            if (value == crate::schema::git::ENTITY_REF as u16
                || value == crate::schema::git::ENTITY_REMOTE as u16
                || value == crate::schema::git::ENTITY_UPSTREAM as u16)
                && !key.is_empty()
                && !key.contains(&0) =>
        {
            Ok(())
        }
        value if value == crate::schema::git::ENTITY_STATUS as u16 => {
            let path = FsPath::decode(key)?;
            if path.components.is_empty() {
                return Err(Error::Invalid("empty Git status path"));
            }
            Ok(())
        }
        value
            if value == crate::schema::git::ENTITY_STASH as u16
                && key.len() == core::mem::size_of::<u32>() =>
        {
            Ok(())
        }
        value
            if value == crate::schema::git::ENTITY_WORKTREE_GENERATION as u16
                && key == b"worktrees" =>
        {
            Ok(())
        }
        _ => Err(Error::Invalid("Git state entity key")),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityRecord {
    pub entity_kind: u16,
    pub key: Vec<u8>,
    pub revision: u64,
    pub body: EntityBody,
    pub extensions: Extensions,
}

impl EntityRecord {
    fn validate(&self) -> Result<()> {
        if self.entity_kind != self.body.entity_kind() {
            return Err(Error::Invalid("Git state entity"));
        }
        validate_entity_key(self.entity_kind, &self.key)?;
        revision(self.revision, "zero Git entity revision")?;
        reject_unknown_required(&self.extensions, &[])?;
        let body_len = self.body.encode()?.len();
        if body_len > MAX_QUERY_BYTES {
            return Err(limit(
                "Git state entity body",
                body_len as u64,
                MAX_QUERY_BYTES as u64,
            ));
        }
        Ok(())
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Git complete state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for EntityRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.entity_kind);
        put_u16(out, 0);
        put_bytes_u16(out, &self.key)?;
        put_u64(out, self.revision);
        put_bytes_u32(out, &self.body.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for EntityRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let entity_kind = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git entity reserved field"));
        }
        let key = decoder.len_bytes_u16()?.to_vec();
        let revision = decoder.u64()?;
        let body = EntityBody::decode_for(entity_kind, decoder.len_bytes_u32()?)?;
        let value = Self {
            entity_kind,
            key,
            revision,
            body,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityPatch {
    pub entity_kind: u16,
    pub key: Vec<u8>,
    pub observed_revision: u64,
    pub fields: u32,
    pub replacement: EntityRecord,
    pub extensions: Extensions,
}

impl EntityPatch {
    fn validate(&self) -> Result<()> {
        if self.fields == 0
            || self.fields & !(crate::schema::git::ENTITY_PATCH_FIELDS as u32) != 0
            || self.entity_kind != self.replacement.entity_kind
            || self.key != self.replacement.key
            || self.observed_revision >= self.replacement.revision
        {
            return Err(Error::Invalid("Git entity patch"));
        }
        self.replacement.validate()?;
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
        put_bytes_u16(out, &self.key)?;
        put_u64(out, self.observed_revision);
        put_u32(out, self.fields);
        put_bytes_u32(out, &self.replacement.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for EntityPatch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let entity_kind = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git entity patch reserved field"));
        }
        let value = Self {
            entity_kind,
            key: decoder.len_bytes_u16()?.to_vec(),
            observed_revision: decoder.u64()?,
            fields: decoder.u32()?,
            replacement: EntityRecord::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemovedEntity {
    pub entity_kind: u16,
    pub key: Vec<u8>,
    pub revision: u64,
}

impl Encode for RemovedEntity {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_entity_key(self.entity_kind, &self.key)?;
        revision(self.revision, "zero Git entity revision")?;
        put_u16(out, self.entity_kind);
        put_u16(out, 0);
        put_bytes_u16(out, &self.key)?;
        put_u64(out, self.revision);
        Ok(())
    }
}

impl Decode for RemovedEntity {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let entity_kind = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Git remove entity reserved field"));
        }
        let value = Self {
            entity_kind,
            key: decoder.len_bytes_u16()?.to_vec(),
            revision: decoder.u64()?,
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

    fn oid(byte: u8) -> ObjectId {
        ObjectId {
            algorithm: crate::schema::git::OBJECT_SHA1 as u8,
            bytes: vec![byte; 20],
        }
    }

    fn path(components: &[&[u8]]) -> FsPath {
        FsPath {
            components: components.iter().map(|value| value.to_vec()).collect(),
        }
    }

    #[test]
    fn sources_queries_and_fetch_round_trip() {
        round_trip(Open {
            source: RepositorySource::Fs {
                root_handle: 1,
                path: FsPath {
                    components: vec![b"repo".to_vec()],
                },
            },
            extensions: Extensions::default(),
        });
        round_trip(Query {
            repository_handle: 2,
            max_records: 10,
            cursor: QueryCursor::Start,
            initial_receive_credit: 1024,
            body: QueryBody::Diff {
                left: QueryEndpoint::Commit(oid(1)),
                right: QueryEndpoint::Commit(oid(2)),
                path: None,
                rename_threshold: 50,
                flags: crate::schema::git::DIFF_RENAMES as u16,
            },
            extensions: Extensions::default(),
        });
        round_trip(WatchQuery {
            repository_handle: 2,
            max_records: 10,
            body: QueryBody::Log {
                spec: b"refs/heads/main".to_vec(),
                tips: Vec::new(),
                hides: Vec::new(),
                path: None,
                flags: 0,
            },
            state: StateWatch {
                initial_credit: 4096,
                resume: None,
                extensions: Extensions::default(),
            },
        });
        round_trip(QueryBody::Patch {
            left: QueryEndpoint::Commit(oid(3)),
            right: QueryEndpoint::Commit(oid(4)),
            path: Some(FsPath {
                components: vec![b"src".to_vec()],
            }),
            context_lines: 3,
            rename_threshold: 50,
            max_bytes: 4096,
            flags: crate::schema::git::PATCH_TEXT as u16,
        });
        round_trip(Fetch {
            repository_handle: 2,
            operation_id: [3; 16],
            flags: 0,
            timeout_ms: 30_000,
            remote: b"origin".to_vec(),
            refspecs: vec![b"refs/heads/main".to_vec()],
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn sources_watch_options_and_open_result_are_exact() {
        for source in [
            RepositorySource::PlatformPath(b"/repo".to_vec()),
            RepositorySource::Fs {
                root_handle: 1,
                path: path(&[b"repo"]),
            },
            RepositorySource::Submodule {
                parent_repository: 2,
                path: path(&[b"vendor", b"lib"]),
            },
            RepositorySource::TerminalCwd {
                terminal_handle: 3,
                suffix: path(&[b"project"]),
            },
        ] {
            round_trip(source);
        }

        let options = WatchOptions {
            refs_settle_ms: 50,
            status_settle_ms: 500,
            ref_prefixes: vec![b"refs/heads/".to_vec(), b"refs/remotes/".to_vec()],
        };
        let extensions = options.to_extensions().unwrap();
        assert_eq!(WatchOptions::from_extensions(&extensions).unwrap(), options);
        round_trip(Watch {
            repository_handle: 4,
            datasets: (crate::schema::git::WATCH_HEAD
                | crate::schema::git::WATCH_REFS
                | crate::schema::git::WATCH_STATUS) as u16,
            state: StateWatch {
                initial_credit: 4096,
                resume: None,
                extensions,
            },
        });

        round_trip(OpenResult {
            repository_handle: 4,
            repository_revision: 7,
            object_algorithm: crate::schema::git::OBJECT_SHA1 as u8,
            repository_flags: (crate::schema::git::REPOSITORY_WRITABLE
                | crate::schema::git::REPOSITORY_FETCHABLE) as u16,
            canonical_worktree_path: b"/repo".to_vec(),
            canonical_git_dir: b"/repo/.git".to_vec(),
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn every_query_variant_and_cursor_round_trips() {
        for endpoint in [
            QueryEndpoint::Empty,
            QueryEndpoint::Commit(oid(1)),
            QueryEndpoint::Tree(oid(2)),
            QueryEndpoint::Index,
            QueryEndpoint::Worktree,
            QueryEndpoint::MergeBase(oid(3)),
        ] {
            round_trip(endpoint);
        }
        assert_eq!(QueryCursor::decode(&[]).unwrap(), QueryCursor::Start);
        for cursor in [
            QueryCursor::LogFrontier(vec![oid(1), oid(2)]),
            QueryCursor::Path(path(&[b"src", b"lib.rs"])),
            QueryCursor::PlatformPath(b"/repo/nested".to_vec()),
            QueryCursor::Patch {
                path: path(&[b"src", b"lib.rs"]),
                position: 17,
            },
            QueryCursor::Position(9),
        ] {
            let bytes = cursor.encode().unwrap();
            assert_eq!(QueryCursor::decode(&bytes).unwrap(), cursor);
            for end in 1..bytes.len() {
                assert!(
                    QueryCursor::decode(&bytes[..end]).is_err(),
                    "accepted cursor prefix {end}"
                );
            }
        }

        let variants = vec![
            QueryBody::Resolve {
                spec: b"main...topic".to_vec(),
            },
            QueryBody::MergeBase {
                objects: vec![oid(1), oid(2), oid(3)],
            },
            QueryBody::Log {
                spec: Vec::new(),
                tips: vec![oid(1)],
                hides: vec![oid(2)],
                path: Some(path(&[b"src"])),
                flags: (crate::schema::git::LOG_TOPO | crate::schema::git::LOG_FULL_MESSAGE) as u16,
            },
            QueryBody::Tree {
                tree: oid(3),
                path: path(&[]),
            },
            QueryBody::Blob {
                object: oid(4),
                path: Some(path(&[b"README.md"])),
                offset: 128,
                max_bytes: 4096,
                flags: 0,
            },
            QueryBody::Diff {
                left: QueryEndpoint::MergeBase(oid(5)),
                right: QueryEndpoint::Worktree,
                path: Some(path(&[b"src"])),
                rename_threshold: 50,
                flags: (crate::schema::git::DIFF_RENAMES | crate::schema::git::DIFF_UNTRACKED)
                    as u16,
            },
            QueryBody::Patch {
                left: QueryEndpoint::Index,
                right: QueryEndpoint::Worktree,
                path: None,
                context_lines: 3,
                rename_threshold: 0,
                max_bytes: 65_536,
                flags: (crate::schema::git::PATCH_TEXT
                    | crate::schema::git::PATCH_IGNORE_SPACE_CHANGE) as u16,
            },
            QueryBody::Index {
                path: Some(path(&[b"src"])),
                flags: crate::schema::git::INDEX_STAGED as u16,
            },
            QueryBody::Discover {
                source: RepositorySource::PlatformPath(b"/work".to_vec()),
                max_depth: 4,
                flags: (crate::schema::git::DISCOVER_NESTED | crate::schema::git::DISCOVER_BARE)
                    as u16,
            },
            QueryBody::Blame {
                object: oid(6),
                path: path(&[b"src", b"lib.rs"]),
                start_line: 1,
                line_count: 200,
                flags: crate::schema::git::BLAME_FOLLOW_RENAMES as u16,
            },
            QueryBody::Reflog {
                name: b"HEAD".to_vec(),
                flags: crate::schema::git::REFLOG_OLDEST_FIRST as u16,
            },
            QueryBody::Worktrees,
        ];
        for variant in variants {
            round_trip(variant);
        }
    }

    #[test]
    fn merge_base_and_watched_query_bounds_are_enforced() {
        for objects in [Vec::new(), vec![oid(1)]] {
            assert!(QueryBody::MergeBase { objects }.encode().is_err());
        }
        assert!(
            QueryBody::MergeBase {
                objects: (0..=crate::schema::git::MAX_QUERY_ENDPOINTS)
                    .map(|index| oid(index as u8))
                    .collect(),
            }
            .encode()
            .is_err()
        );

        let watched = WatchQuery {
            repository_handle: 1,
            max_records: crate::schema::git::MAX_QUERY_RECORDS as u16 + 1,
            body: QueryBody::Log {
                spec: b"HEAD".to_vec(),
                tips: Vec::new(),
                hides: Vec::new(),
                path: None,
                flags: 0,
            },
            state: StateWatch {
                initial_credit: 1,
                resume: None,
                extensions: Extensions::default(),
            },
        };
        assert!(watched.encode().is_err());
    }

    #[test]
    fn fetch_per_ref_results_round_trip() {
        round_trip(FetchResult {
            repository_revision: 9,
            refs: vec![FetchRefResult {
                flags: crate::schema::git::FETCH_REF_FORCED as u16,
                status: crate::core::Status::Ok.code(),
                old: Some(oid(1)),
                new: Some(oid(2)),
                name: b"refs/remotes/origin/main".to_vec(),
                detail: "forced update".into(),
            }],
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn object_ids_and_state_are_exact() {
        round_trip(oid(7));
        let entity = EntityRecord {
            entity_kind: crate::schema::git::ENTITY_HEAD as u16,
            key: b"HEAD".to_vec(),
            revision: 1,
            body: EntityBody::Head(HeadEntityBody {
                flags: crate::schema::git::HEAD_DETACHED as u16,
                object: Some(oid(7)),
                symbolic_target: Vec::new(),
            }),
            extensions: Extensions::default(),
        };
        round_trip(entity.clone());
        assert_eq!(
            EntityRecord::decode(&entity.state_record(RecordKind::Add).unwrap().body).unwrap(),
            entity
        );
    }

    #[test]
    fn every_state_entity_body_is_typed_and_truncation_safe() {
        let head = HeadEntityBody {
            flags: 0,
            object: Some(oid(1)),
            symbolic_target: b"refs/heads/main".to_vec(),
        };
        round_trip(head.clone());
        let reference = RefEntityBody {
            flags: crate::schema::git::REF_PEELED as u16,
            object: oid(2),
            peeled: Some(oid(3)),
            symbolic_target: Vec::new(),
        };
        round_trip(reference.clone());
        let remote = RemoteEntityBody {
            flags: crate::schema::git::REMOTE_DEFAULT as u16,
            fetch_url: b"ssh://host/repo".to_vec(),
            push_url: Vec::new(),
        };
        round_trip(remote.clone());
        let operation = OperationEntityBody {
            operation_kind: crate::schema::git::OPERATION_REBASE as u8,
            flags: crate::schema::git::OPERATION_HEAD_PRESENT as u8,
            head: Some(oid(4)),
            detail: "onto main".into(),
        };
        round_trip(operation.clone());
        let old_path = FsPath {
            components: vec![b"old".to_vec()],
        };
        let status = StatusEntityBody {
            index_status: crate::schema::git::WORKTREE_STATUS_RENAMED as u8,
            worktree_status: crate::schema::git::WORKTREE_STATUS_MODIFIED as u8,
            flags: (crate::schema::git::STATE_STATUS_CONTENT_PRESENT
                | crate::schema::git::STATE_STATUS_OLD_PATH_PRESENT) as u16,
            content: Some(oid(5)),
            old_path: Some(old_path),
        };
        round_trip(status.clone());
        let upstream = UpstreamEntityBody {
            flags: crate::schema::git::UPSTREAM_COUNTS_VALID as u16,
            ahead: 2,
            behind: 3,
            upstream: b"refs/remotes/origin/main".to_vec(),
        };
        round_trip(upstream.clone());
        let stash = StashEntityBody {
            object: oid(6),
            created_unix_seconds: 7,
            timezone_minutes: 60,
            message: b"WIP on main\0raw".to_vec(),
        };
        round_trip(stash.clone());
        let worktree_generation = WorktreeGenerationEntityBody {
            count: 2,
            digest: 0x1122_3344_5566_7788,
        };
        round_trip(worktree_generation);

        let status_key = FsPath {
            components: vec![b"new".to_vec()],
        }
        .encode()
        .unwrap();
        for value in [
            EntityRecord {
                entity_kind: crate::schema::git::ENTITY_HEAD as u16,
                key: b"HEAD".to_vec(),
                revision: 1,
                body: EntityBody::Head(head),
                extensions: Extensions::default(),
            },
            EntityRecord {
                entity_kind: crate::schema::git::ENTITY_REF as u16,
                key: b"refs/heads/main".to_vec(),
                revision: 2,
                body: EntityBody::Ref(reference),
                extensions: Extensions::default(),
            },
            EntityRecord {
                entity_kind: crate::schema::git::ENTITY_REMOTE as u16,
                key: b"origin".to_vec(),
                revision: 3,
                body: EntityBody::Remote(remote),
                extensions: Extensions::default(),
            },
            EntityRecord {
                entity_kind: crate::schema::git::ENTITY_OPERATION as u16,
                key: b"operation".to_vec(),
                revision: 4,
                body: EntityBody::Operation(operation),
                extensions: Extensions::default(),
            },
            EntityRecord {
                entity_kind: crate::schema::git::ENTITY_STATUS as u16,
                key: status_key,
                revision: 5,
                body: EntityBody::Status(status),
                extensions: Extensions::default(),
            },
            EntityRecord {
                entity_kind: crate::schema::git::ENTITY_UPSTREAM as u16,
                key: b"refs/heads/main".to_vec(),
                revision: 6,
                body: EntityBody::Upstream(upstream),
                extensions: Extensions::default(),
            },
            EntityRecord {
                entity_kind: crate::schema::git::ENTITY_STASH as u16,
                key: 0u32.to_le_bytes().to_vec(),
                revision: 7,
                body: EntityBody::Stash(stash),
                extensions: Extensions::default(),
            },
            EntityRecord {
                entity_kind: crate::schema::git::ENTITY_WORKTREE_GENERATION as u16,
                key: b"worktrees".to_vec(),
                revision: 8,
                body: EntityBody::WorktreeGeneration(worktree_generation),
                extensions: Extensions::default(),
            },
        ] {
            round_trip(value);
        }

        let mismatched = EntityRecord {
            entity_kind: crate::schema::git::ENTITY_REF as u16,
            key: b"refs/heads/main".to_vec(),
            revision: 1,
            body: EntityBody::Head(HeadEntityBody {
                flags: crate::schema::git::HEAD_DETACHED as u16,
                object: Some(oid(9)),
                symbolic_target: Vec::new(),
            }),
            extensions: Extensions::default(),
        };
        assert_eq!(mismatched.encode(), Err(Error::Invalid("Git state entity")));
    }

    #[test]
    fn pages_and_progress_round_trip() {
        let commit = CommitRecord {
            flags: 0,
            object: oid(1),
            tree: oid(3),
            parents: vec![oid(2)],
            authored_unix_seconds: 1,
            author_timezone_minutes: 60,
            committed_unix_seconds: 2,
            committer_timezone_minutes: 60,
            author_name: b"A".to_vec(),
            author_email: b"a@example.invalid".to_vec(),
            committer_name: b"C".to_vec(),
            committer_email: b"c@example.invalid".to_vec(),
            message: b"message\0bytes".to_vec(),
        };
        round_trip(commit.clone());
        round_trip(QueryPage {
            next_cursor: QueryCursor::LogFrontier(vec![oid(1)]),
            total_hint: 1,
            flags: crate::schema::git::QUERY_PAGE_MORE as u16,
            delivery: PageDelivery::Inline(vec![
                QueryRecord::Commit(commit).encode_typed().unwrap(),
            ]),
            extensions: Extensions::default(),
        });
        round_trip(Progress {
            operation_id: [8; 16],
            phase: crate::schema::git::PROGRESS_RECEIVING as u8,
            flags: 0,
            current: 1,
            total: 2,
            message: "objects".into(),
        });
        round_trip(Closed {
            repository_handle: 2,
            repository_revision: 3,
            reason: crate::schema::git::CLOSED_REPOSITORY_GONE as u8,
            detail: "repository disappeared".into(),
        });
    }

    #[test]
    fn query_state_subscription_ids_must_match() {
        let value = QueryState {
            query_subscription_id: 7,
            event: StateEvent {
                subscription_id: 7,
                phase: crate::state::Phase::SnapshotBegin,
                flags: 0,
                from_revision: 0,
                to_revision: 1,
                records: Vec::new(),
            },
        };
        round_trip(value.clone());

        let mut mismatch = value.clone();
        mismatch.event.subscription_id = 8;
        assert!(mismatch.encode().is_err());

        let mut bytes = value.encode().unwrap();
        bytes[8..12].copy_from_slice(&8u32.to_le_bytes());
        assert_eq!(
            QueryState::decode(&bytes),
            Err(Error::Invalid("Git query subscription ID mismatch"))
        );

        let inline_page = QueryPage {
            next_cursor: QueryCursor::LogFrontier(vec![oid(1)]),
            total_hint: 1,
            flags: crate::schema::git::QUERY_PAGE_MORE as u16,
            delivery: PageDelivery::Inline(vec![
                QueryRecord::Object(ObjectRecord {
                    role: crate::schema::git::OBJECT_ROLE_RESULT as u8,
                    object: oid(1),
                })
                .encode_typed()
                .unwrap(),
            ]),
            extensions: Extensions::default(),
        };
        let watched = QueryState {
            query_subscription_id: 7,
            event: StateEvent {
                subscription_id: 7,
                phase: crate::state::Phase::SnapshotRecords,
                flags: 0,
                from_revision: 1,
                to_revision: 1,
                records: vec![Record {
                    kind: RecordKind::Add,
                    required: false,
                    body: WatchedQueryValue {
                        status: crate::core::Status::Ok,
                        detail: String::new(),
                        page: Some(inline_page.clone()),
                    }
                    .encode()
                    .unwrap(),
                }],
            },
        };
        round_trip(watched.clone());
        assert_eq!(watched.page().unwrap(), Some(inline_page));

        let failed = QueryState {
            query_subscription_id: 7,
            event: StateEvent {
                subscription_id: 7,
                phase: crate::state::Phase::Delta,
                flags: 0,
                from_revision: 1,
                to_revision: 2,
                records: vec![Record {
                    kind: RecordKind::Replace,
                    required: false,
                    body: WatchedQueryValue {
                        status: crate::core::Status::NotFound,
                        detail: "ref disappeared".into(),
                        page: None,
                    }
                    .encode()
                    .unwrap(),
                }],
            },
        };
        round_trip(failed.clone());
        assert_eq!(
            failed.value().unwrap(),
            Some(WatchedQueryValue {
                status: crate::core::Status::NotFound,
                detail: "ref disappeared".into(),
                page: None,
            })
        );

        let descriptor = Descriptor {
            transfer_id: 2,
            mode: Mode::Message,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: 4096,
            max_item_bytes: 4096,
            max_chunk_bytes: 1024,
            content_family: crate::family::GIT,
            content_kind: crate::schema::git::QUERY_CONTENT_KIND as u16,
            content_version: VERSION,
            extensions: Extensions(vec![crate::codec::Extension {
                tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        };
        let transferred = WatchedQueryValue {
            status: crate::core::Status::Ok,
            detail: String::new(),
            page: Some(QueryPage {
                next_cursor: QueryCursor::Start,
                total_hint: 0,
                flags: 0,
                delivery: PageDelivery::Transfer(descriptor),
                extensions: Extensions::default(),
            }),
        };
        assert_eq!(
            transferred.encode(),
            Err(Error::Invalid("Git watched query Transfer delivery"))
        );

        assert!(
            WatchedQueryValue {
                status: crate::core::Status::Ok,
                detail: String::new(),
                page: None,
            }
            .encode()
            .is_err()
        );
        assert!(
            WatchedQueryValue {
                status: crate::core::Status::NotFound,
                detail: "missing".into(),
                page: Some(QueryPage {
                    next_cursor: QueryCursor::Start,
                    total_hint: 0,
                    flags: 0,
                    delivery: PageDelivery::Inline(Vec::new()),
                    extensions: Extensions::default(),
                }),
            }
            .encode()
            .is_err()
        );
    }

    #[test]
    fn typed_query_records_round_trip() {
        let values = vec![
            QueryRecord::Object(ObjectRecord {
                role: crate::schema::git::OBJECT_ROLE_TIP as u8,
                object: oid(1),
            }),
            QueryRecord::LogPath(LogPathRecord {
                entry_kind: crate::schema::git::TREE_BLOB as u8,
                mode: 0o100644,
                object: Some(oid(11)),
                path: path(&[b"src", b"lib.rs"]),
            }),
            QueryRecord::TreeEntry(TreeEntryRecord {
                entry_kind: crate::schema::git::TREE_BLOB as u8,
                mode: 0o100644,
                name: b"file".to_vec(),
                object: oid(2),
            }),
            QueryRecord::Blob(ContentRecord {
                object: oid(3),
                byte_len: 3,
                offset: 0,
                next_offset: 3,
                delivery: ContentDelivery::Inline(b"yas".to_vec()),
            }),
            QueryRecord::Diff(DiffRecord {
                status: crate::schema::git::DIFF_RENAMED as u8,
                similarity_percent: 100,
                flags: 0,
                old_path: Some(FsPath {
                    components: vec![b"a".to_vec()],
                }),
                new_path: Some(FsPath {
                    components: vec![b"b".to_vec()],
                }),
                old_mode: 0o100644,
                new_mode: 0o100644,
                old_object: Some(oid(4)),
                new_object: Some(oid(5)),
            }),
            QueryRecord::PatchContent(ContentRecord {
                object: oid(12),
                byte_len: 5,
                offset: 0,
                next_offset: 5,
                delivery: ContentDelivery::Inline(b"patch".to_vec()),
            }),
            QueryRecord::PatchFile(PatchFileRecord {
                status: crate::schema::git::DIFF_MODIFIED as u8,
                similarity_percent: 0,
                flags: 0,
                old_path: Some(path(&[b"src", b"lib.rs"])),
                new_path: Some(path(&[b"src", b"lib.rs"])),
            }),
            QueryRecord::PatchRow(PatchRowRecord {
                old_line: 4,
                new_line: 4,
                old_text: b"old text".to_vec(),
                new_text: b"new text".to_vec(),
                old_spans: vec![PatchSpan {
                    start: 0,
                    length: 3,
                }],
                new_spans: vec![PatchSpan {
                    start: 0,
                    length: 3,
                }],
            }),
            QueryRecord::PatchGap(PatchGapRecord {
                old_line: 9,
                new_line: 9,
            }),
            QueryRecord::PatchBase(PatchBaseRecord { object: oid(13) }),
            QueryRecord::IndexEntry(IndexEntryRecord {
                stage: 0,
                status: crate::schema::git::INDEX_STATUS_MODIFIED as u8,
                flags: 0,
                path: FsPath {
                    components: vec![b"src".to_vec()],
                },
                mode: 0o100644,
                size: 12,
                modified_unix_ns: 34,
                object: oid(6),
            }),
            QueryRecord::Discovery(DiscoveryRecord {
                flags: crate::schema::git::DISCOVERY_LINKED as u16,
                object_algorithm: crate::schema::git::OBJECT_SHA1 as u8,
                worktree_path: b"/repo".to_vec(),
                git_dir: b"/repo/.git".to_vec(),
            }),
            QueryRecord::Blame(BlameRecord {
                flags: 0,
                start_line: 1,
                end_line: 2,
                original_start_line: 4,
                commit: oid(7),
                original_path: Some(FsPath {
                    components: vec![b"file".to_vec()],
                }),
                author: b"author".to_vec(),
                summary: b"summary".to_vec(),
            }),
            QueryRecord::Reflog(ReflogRecord {
                flags: 0,
                index: 1,
                old_object: oid(8),
                new_object: oid(9),
                committer: b"committer".to_vec(),
                committed_unix_seconds: 3,
                timezone_minutes: 60,
                message: b"update".to_vec(),
            }),
            QueryRecord::Worktree(WorktreeRecord {
                flags: crate::schema::git::WORKTREE_MAIN as u16,
                path: b"/repo".to_vec(),
                head: Some(oid(10)),
                branch: b"refs/heads/main".to_vec(),
                lock_reason: String::new(),
            }),
        ];
        for value in values {
            let typed = value.encode_typed().unwrap();
            let message = typed.encode_message().unwrap();
            assert_eq!(TypedRecord::decode_message(&message).unwrap(), typed);
            assert_eq!(QueryRecord::decode_typed(&typed).unwrap(), Some(value));
        }
    }

    #[test]
    fn log_path_and_structured_patch_records_are_truncation_safe() {
        round_trip(LogPathRecord {
            entry_kind: crate::schema::git::TREE_BLOB as u8,
            mode: 0o100644,
            object: Some(oid(1)),
            path: path(&[b"src", b"lib.rs"]),
        });
        round_trip(LogPathRecord {
            entry_kind: crate::schema::git::TREE_BLOB as u8,
            mode: 0,
            object: None,
            path: path(&[b"deleted"]),
        });
        round_trip(PatchFileRecord {
            status: crate::schema::git::DIFF_RENAMED as u8,
            similarity_percent: 90,
            flags: 0,
            old_path: Some(path(&[b"old.rs"])),
            new_path: Some(path(&[b"new.rs"])),
        });
        round_trip(PatchRowRecord {
            old_line: 4,
            new_line: 4,
            old_text: b"old text".to_vec(),
            new_text: b"new text".to_vec(),
            old_spans: vec![PatchSpan {
                start: 0,
                length: 3,
            }],
            new_spans: vec![PatchSpan {
                start: 0,
                length: 3,
            }],
        });
        round_trip(PatchGapRecord {
            old_line: 9,
            new_line: 10,
        });
        round_trip(PatchBaseRecord { object: oid(2) });
    }

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);
        let mut invalid = Limits::HARD;
        invalid.max_concurrent_fetches = 0;
        assert!(invalid.to_extensions().is_err());
        assert!(Limits::from_extensions(&Extensions::default()).is_err());
    }
}
