//! YAS Terminal family version 1 payload codecs.
//!
//! The numeric family and kind registry is generated from
//! `protocol/yas/families/terminal.toml`; this module supplies the semantic
//! payload codecs for those generated operations.

use crate::prelude::*;

use lz4_flex::block::{compress, decompress};

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, limit_u32, put_bytes_u16,
    put_bytes_u32, put_i32, put_i64, put_len_u16, put_len_u32, put_string_u32, put_u16, put_u32,
    put_u64, read_limit_u32, reject_unknown_required_extensions,
};
use crate::state::{Record, RecordKind};
use crate::transfer::{Descriptor, Direction, Mode};

pub const VERSION: u16 = crate::schema::terminal::VERSION;

pub mod request_kind {
    pub use crate::schema::terminal::request::*;
}

pub mod event_kind {
    pub use crate::schema::terminal::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_terminals_per_session: u32,
    pub max_views_per_session: u32,
    pub max_view_rows: u32,
    pub max_view_cols: u32,
    pub max_input_bytes: u32,
    pub max_inline_query_bytes: u32,
    pub max_query_records: u32,
    pub max_hyperlink_uri_bytes: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_terminals_per_session: crate::schema::terminal::MAX_TERMINALS_PER_SESSION as u32,
        max_views_per_session: crate::schema::terminal::MAX_VIEWS_PER_SESSION as u32,
        max_view_rows: crate::schema::terminal::MAX_VIEW_ROWS as u32,
        max_view_cols: crate::schema::terminal::MAX_VIEW_COLS as u32,
        max_input_bytes: crate::schema::terminal::MAX_INPUT_BYTES as u32,
        max_inline_query_bytes: crate::schema::terminal::MAX_INLINE_QUERY_BYTES as u32,
        max_query_records: crate::schema::terminal::MAX_QUERY_RECORDS as u32,
        max_hyperlink_uri_bytes: crate::schema::terminal::MAX_HYPERLINK_URI_BYTES as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let valid = |value: u32, maximum: u32| value != 0 && value <= maximum;
        if !valid(
            self.max_terminals_per_session,
            hard.max_terminals_per_session,
        ) || !valid(self.max_views_per_session, hard.max_views_per_session)
            || !valid(self.max_view_rows, hard.max_view_rows)
            || !valid(self.max_view_cols, hard.max_view_cols)
            || !valid(self.max_input_bytes, hard.max_input_bytes)
            || !valid(self.max_inline_query_bytes, hard.max_inline_query_bytes)
            || !valid(self.max_query_records, hard.max_query_records)
            || !valid(self.max_hyperlink_uri_bytes, hard.max_hyperlink_uri_bytes)
        {
            return Err(Error::Invalid("Terminal family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::terminal::LIMIT_MAX_TERMINALS_PER_SESSION,
                self.max_terminals_per_session,
            ),
            limit_u32(
                crate::schema::terminal::LIMIT_MAX_VIEWS_PER_SESSION,
                self.max_views_per_session,
            ),
            limit_u32(
                crate::schema::terminal::LIMIT_MAX_VIEW_ROWS,
                self.max_view_rows,
            ),
            limit_u32(
                crate::schema::terminal::LIMIT_MAX_VIEW_COLS,
                self.max_view_cols,
            ),
            limit_u32(
                crate::schema::terminal::LIMIT_MAX_INPUT_BYTES,
                self.max_input_bytes,
            ),
            limit_u32(
                crate::schema::terminal::LIMIT_MAX_INLINE_QUERY_BYTES,
                self.max_inline_query_bytes,
            ),
            limit_u32(
                crate::schema::terminal::LIMIT_MAX_QUERY_RECORDS,
                self.max_query_records,
            ),
            limit_u32(
                crate::schema::terminal::LIMIT_MAX_HYPERLINK_URI_BYTES,
                self.max_hyperlink_uri_bytes,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required_extensions(
            extensions,
            &[
                crate::schema::terminal::LIMIT_MAX_TERMINALS_PER_SESSION as u16,
                crate::schema::terminal::LIMIT_MAX_VIEWS_PER_SESSION as u16,
                crate::schema::terminal::LIMIT_MAX_VIEW_ROWS as u16,
                crate::schema::terminal::LIMIT_MAX_VIEW_COLS as u16,
                crate::schema::terminal::LIMIT_MAX_INPUT_BYTES as u16,
                crate::schema::terminal::LIMIT_MAX_INLINE_QUERY_BYTES as u16,
                crate::schema::terminal::LIMIT_MAX_QUERY_RECORDS as u16,
                crate::schema::terminal::LIMIT_MAX_HYPERLINK_URI_BYTES as u16,
            ],
            "unknown required Terminal family limit",
        )?;
        let value = Self {
            max_terminals_per_session: read_limit_u32(
                extensions,
                crate::schema::terminal::LIMIT_MAX_TERMINALS_PER_SESSION,
            )?,
            max_views_per_session: read_limit_u32(
                extensions,
                crate::schema::terminal::LIMIT_MAX_VIEWS_PER_SESSION,
            )?,
            max_view_rows: read_limit_u32(
                extensions,
                crate::schema::terminal::LIMIT_MAX_VIEW_ROWS,
            )?,
            max_view_cols: read_limit_u32(
                extensions,
                crate::schema::terminal::LIMIT_MAX_VIEW_COLS,
            )?,
            max_input_bytes: read_limit_u32(
                extensions,
                crate::schema::terminal::LIMIT_MAX_INPUT_BYTES,
            )?,
            max_inline_query_bytes: read_limit_u32(
                extensions,
                crate::schema::terminal::LIMIT_MAX_INLINE_QUERY_BYTES,
            )?,
            max_query_records: read_limit_u32(
                extensions,
                crate::schema::terminal::LIMIT_MAX_QUERY_RECORDS,
            )?,
            max_hyperlink_uri_bytes: read_limit_u32(
                extensions,
                crate::schema::terminal::LIMIT_MAX_HYPERLINK_URI_BYTES,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

const MAX_INPUT_BYTES: usize = crate::schema::terminal::MAX_INPUT_BYTES as usize;
const MAX_INLINE_QUERY_BYTES: usize = crate::schema::terminal::MAX_INLINE_QUERY_BYTES as usize;
const MAX_QUERY_RECORDS: usize = crate::schema::terminal::MAX_QUERY_RECORDS as usize;
const MAX_HYPERLINK_URI_BYTES: usize = crate::schema::terminal::MAX_HYPERLINK_URI_BYTES as usize;
const MAX_CATALOG_SEARCH_QUERY_BYTES: usize =
    crate::schema::terminal::MAX_CATALOG_SEARCH_QUERY_BYTES as usize;
const MAX_CATALOG_SEARCH_CONTEXT_BYTES: usize =
    crate::schema::terminal::MAX_CATALOG_SEARCH_CONTEXT_BYTES as usize;
const CELL_BYTES: usize = 12;

fn nonzero_handle(value: u64, what: &'static str) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid(what))
    } else {
        Ok(())
    }
}

fn nonzero_view(value: u32) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid("zero Terminal view ID"))
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Lifecycle {
    Running = crate::schema::terminal::LIFECYCLE_RUNNING as u8,
    Exited = crate::schema::terminal::LIFECYCLE_EXITED as u8,
}

impl TryFrom<u8> for Lifecycle {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Running as u8 => Ok(Self::Running),
            value if value == Self::Exited as u8 => Ok(Self::Exited),
            _ => Err(Error::Invalid("Terminal lifecycle")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalRecord {
    pub terminal_handle: u64,
    pub lifecycle: Lifecycle,
    pub rows: u16,
    pub cols: u16,
    pub generation: u32,
    pub used_rows: u32,
    pub extensions: Extensions,
}

impl TerminalRecord {
    fn validate(&self) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        if self.rows == 0 || self.cols == 0 || self.generation == 0 {
            return Err(Error::Invalid("Terminal state dimensions or generation"));
        }
        self.extensions.validate()
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Terminal complete state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for TerminalRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.terminal_handle);
        out.push(self.lifecycle as u8);
        out.push(0);
        put_u16(out, self.rows);
        put_u16(out, self.cols);
        put_u32(out, self.generation);
        put_u32(out, self.used_rows);
        self.extensions.encode_tail(out)
    }
}

impl Decode for TerminalRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let lifecycle = Lifecycle::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Terminal state reserved byte"));
        }
        let value = Self {
            terminal_handle,
            lifecycle,
            rows: decoder.u16()?,
            cols: decoder.u16()?,
            generation: decoder.u32()?,
            used_rows: decoder.u32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalPatch {
    pub terminal_handle: u64,
    pub extensions: Extensions,
}

impl TerminalPatch {
    pub fn state_record(&self) -> Result<Record> {
        Ok(Record {
            kind: RecordKind::Patch,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for TerminalPatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        put_u64(out, self.terminal_handle);
        self.extensions.encode_tail(out)
    }
}

impl Decode for TerminalPatch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            terminal_handle: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        nonzero_handle(value.terminal_handle, "zero terminal handle")?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovedTerminal {
    pub terminal_handle: u64,
}

impl RemovedTerminal {
    pub fn state_record(self) -> Result<Record> {
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for RemovedTerminal {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        put_u64(out, self.terminal_handle);
        Ok(())
    }
}

impl Decode for RemovedTerminal {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            terminal_handle: decoder.u64()?,
        };
        decoder.finish()?;
        nonzero_handle(value.terminal_handle, "zero terminal handle")?;
        Ok(value)
    }
}

pub fn terminal_from_state_record(record: &Record) -> Result<TerminalRecord> {
    if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
        return Err(Error::Invalid("Terminal complete state record kind"));
    }
    TerminalRecord::decode(&record.body)
}

pub fn patch_from_state_record(record: &Record) -> Result<TerminalPatch> {
    if record.kind != RecordKind::Patch {
        return Err(Error::Invalid("Terminal patch state record kind"));
    }
    TerminalPatch::decode(&record.body)
}

pub fn removal_from_state_record(record: &Record) -> Result<RemovedTerminal> {
    if record.kind != RecordKind::Remove {
        return Err(Error::Invalid("Terminal removal state record kind"));
    }
    RemovedTerminal::decode(&record.body)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitReason {
    Unknown,
    Interrupt,
    Terminate,
    Kill,
    Hangup,
}

impl ExitReason {
    fn wire(self) -> u8 {
        match self {
            Self::Unknown => crate::schema::terminal::EXIT_REASON_UNKNOWN as u8,
            Self::Interrupt => crate::schema::terminal::EXIT_REASON_INTERRUPT as u8,
            Self::Terminate => crate::schema::terminal::EXIT_REASON_TERMINATE as u8,
            Self::Kill => crate::schema::terminal::EXIT_REASON_KILL as u8,
            Self::Hangup => crate::schema::terminal::EXIT_REASON_HANGUP as u8,
        }
    }

    fn decode(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::terminal::EXIT_REASON_UNKNOWN as u8 => {
                Ok(Self::Unknown)
            }
            value if value == crate::schema::terminal::EXIT_REASON_INTERRUPT as u8 => {
                Ok(Self::Interrupt)
            }
            value if value == crate::schema::terminal::EXIT_REASON_TERMINATE as u8 => {
                Ok(Self::Terminate)
            }
            value if value == crate::schema::terminal::EXIT_REASON_KILL as u8 => Ok(Self::Kill),
            value if value == crate::schema::terminal::EXIT_REASON_HANGUP as u8 => Ok(Self::Hangup),
            _ => Err(Error::Invalid("Terminal exit reason")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExitRecord {
    Code {
        code: i32,
        detail: String,
    },
    Signal {
        reason: ExitReason,
        native_signal: i32,
        detail: String,
    },
    Other {
        detail: String,
    },
}

impl Encode for ExitRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let (kind, reason, code, detail) = match self {
            Self::Code { code, detail } => (
                crate::schema::terminal::EXIT_KIND_CODE as u8,
                ExitReason::Unknown,
                *code,
                detail,
            ),
            Self::Signal {
                reason,
                native_signal,
                detail,
            } => (
                crate::schema::terminal::EXIT_KIND_SIGNAL as u8,
                *reason,
                *native_signal,
                detail,
            ),
            Self::Other { detail } if !detail.is_empty() => (
                crate::schema::terminal::EXIT_KIND_OTHER as u8,
                ExitReason::Unknown,
                0,
                detail,
            ),
            Self::Other { .. } => return Err(Error::Invalid("empty Terminal OTHER exit detail")),
        };
        out.push(kind);
        out.push(reason.wire());
        put_u16(out, 0);
        put_i32(out, code);
        put_string_u32(out, detail)
    }
}

impl Decode for ExitRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        let reason = ExitReason::decode(decoder.u8()?)?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Terminal exit reserved field"));
        }
        let code = decoder.i32()?;
        let detail = decoder.string_u32()?;
        decoder.finish()?;
        match kind {
            value
                if value == crate::schema::terminal::EXIT_KIND_CODE as u8
                    && reason == ExitReason::Unknown =>
            {
                Ok(Self::Code { code, detail })
            }
            value if value == crate::schema::terminal::EXIT_KIND_SIGNAL as u8 => Ok(Self::Signal {
                reason,
                native_signal: code,
                detail,
            }),
            value
                if value == crate::schema::terminal::EXIT_KIND_OTHER as u8
                    && reason == ExitReason::Unknown
                    && code == 0
                    && !detail.is_empty() =>
            {
                Ok(Self::Other { detail })
            }
            _ => Err(Error::Invalid("Terminal exit record")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    DefaultShell,
    Argv(Vec<Vec<u8>>),
    ShellCommand(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cwd {
    ServerDefault,
    Path(Vec<u8>),
    Terminal(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvironmentBase {
    Server,
    Empty,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EnvironmentValue {
    Set(Vec<u8>),
    Remove,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvironmentEntry {
    pub key: Vec<u8>,
    pub value: EnvironmentValue,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub command: Command,
    pub cwd: Cwd,
    pub environment_base: EnvironmentBase,
    pub environment: Vec<EnvironmentEntry>,
    pub extensions: Extensions,
}

impl Launch {
    fn validate(&self) -> Result<()> {
        if matches!(&self.command, Command::Argv(argv) if argv.is_empty()) {
            return Err(Error::Invalid("empty Terminal argv"));
        }
        if matches!(&self.cwd, Cwd::Path(path) if path.is_empty()) {
            return Err(Error::Invalid("empty Terminal cwd path"));
        }
        if let Cwd::Terminal(handle) = self.cwd {
            nonzero_handle(handle, "zero source terminal handle")?;
        }
        let mut keys = BTreeSet::new();
        for entry in &self.environment {
            if entry.key.is_empty() || !keys.insert(entry.key.as_slice()) {
                return Err(Error::Invalid("Terminal environment key"));
            }
        }
        self.extensions.validate()
    }
}

impl Encode for Launch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(match self.command {
            Command::DefaultShell => crate::schema::terminal::COMMAND_DEFAULT_SHELL as u8,
            Command::Argv(_) => crate::schema::terminal::COMMAND_ARGV as u8,
            Command::ShellCommand(_) => crate::schema::terminal::COMMAND_SHELL_COMMAND as u8,
        });
        out.push(match self.cwd {
            Cwd::ServerDefault => crate::schema::terminal::CWD_SERVER_DEFAULT as u8,
            Cwd::Path(_) => crate::schema::terminal::CWD_PATH as u8,
            Cwd::Terminal(_) => crate::schema::terminal::CWD_TERMINAL as u8,
        });
        out.push(match self.environment_base {
            EnvironmentBase::Server => crate::schema::terminal::ENVIRONMENT_SERVER as u8,
            EnvironmentBase::Empty => crate::schema::terminal::ENVIRONMENT_EMPTY as u8,
        });
        out.push(0);
        match &self.command {
            Command::DefaultShell => {}
            Command::Argv(argv) => {
                put_len_u16(out, argv.len())?;
                for arg in argv {
                    put_bytes_u32(out, arg)?;
                }
            }
            Command::ShellCommand(command) => put_string_u32(out, command)?,
        }
        match &self.cwd {
            Cwd::ServerDefault => {}
            Cwd::Path(path) => put_bytes_u32(out, path)?,
            Cwd::Terminal(handle) => put_u64(out, *handle),
        }
        put_len_u16(out, self.environment.len())?;
        for entry in &self.environment {
            put_bytes_u16(out, &entry.key)?;
            match &entry.value {
                EnvironmentValue::Set(value) => {
                    out.push(crate::schema::terminal::ENVIRONMENT_SET as u8);
                    put_bytes_u32(out, value)?;
                }
                EnvironmentValue::Remove => {
                    out.push(crate::schema::terminal::ENVIRONMENT_REMOVE as u8);
                    put_len_u32(out, 0)?;
                }
            }
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Launch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let command_kind = decoder.u8()?;
        let cwd_kind = decoder.u8()?;
        let environment_base = match decoder.u8()? {
            value if value == crate::schema::terminal::ENVIRONMENT_SERVER as u8 => {
                EnvironmentBase::Server
            }
            value if value == crate::schema::terminal::ENVIRONMENT_EMPTY as u8 => {
                EnvironmentBase::Empty
            }
            _ => return Err(Error::Invalid("Terminal environment base")),
        };
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Terminal launch reserved byte"));
        }
        let command = match command_kind {
            value if value == crate::schema::terminal::COMMAND_DEFAULT_SHELL as u8 => {
                Command::DefaultShell
            }
            value if value == crate::schema::terminal::COMMAND_ARGV as u8 => {
                let count = usize::from(decoder.u16()?);
                let mut argv = Vec::with_capacity(count);
                for _ in 0..count {
                    argv.push(decoder.len_bytes_u32()?.to_vec());
                }
                Command::Argv(argv)
            }
            value if value == crate::schema::terminal::COMMAND_SHELL_COMMAND as u8 => {
                Command::ShellCommand(decoder.string_u32()?)
            }
            _ => return Err(Error::Invalid("Terminal command kind")),
        };
        let cwd = match cwd_kind {
            value if value == crate::schema::terminal::CWD_SERVER_DEFAULT as u8 => {
                Cwd::ServerDefault
            }
            value if value == crate::schema::terminal::CWD_PATH as u8 => {
                Cwd::Path(decoder.len_bytes_u32()?.to_vec())
            }
            value if value == crate::schema::terminal::CWD_TERMINAL as u8 => {
                Cwd::Terminal(decoder.u64()?)
            }
            _ => return Err(Error::Invalid("Terminal cwd kind")),
        };
        let count = usize::from(decoder.u16()?);
        let mut environment = Vec::with_capacity(count);
        for _ in 0..count {
            let key = decoder.len_bytes_u16()?.to_vec();
            let kind = decoder.u8()?;
            let bytes = decoder.len_bytes_u32()?.to_vec();
            let value = match kind {
                value if value == crate::schema::terminal::ENVIRONMENT_SET as u8 => {
                    EnvironmentValue::Set(bytes)
                }
                value
                    if value == crate::schema::terminal::ENVIRONMENT_REMOVE as u8
                        && bytes.is_empty() =>
                {
                    EnvironmentValue::Remove
                }
                value if value == crate::schema::terminal::ENVIRONMENT_REMOVE as u8 => {
                    return Err(Error::Invalid("Terminal REMOVE environment value"));
                }
                _ => return Err(Error::Invalid("Terminal environment entry kind")),
            };
            environment.push(EnvironmentEntry { key, value });
        }
        let value = Self {
            command,
            cwd,
            environment_base,
            environment,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Create {
    pub rows: u16,
    pub cols: u16,
    pub operation_id: [u8; 16],
    pub launch: Launch,
    pub extensions: Extensions,
}

impl Encode for Create {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.rows == 0 || self.cols == 0 {
            return Err(Error::Invalid("Terminal CREATE dimensions"));
        }
        put_u16(out, self.rows);
        put_u16(out, self.cols);
        put_u32(out, 0);
        out.extend_from_slice(&self.operation_id);
        put_bytes_u32(out, &self.launch.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Create {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let rows = decoder.u16()?;
        let cols = decoder.u16()?;
        if decoder.u32()? != 0 {
            return Err(Error::Invalid("Terminal CREATE reserved field"));
        }
        let value = Self {
            rows,
            cols,
            operation_id: decoder.array_16()?,
            launch: Launch::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        if value.rows == 0 || value.cols == 0 {
            return Err(Error::Invalid("Terminal CREATE dimensions"));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateResult {
    pub terminal_handle: u64,
    pub state_revision: u64,
    pub generation: u32,
    pub extensions: Extensions,
}

impl Encode for CreateResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        if self.state_revision == 0 || self.generation == 0 {
            return Err(Error::Invalid(
                "Terminal CREATE result revision or generation",
            ));
        }
        put_u64(out, self.terminal_handle);
        put_u64(out, self.state_revision);
        put_u32(out, self.generation);
        put_u32(out, 0);
        self.extensions.encode_tail(out)
    }
}

impl Decode for CreateResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            terminal_handle: decoder.u64()?,
            state_revision: decoder.u64()?,
            generation: decoder.u32()?,
            extensions: {
                if decoder.u32()? != 0 {
                    return Err(Error::Invalid("Terminal CREATE result reserved field"));
                }
                decoder.extensions()?
            },
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchMode {
    Replay,
    Replace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CutoverMode {
    StopThenStart,
    StartThenSwitch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restart {
    pub terminal_handle: u64,
    pub operation_id: [u8; 16],
    pub launch_mode: LaunchMode,
    pub cutover_mode: CutoverMode,
    pub launch: Option<Launch>,
    pub extensions: Extensions,
}

impl Restart {
    fn validate(&self) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        match (self.launch_mode, &self.launch) {
            (LaunchMode::Replay, None) | (LaunchMode::Replace, Some(_)) => Ok(()),
            _ => Err(Error::Invalid("Terminal RESTART launch presence")),
        }
    }
}

impl Encode for Restart {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.terminal_handle);
        out.extend_from_slice(&self.operation_id);
        out.push(match self.launch_mode {
            LaunchMode::Replay => crate::schema::terminal::LAUNCH_REPLAY as u8,
            LaunchMode::Replace => crate::schema::terminal::LAUNCH_REPLACE as u8,
        });
        out.push(match self.cutover_mode {
            CutoverMode::StopThenStart => crate::schema::terminal::CUTOVER_STOP_THEN_START as u8,
            CutoverMode::StartThenSwitch => {
                crate::schema::terminal::CUTOVER_START_THEN_SWITCH as u8
            }
        });
        put_u16(out, 0);
        if let Some(launch) = &self.launch {
            put_bytes_u32(out, &launch.encode()?)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Restart {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let launch_mode = match decoder.u8()? {
            value if value == crate::schema::terminal::LAUNCH_REPLAY as u8 => LaunchMode::Replay,
            value if value == crate::schema::terminal::LAUNCH_REPLACE as u8 => LaunchMode::Replace,
            _ => return Err(Error::Invalid("Terminal RESTART launch mode")),
        };
        let cutover_mode = match decoder.u8()? {
            value if value == crate::schema::terminal::CUTOVER_STOP_THEN_START as u8 => {
                CutoverMode::StopThenStart
            }
            value if value == crate::schema::terminal::CUTOVER_START_THEN_SWITCH as u8 => {
                CutoverMode::StartThenSwitch
            }
            _ => return Err(Error::Invalid("Terminal RESTART cutover mode")),
        };
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Terminal RESTART reserved field"));
        }
        let launch = match launch_mode {
            LaunchMode::Replay => None,
            LaunchMode::Replace => Some(Launch::decode(decoder.len_bytes_u32()?)?),
        };
        let value = Self {
            terminal_handle,
            operation_id,
            launch_mode,
            cutover_mode,
            launch,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestartResult {
    pub state_revision: u64,
    pub generation: u32,
}

impl Encode for RestartResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.state_revision == 0 || self.generation == 0 {
            return Err(Error::Invalid(
                "Terminal RESTART result revision or generation",
            ));
        }
        put_u64(out, self.state_revision);
        put_u32(out, self.generation);
        put_u32(out, 0);
        Ok(())
    }
}

impl Decode for RestartResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            state_revision: decoder.u64()?,
            generation: decoder.u32()?,
        };
        if decoder.u32()? != 0 {
            return Err(Error::Invalid("Terminal RESTART result reserved field"));
        }
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalKind {
    Interrupt,
    Terminate,
    Kill,
    Hangup,
}

impl SignalKind {
    fn wire(self) -> u16 {
        match self {
            Self::Interrupt => crate::schema::terminal::SIGNAL_INTERRUPT as u16,
            Self::Terminate => crate::schema::terminal::SIGNAL_TERMINATE as u16,
            Self::Kill => crate::schema::terminal::SIGNAL_KILL as u16,
            Self::Hangup => crate::schema::terminal::SIGNAL_HANGUP as u16,
        }
    }

    fn decode(value: u16) -> Result<Self> {
        match value {
            value if value == crate::schema::terminal::SIGNAL_INTERRUPT as u16 => {
                Ok(Self::Interrupt)
            }
            value if value == crate::schema::terminal::SIGNAL_TERMINATE as u16 => {
                Ok(Self::Terminate)
            }
            value if value == crate::schema::terminal::SIGNAL_KILL as u16 => Ok(Self::Kill),
            value if value == crate::schema::terminal::SIGNAL_HANGUP as u16 => Ok(Self::Hangup),
            _ => Err(Error::Invalid("Terminal signal")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signal {
    pub terminal_handle: u64,
    pub operation_id: [u8; 16],
    pub signal: SignalKind,
    pub extensions: Extensions,
}

impl Encode for Signal {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        put_u64(out, self.terminal_handle);
        out.extend_from_slice(&self.operation_id);
        put_u16(out, self.signal.wire());
        put_u16(out, 0);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Signal {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let signal = SignalKind::decode(decoder.u16()?)?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Terminal SIGNAL reserved field"));
        }
        let value = Self {
            terminal_handle,
            operation_id,
            signal,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        nonzero_handle(value.terminal_handle, "zero terminal handle")?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Close {
    pub terminal_handle: u64,
    pub operation_id: [u8; 16],
}

impl Encode for Close {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        put_u64(out, self.terminal_handle);
        out.extend_from_slice(&self.operation_id);
        Ok(())
    }
}

impl Decode for Close {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            terminal_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
        };
        decoder.finish()?;
        nonzero_handle(value.terminal_handle, "zero terminal handle")?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deadline {
    Clear,
    Set(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetDeadline {
    pub terminal_handle: u64,
    pub operation_id: [u8; 16],
    pub deadline: Deadline,
}

impl Encode for SetDeadline {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        put_u64(out, self.terminal_handle);
        out.extend_from_slice(&self.operation_id);
        let (mode, duration) = match self.deadline {
            Deadline::Clear => (crate::schema::terminal::DEADLINE_CLEAR as u8, 0),
            Deadline::Set(duration) if duration != 0 => {
                (crate::schema::terminal::DEADLINE_SET as u8, duration)
            }
            Deadline::Set(_) => return Err(Error::Invalid("zero Terminal deadline duration")),
        };
        out.push(mode);
        out.extend_from_slice(&[0; 7]);
        put_u64(out, duration);
        Ok(())
    }
}

impl Decode for SetDeadline {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let mode = decoder.u8()?;
        if decoder.take(7)? != [0; 7] {
            return Err(Error::Invalid("Terminal SET_DEADLINE reserved bytes"));
        }
        let duration = decoder.u64()?;
        let deadline = match mode {
            value if value == crate::schema::terminal::DEADLINE_CLEAR as u8 && duration == 0 => {
                Deadline::Clear
            }
            value if value == crate::schema::terminal::DEADLINE_SET as u8 && duration != 0 => {
                Deadline::Set(duration)
            }
            _ => return Err(Error::Invalid("Terminal deadline mode or duration")),
        };
        decoder.finish()?;
        nonzero_handle(terminal_handle, "zero terminal handle")?;
        Ok(Self {
            terminal_handle,
            operation_id,
            deadline,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resize {
    pub terminal_handle: u64,
    pub rows: u16,
    pub cols: u16,
}

impl Encode for Resize {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        if self.rows == 0 || self.cols == 0 {
            return Err(Error::Invalid("Terminal RESIZE dimensions"));
        }
        put_u64(out, self.terminal_handle);
        put_u16(out, self.rows);
        put_u16(out, self.cols);
        Ok(())
    }
}

impl Decode for Resize {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            terminal_handle: decoder.u64()?,
            rows: decoder.u16()?,
            cols: decoder.u16()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResizeResult {
    pub state_revision: u64,
}

impl Encode for ResizeResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.state_revision == 0 {
            return Err(Error::Invalid("zero Terminal state revision"));
        }
        put_u64(out, self.state_revision);
        Ok(())
    }
}

impl Decode for ResizeResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            state_revision: decoder.u64()?,
        };
        decoder.finish()?;
        if value.state_revision == 0 {
            return Err(Error::Invalid("zero Terminal state revision"));
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetFocus {
    pub view_id: u32,
    pub focused: bool,
}

impl Encode for SetFocus {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_view(self.view_id)?;
        put_u32(out, self.view_id);
        out.push(u8::from(self.focused));
        out.extend_from_slice(&[0; 3]);
        Ok(())
    }
}

impl Decode for SetFocus {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let focused = match decoder.u8()? {
            0 => false,
            1 => true,
            _ => return Err(Error::Invalid("Terminal focus boolean")),
        };
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Terminal SET_FOCUS reserved bytes"));
        }
        decoder.finish()?;
        nonzero_view(view_id)?;
        Ok(Self { view_id, focused })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollMode {
    Absolute,
    Relative,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scroll {
    pub view_id: u32,
    pub mode: ScrollMode,
    pub amount: i64,
}

impl Encode for Scroll {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_view(self.view_id)?;
        put_u32(out, self.view_id);
        out.push(match self.mode {
            ScrollMode::Absolute => crate::schema::terminal::SCROLL_ABSOLUTE as u8,
            ScrollMode::Relative => crate::schema::terminal::SCROLL_RELATIVE as u8,
        });
        out.extend_from_slice(&[0; 7]);
        put_i64(out, self.amount);
        Ok(())
    }
}

impl Decode for Scroll {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let mode = match decoder.u8()? {
            value if value == crate::schema::terminal::SCROLL_ABSOLUTE as u8 => {
                ScrollMode::Absolute
            }
            value if value == crate::schema::terminal::SCROLL_RELATIVE as u8 => {
                ScrollMode::Relative
            }
            _ => return Err(Error::Invalid("Terminal scroll mode")),
        };
        if decoder.take(7)? != [0; 7] {
            return Err(Error::Invalid("Terminal SCROLL reserved bytes"));
        }
        let amount = decoder.i64()?;
        decoder.finish()?;
        nonzero_view(view_id)?;
        Ok(Self {
            view_id,
            mode,
            amount,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrollResult {
    pub applied_offset: i64,
}

impl Encode for ScrollResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        put_i64(out, self.applied_offset);
        Ok(())
    }
}

impl Decode for ScrollResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            applied_offset: decoder.i64()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenView {
    pub terminal_handle: u64,
    pub rows: u16,
    pub cols: u16,
    pub max_fps: u16,
    pub codec_versions: Vec<u16>,
    pub extensions: Extensions,
}

impl Encode for OpenView {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        if self.rows == 0
            || self.cols == 0
            || self.max_fps == 0
            || self.codec_versions.is_empty()
            || self.codec_versions.len() > usize::from(u8::MAX)
        {
            return Err(Error::Invalid("Terminal OPEN_VIEW parameters"));
        }
        let mut previous = None;
        for codec in &self.codec_versions {
            if *codec == 0 || previous.is_some_and(|old| old >= *codec) {
                return Err(Error::Invalid("Terminal grid codec list"));
            }
            previous = Some(*codec);
        }
        put_u64(out, self.terminal_handle);
        put_u16(out, self.rows);
        put_u16(out, self.cols);
        put_u16(out, self.max_fps);
        out.push(self.codec_versions.len() as u8);
        out.push(0);
        for codec in &self.codec_versions {
            put_u16(out, *codec);
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for OpenView {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let rows = decoder.u16()?;
        let cols = decoder.u16()?;
        let max_fps = decoder.u16()?;
        let count = usize::from(decoder.u8()?);
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Terminal OPEN_VIEW reserved byte"));
        }
        let mut codec_versions = Vec::with_capacity(count);
        for _ in 0..count {
            codec_versions.push(decoder.u16()?);
        }
        let value = Self {
            terminal_handle,
            rows,
            cols,
            max_fps,
            codec_versions,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

/// Terminal CREATE extension tag `CREATE_INITIAL_VIEW_EXTENSION`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialViewRequest {
    pub rows: u16,
    pub cols: u16,
    pub max_fps: u16,
    pub codec_versions: Vec<u16>,
    pub extensions: Extensions,
}

impl Encode for InitialViewRequest {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.rows == 0
            || self.cols == 0
            || self.max_fps == 0
            || self.codec_versions.is_empty()
            || self.codec_versions.len() > usize::from(u8::MAX)
        {
            return Err(Error::Invalid("Terminal initial-view parameters"));
        }
        let mut previous = None;
        for codec in &self.codec_versions {
            if *codec == 0 || previous.is_some_and(|old| old >= *codec) {
                return Err(Error::Invalid("Terminal grid codec list"));
            }
            previous = Some(*codec);
        }
        put_u16(out, self.rows);
        put_u16(out, self.cols);
        put_u16(out, self.max_fps);
        out.push(self.codec_versions.len() as u8);
        out.push(0);
        for codec in &self.codec_versions {
            put_u16(out, *codec);
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for InitialViewRequest {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let rows = decoder.u16()?;
        let cols = decoder.u16()?;
        let max_fps = decoder.u16()?;
        let count = usize::from(decoder.u8()?);
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Terminal initial-view reserved byte"));
        }
        let mut codec_versions = Vec::with_capacity(count);
        for _ in 0..count {
            codec_versions.push(decoder.u16()?);
        }
        let value = Self {
            rows,
            cols,
            max_fps,
            codec_versions,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenViewResult {
    pub view_id: u32,
    pub codec_version: u16,
    pub max_inflight_frames: u8,
    pub max_encoded_frame: u32,
    pub max_decoded_frame: u32,
    pub first_sequence: u32,
    pub extensions: Extensions,
}

impl Encode for OpenViewResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_view(self.view_id)?;
        if self.codec_version == 0
            || self.max_inflight_frames == 0
            || self.max_encoded_frame == 0
            || self.max_decoded_frame == 0
        {
            return Err(Error::Invalid("Terminal OPEN_VIEW result limits"));
        }
        put_u32(out, self.view_id);
        put_u16(out, self.codec_version);
        out.push(self.max_inflight_frames);
        out.push(0);
        put_u32(out, self.max_encoded_frame);
        put_u32(out, self.max_decoded_frame);
        put_u32(out, self.first_sequence);
        self.extensions.encode_tail(out)
    }
}

impl Decode for OpenViewResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let codec_version = decoder.u16()?;
        let max_inflight_frames = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Terminal OPEN_VIEW result reserved byte"));
        }
        let value = Self {
            view_id,
            codec_version,
            max_inflight_frames,
            max_encoded_frame: decoder.u32()?,
            max_decoded_frame: decoder.u32()?,
            first_sequence: decoder.u32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationMetrics {
    pub viewport_width_px: u32,
    pub viewport_height_px: u32,
    pub cell_width_16_16: u32,
    pub cell_height_16_16: u32,
    pub device_scale_16_16: u32,
}

impl Encode for PresentationMetrics {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.viewport_width_px == 0
            || self.viewport_height_px == 0
            || self.cell_width_16_16 == 0
            || self.cell_height_16_16 == 0
            || self.device_scale_16_16 == 0
        {
            return Err(Error::Invalid("Terminal presentation metrics"));
        }
        put_u32(out, self.viewport_width_px);
        put_u32(out, self.viewport_height_px);
        put_u32(out, self.cell_width_16_16);
        put_u32(out, self.cell_height_16_16);
        put_u32(out, self.device_scale_16_16);
        Ok(())
    }
}

impl Decode for PresentationMetrics {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            viewport_width_px: decoder.u32()?,
            viewport_height_px: decoder.u32()?,
            cell_width_16_16: decoder.u32()?,
            cell_height_16_16: decoder.u32()?,
            device_scale_16_16: decoder.u32()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewConfiguration {
    pub rows: Option<u16>,
    pub cols: Option<u16>,
    pub max_fps: Option<u16>,
    pub presentation_metrics: Option<PresentationMetrics>,
    pub queue_target: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigureView {
    pub view_id: u32,
    pub configuration: ViewConfiguration,
    /// Unknown optional extensions; tags 1 through 5 are reserved above.
    pub extensions: Extensions,
}

impl ConfigureView {
    fn wire_extensions(&self) -> Result<Extensions> {
        let reserved = |tag| {
            (crate::schema::terminal::CONFIGURE_ROWS_EXTENSION as u16
                ..=crate::schema::terminal::CONFIGURE_QUEUE_TARGET_EXTENSION as u16)
                .contains(&tag)
        };
        if self.extensions.0.iter().any(|entry| reserved(entry.tag)) {
            return Err(Error::Invalid("typed Terminal view extension duplicated"));
        }
        let mut entries = self.extensions.0.clone();
        let mut add = |tag: u16, value: Vec<u8>| {
            entries.push(Extension {
                tag,
                required: false,
                value,
            });
        };
        if let Some(value) = self.configuration.rows {
            if value == 0 {
                return Err(Error::Invalid("zero Terminal configured rows"));
            }
            add(
                crate::schema::terminal::CONFIGURE_ROWS_EXTENSION as u16,
                value.to_le_bytes().to_vec(),
            );
        }
        if let Some(value) = self.configuration.cols {
            if value == 0 {
                return Err(Error::Invalid("zero Terminal configured columns"));
            }
            add(
                crate::schema::terminal::CONFIGURE_COLS_EXTENSION as u16,
                value.to_le_bytes().to_vec(),
            );
        }
        if let Some(value) = self.configuration.max_fps {
            if value == 0 {
                return Err(Error::Invalid("zero Terminal configured FPS"));
            }
            add(
                crate::schema::terminal::CONFIGURE_MAX_FPS_EXTENSION as u16,
                value.to_le_bytes().to_vec(),
            );
        }
        if let Some(value) = self.configuration.presentation_metrics {
            add(
                crate::schema::terminal::CONFIGURE_PRESENTATION_METRICS_EXTENSION as u16,
                value.encode()?,
            );
        }
        if let Some(value) = self.configuration.queue_target {
            if value == 0 {
                return Err(Error::Invalid("zero Terminal queue target"));
            }
            add(
                crate::schema::terminal::CONFIGURE_QUEUE_TARGET_EXTENSION as u16,
                vec![value],
            );
        }
        entries.sort_by_key(|entry| entry.tag);
        let value = Extensions(entries);
        value.validate()?;
        Ok(value)
    }
}

impl Encode for ConfigureView {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_view(self.view_id)?;
        put_u32(out, self.view_id);
        self.wire_extensions()?.encode_tail(out)
    }
}

impl Decode for ConfigureView {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let mut entries = decoder.extensions()?.0;
        decoder.finish()?;
        nonzero_view(view_id)?;
        let take = |entries: &mut Vec<Extension>, tag: u16| {
            entries
                .iter()
                .position(|entry| entry.tag == tag)
                .map(|position| entries.remove(position).value)
        };
        let decode_u16 = |value: Option<Vec<u8>>, what| -> Result<Option<u16>> {
            value
                .map(|value| {
                    if value.len() != 2 {
                        return Err(Error::Invalid(what));
                    }
                    let decoded = u16::from_le_bytes(value.try_into().unwrap());
                    if decoded == 0 {
                        return Err(Error::Invalid(what));
                    }
                    Ok(decoded)
                })
                .transpose()
        };
        let rows = decode_u16(
            take(
                &mut entries,
                crate::schema::terminal::CONFIGURE_ROWS_EXTENSION as u16,
            ),
            "Terminal configured rows",
        )?;
        let cols = decode_u16(
            take(
                &mut entries,
                crate::schema::terminal::CONFIGURE_COLS_EXTENSION as u16,
            ),
            "Terminal configured columns",
        )?;
        let max_fps = decode_u16(
            take(
                &mut entries,
                crate::schema::terminal::CONFIGURE_MAX_FPS_EXTENSION as u16,
            ),
            "Terminal configured FPS",
        )?;
        let presentation_metrics = take(
            &mut entries,
            crate::schema::terminal::CONFIGURE_PRESENTATION_METRICS_EXTENSION as u16,
        )
        .map(|value| PresentationMetrics::decode(&value))
        .transpose()?;
        let queue_target = take(
            &mut entries,
            crate::schema::terminal::CONFIGURE_QUEUE_TARGET_EXTENSION as u16,
        )
        .map(|value| match value.as_slice() {
            [value @ 1..=u8::MAX] => Ok(*value),
            _ => Err(Error::Invalid("Terminal queue target")),
        })
        .transpose()?;
        Ok(Self {
            view_id,
            configuration: ViewConfiguration {
                rows,
                cols,
                max_fps,
                presentation_metrics,
                queue_target,
            },
            extensions: Extensions(entries),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewRequest {
    pub view_id: u32,
}

impl Encode for ViewRequest {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_view(self.view_id)?;
        put_u32(out, self.view_id);
        Ok(())
    }
}

impl Decode for ViewRequest {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            view_id: decoder.u32()?,
        };
        decoder.finish()?;
        nonzero_view(value.view_id)?;
        Ok(value)
    }
}

pub type ResetView = ViewRequest;
pub type CloseView = ViewRequest;

fn validate_query_identity(terminal_handle: u64, generation: u32) -> Result<()> {
    nonzero_handle(terminal_handle, "zero terminal handle")?;
    if generation == 0 {
        return Err(Error::Invalid("zero Terminal generation"));
    }
    Ok(())
}

fn validate_query_representation(representation: u8) -> Result<()> {
    if representation > crate::schema::terminal::QUERY_REPRESENTATION_BOTH as u8 {
        return Err(Error::Invalid("Terminal query representation"));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Read {
    pub terminal_handle: u64,
    pub generation: u32,
    pub cursor_kind: u8,
    pub representation: u8,
    pub flags: u16,
    pub cursor_a: u64,
    pub cursor_b: u32,
    pub max_bytes: u32,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Read {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_identity(self.terminal_handle, self.generation)?;
        validate_query_representation(self.representation)?;
        if self.cursor_kind > crate::schema::terminal::READ_CURSOR_TAIL as u8
            || self.flags != crate::schema::terminal::READ_FLAGS as u16
        {
            return Err(Error::Invalid("Terminal READ cursor or flags"));
        }
        if self.max_bytes == 0 {
            return Err(Error::Invalid("zero Terminal query byte limit"));
        }
        put_u64(out, self.terminal_handle);
        put_u32(out, self.generation);
        out.push(self.cursor_kind);
        out.push(self.representation);
        put_u16(out, self.flags);
        put_u64(out, self.cursor_a);
        put_u32(out, self.cursor_b);
        put_u32(out, self.max_bytes);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Read {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            terminal_handle: decoder.u64()?,
            generation: decoder.u32()?,
            cursor_kind: decoder.u8()?,
            representation: decoder.u8()?,
            flags: decoder.u16()?,
            cursor_a: decoder.u64()?,
            cursor_b: decoder.u32()?,
            max_bytes: decoder.u32()?,
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
pub struct Search {
    pub terminal_handle: u64,
    pub generation: u32,
    pub flags: u16,
    pub start_cursor: QueryCursor,
    pub max_results: u32,
    pub query: Vec<u8>,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Search {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_identity(self.terminal_handle, self.generation)?;
        if self.flags & !(crate::schema::terminal::SEARCH_FLAGS as u16) != 0
            || self.start_cursor.kind != crate::schema::terminal::SEARCH_CURSOR_POSITION as u8
            || self.max_results == 0
            || self.query.is_empty()
            || self.query.len() > MAX_INPUT_BYTES
        {
            return Err(Error::Invalid("Terminal SEARCH bounds or query"));
        }
        put_u64(out, self.terminal_handle);
        put_u32(out, self.generation);
        put_u16(out, self.flags);
        put_u16(out, 0);
        self.start_cursor.encode_to(out)?;
        put_u32(out, self.max_results);
        put_bytes_u32(out, &self.query)?;
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Search {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let generation = decoder.u32()?;
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Terminal SEARCH reserved field"));
        }
        let value = Self {
            terminal_handle,
            generation,
            flags,
            start_cursor: QueryCursor {
                kind: decoder.u8()?,
                a: decoder.u64()?,
                b: decoder.u32()?,
            },
            max_results: decoder.u32()?,
            query: decoder.len_bytes_u32()?.to_vec(),
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

/// Search the complete Terminal catalogue using the terminal model's
/// ranked-search semantics rather than one terminal's scrollback cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSearch {
    pub max_results: u32,
    pub query: String,
    pub extensions: Extensions,
}

impl CatalogSearch {
    fn validate(&self) -> Result<()> {
        if self.max_results == 0 || self.max_results as usize > MAX_QUERY_RECORDS {
            return Err(Error::Invalid("Terminal catalogue search result limit"));
        }
        if self.query.len() > MAX_CATALOG_SEARCH_QUERY_BYTES {
            return Err(Error::Invalid("Terminal catalogue search query"));
        }
        reject_unknown_required_extensions(
            &self.extensions,
            &[],
            "unknown required Terminal catalogue search extension",
        )
    }
}

impl Encode for CatalogSearch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.max_results);
        put_string_u32(out, &self.query)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for CatalogSearch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            max_results: decoder.u32()?,
            query: decoder.string_u32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CwdQuery {
    pub terminal_handle: u64,
    pub generation: u32,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for CwdQuery {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_identity(self.terminal_handle, self.generation)?;
        put_u64(out, self.terminal_handle);
        put_u32(out, self.generation);
        put_u32(out, 0);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for CwdQuery {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let generation = decoder.u32()?;
        if decoder.u32()? != 0 {
            return Err(Error::Invalid("Terminal CWD reserved field"));
        }
        let value = Self {
            terminal_handle,
            generation,
            initial_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        validate_query_identity(value.terminal_handle, value.generation)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Journal {
    pub terminal_handle: u64,
    pub generation: u32,
    pub flags: u16,
    pub limit: u16,
    pub from_index: u64,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Journal {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_identity(self.terminal_handle, self.generation)?;
        if self.flags & !(crate::schema::terminal::JOURNAL_REQUEST_FLAGS as u16) != 0
            || self.limit == 0
        {
            return Err(Error::Invalid("zero Terminal journal limit"));
        }
        put_u64(out, self.terminal_handle);
        put_u32(out, self.generation);
        put_u16(out, self.flags);
        put_u16(out, self.limit);
        put_u64(out, self.from_index);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Journal {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            terminal_handle: decoder.u64()?,
            generation: decoder.u32()?,
            flags: decoder.u16()?,
            limit: decoder.u16()?,
            from_index: decoder.u64()?,
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
pub struct Output {
    pub terminal_handle: u64,
    pub generation: u32,
    pub cursor_kind: u8,
    pub flags: u8,
    pub cursor_a: u64,
    pub cursor_b: u32,
    pub max_bytes: u32,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Output {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_identity(self.terminal_handle, self.generation)?;
        if self.cursor_kind > crate::schema::terminal::OUTPUT_CURSOR_PROBE as u8
            || self.flags != crate::schema::terminal::OUTPUT_REQUEST_FLAGS as u8
            || (self.cursor_kind == crate::schema::terminal::OUTPUT_CURSOR_LATEST_COMMAND as u8
                && self.cursor_a != 0)
        {
            return Err(Error::Invalid("Terminal OUTPUT cursor or flags"));
        }
        if self.max_bytes == 0 {
            return Err(Error::Invalid("zero Terminal query byte limit"));
        }
        put_u64(out, self.terminal_handle);
        put_u32(out, self.generation);
        out.push(self.cursor_kind);
        out.push(self.flags);
        put_u16(out, 0);
        put_u64(out, self.cursor_a);
        put_u32(out, self.cursor_b);
        put_u32(out, self.max_bytes);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Output {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let generation = decoder.u32()?;
        let cursor_kind = decoder.u8()?;
        let flags = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Terminal OUTPUT reserved field"));
        }
        let value = Self {
            terminal_handle,
            generation,
            cursor_kind,
            flags,
            cursor_a: decoder.u64()?,
            cursor_b: decoder.u32()?,
            max_bytes: decoder.u32()?,
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
pub struct Wait {
    pub terminal_handle: u64,
    pub generation: u32,
    pub wait_kind: u8,
    pub flags: u8,
    pub cursor_a: u64,
    pub cursor_b: u32,
    pub max_bytes: u32,
    pub timeout_ns: u64,
    pub needle: Vec<u8>,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Wait {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_identity(self.terminal_handle, self.generation)?;
        if self.wait_kind > crate::schema::terminal::WAIT_LATEST_COMMAND as u8
            || self.flags != crate::schema::terminal::WAIT_FLAGS as u8
            || self.max_bytes == 0
            || self.timeout_ns == 0
            || self.needle.len() > MAX_INPUT_BYTES
        {
            return Err(Error::Invalid("Terminal WAIT bounds"));
        }
        match self.wait_kind {
            kind if kind == crate::schema::terminal::WAIT_OUTPUT as u8 => {
                if self.needle.is_empty() {
                    return Err(Error::Invalid("empty Terminal WAIT output needle"));
                }
            }
            kind if kind == crate::schema::terminal::WAIT_COMMAND as u8 => {
                if self.cursor_b != 0 || !self.needle.is_empty() {
                    return Err(Error::Invalid("Terminal WAIT command cursor"));
                }
            }
            kind if kind == crate::schema::terminal::WAIT_LATEST_COMMAND as u8 => {
                if self.cursor_a != 0 || self.cursor_b != 0 || !self.needle.is_empty() {
                    return Err(Error::Invalid("Terminal WAIT latest-command cursor"));
                }
            }
            _ => unreachable!(),
        }
        put_u64(out, self.terminal_handle);
        put_u32(out, self.generation);
        out.push(self.wait_kind);
        out.push(self.flags);
        put_u16(out, 0);
        put_u64(out, self.cursor_a);
        put_u32(out, self.cursor_b);
        put_u32(out, self.max_bytes);
        put_u64(out, self.timeout_ns);
        put_bytes_u32(out, &self.needle)?;
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Wait {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let generation = decoder.u32()?;
        let wait_kind = decoder.u8()?;
        let flags = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Terminal WAIT reserved field"));
        }
        let value = Self {
            terminal_handle,
            generation,
            wait_kind,
            flags,
            cursor_a: decoder.u64()?,
            cursor_b: decoder.u32()?,
            max_bytes: decoder.u32()?,
            timeout_ns: decoder.u64()?,
            needle: decoder.len_bytes_u32()?.to_vec(),
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
pub struct CopyRange {
    pub terminal_handle: u64,
    pub generation: u32,
    pub representation: u8,
    pub start_row: i64,
    pub start_col: u32,
    pub end_row: i64,
    pub end_col: u32,
    pub max_bytes: u32,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for CopyRange {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_query_identity(self.terminal_handle, self.generation)?;
        validate_query_representation(self.representation)?;
        if self.max_bytes == 0 || (self.start_row == self.end_row && self.start_col > self.end_col)
        {
            return Err(Error::Invalid("zero Terminal query byte limit"));
        }
        put_u64(out, self.terminal_handle);
        put_u32(out, self.generation);
        out.push(self.representation);
        out.extend_from_slice(&[0; 3]);
        put_i64(out, self.start_row);
        put_u32(out, self.start_col);
        put_i64(out, self.end_row);
        put_u32(out, self.end_col);
        put_u32(out, self.max_bytes);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for CopyRange {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_handle = decoder.u64()?;
        let generation = decoder.u32()?;
        let representation = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Terminal COPY_RANGE reserved bytes"));
        }
        let value = Self {
            terminal_handle,
            generation,
            representation,
            start_row: decoder.i64()?,
            start_col: decoder.u32()?,
            end_row: decoder.i64()?,
            end_col: decoder.u32()?,
            max_bytes: decoder.u32()?,
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
pub enum QueryDelivery {
    Inline(Vec<u8>),
    Transfer(Descriptor),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueryCursor {
    pub kind: u8,
    pub a: u64,
    pub b: u32,
}

impl Encode for QueryCursor {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        out.push(self.kind);
        put_u64(out, self.a);
        put_u32(out, self.b);
        Ok(())
    }
}

impl Decode for QueryCursor {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            kind: decoder.u8()?,
            a: decoder.u64()?,
            b: decoder.u32()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryNextCursor {
    Read(QueryCursor),
    Search(QueryCursor),
    JournalIndex(u64),
    Output(QueryCursor),
}

impl QueryNextCursor {
    fn encode_for(self, content_kind: u8) -> Result<Vec<u8>> {
        match (content_kind, self) {
            (kind, Self::Read(cursor))
                if kind == crate::schema::terminal::CONTENT_TEXT as u8
                    || kind == crate::schema::terminal::CONTENT_STYLED_LINES as u8
                    || kind == crate::schema::terminal::CONTENT_TEXT_AND_STYLED as u8 =>
            {
                if cursor.kind > crate::schema::terminal::READ_CURSOR_TAIL as u8 {
                    return Err(Error::Invalid("Terminal READ next cursor"));
                }
                cursor.encode()
            }
            (kind, Self::Search(cursor))
                if kind == crate::schema::terminal::CONTENT_SEARCH_RESULTS as u8 =>
            {
                if cursor.kind != crate::schema::terminal::SEARCH_CURSOR_POSITION as u8 {
                    return Err(Error::Invalid("Terminal SEARCH next cursor"));
                }
                cursor.encode()
            }
            (kind, Self::JournalIndex(index))
                if kind == crate::schema::terminal::CONTENT_JOURNAL as u8 =>
            {
                Ok(index.to_le_bytes().to_vec())
            }
            (kind, Self::Output(cursor))
                if kind == crate::schema::terminal::CONTENT_OUTPUT as u8 =>
            {
                if cursor.kind != crate::schema::terminal::OUTPUT_CURSOR_SEQUENCE as u8 {
                    return Err(Error::Invalid("Terminal OUTPUT next cursor"));
                }
                cursor.encode()
            }
            _ => Err(Error::Invalid("Terminal next cursor content kind")),
        }
    }

    fn decode_for(content_kind: u8, input: &[u8]) -> Result<Self> {
        match content_kind {
            kind if kind == crate::schema::terminal::CONTENT_TEXT as u8
                || kind == crate::schema::terminal::CONTENT_STYLED_LINES as u8
                || kind == crate::schema::terminal::CONTENT_TEXT_AND_STYLED as u8 =>
            {
                let cursor = QueryCursor::decode(input)?;
                if cursor.kind > crate::schema::terminal::READ_CURSOR_TAIL as u8 {
                    return Err(Error::Invalid("Terminal READ next cursor"));
                }
                Ok(Self::Read(cursor))
            }
            kind if kind == crate::schema::terminal::CONTENT_SEARCH_RESULTS as u8 => {
                let cursor = QueryCursor::decode(input)?;
                if cursor.kind != crate::schema::terminal::SEARCH_CURSOR_POSITION as u8 {
                    return Err(Error::Invalid("Terminal SEARCH next cursor"));
                }
                Ok(Self::Search(cursor))
            }
            kind if kind == crate::schema::terminal::CONTENT_JOURNAL as u8 => {
                if input.len() != 8 {
                    return Err(Error::Invalid("Terminal JOURNAL next cursor"));
                }
                Ok(Self::JournalIndex(u64::from_le_bytes(
                    input.try_into().unwrap(),
                )))
            }
            kind if kind == crate::schema::terminal::CONTENT_OUTPUT as u8 => {
                let cursor = QueryCursor::decode(input)?;
                if cursor.kind != crate::schema::terminal::OUTPUT_CURSOR_SEQUENCE as u8 {
                    return Err(Error::Invalid("Terminal OUTPUT next cursor"));
                }
                Ok(Self::Output(cursor))
            }
            _ => Err(Error::Invalid("Terminal next cursor content kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryBody {
    pub content_kind: u8,
    pub encoding: u8,
    pub flags: u16,
    pub delivery: QueryDelivery,
    pub next_cursor: Option<QueryNextCursor>,
    pub total_lines: Option<u64>,
    pub satisfying_state_revision: Option<u64>,
    /// Unknown optional extensions. Tags 1 through 5 are reserved by the
    /// typed fields above and are rejected here.
    pub extensions: Extensions,
}

impl QueryBody {
    pub fn validate_receive_credit(&self, initial_receive_credit: u64) -> Result<()> {
        self.validate()?;
        if let QueryDelivery::Transfer(descriptor) = &self.delivery
            && (initial_receive_credit == 0
                || descriptor.sender_send_credit > initial_receive_credit)
        {
            return Err(Error::Invalid("Terminal query Transfer credit proposal"));
        }
        Ok(())
    }

    fn validate(&self) -> Result<()> {
        let expected_encoding = match self.content_kind {
            kind if kind == crate::schema::terminal::CONTENT_TEXT as u8 => {
                crate::schema::terminal::QUERY_ENCODING_UTF8 as u8
            }
            kind if kind == crate::schema::terminal::CONTENT_PATH as u8 => {
                crate::schema::terminal::QUERY_ENCODING_BYTES as u8
            }
            kind if kind == crate::schema::terminal::CONTENT_STYLED_LINES as u8
                || kind == crate::schema::terminal::CONTENT_SEARCH_RESULTS as u8
                || kind == crate::schema::terminal::CONTENT_JOURNAL as u8
                || kind == crate::schema::terminal::CONTENT_OUTPUT as u8
                || kind == crate::schema::terminal::CONTENT_TEXT_AND_STYLED as u8 =>
            {
                crate::schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8
            }
            _ => return Err(Error::Invalid("Terminal query content kind")),
        };
        if self.encoding != expected_encoding
            || self.flags & !(crate::schema::terminal::QUERY_TRUNCATED as u16) != 0
        {
            return Err(Error::Invalid("Terminal query encoding or flags"));
        }
        if let Some(cursor) = self.next_cursor {
            cursor.encode_for(self.content_kind)?;
        }
        if self.content_kind == crate::schema::terminal::CONTENT_PATH as u8
            && (self.next_cursor.is_some() || self.total_lines.is_some())
        {
            return Err(Error::Invalid("Terminal PATH query metadata"));
        }
        if self.satisfying_state_revision == Some(0) {
            return Err(Error::Invalid("zero Terminal satisfying revision"));
        }
        if self.extensions.0.iter().any(|entry| {
            matches!(
                entry.tag,
                tag if tag == crate::schema::terminal::QUERY_EXTENSION_INLINE_BYTES as u16
                    || tag == crate::schema::terminal::QUERY_EXTENSION_TRANSFER as u16
                    || tag == crate::schema::terminal::QUERY_EXTENSION_NEXT_CURSOR as u16
                    || tag == crate::schema::terminal::QUERY_EXTENSION_TOTAL_LINES as u16
                    || tag
                        == crate::schema::terminal::QUERY_EXTENSION_SATISFYING_STATE_REVISION
                            as u16
            )
        }) {
            return Err(Error::Invalid("typed Terminal query extension duplicated"));
        }
        reject_unknown_required_extensions(
            &self.extensions,
            &[],
            "unknown required Terminal query extension",
        )?;
        match &self.delivery {
            QueryDelivery::Inline(bytes) => {
                if bytes.len() > MAX_INLINE_QUERY_BYTES {
                    return Err(Error::LimitExceeded {
                        limit: "inline Terminal query",
                        actual: bytes.len() as u64,
                        maximum: MAX_INLINE_QUERY_BYTES as u64,
                    });
                }
                validate_query_inline(self.content_kind, bytes)
            }
            QueryDelivery::Transfer(descriptor) => validate_query_transfer(descriptor),
        }
    }

    fn wire_extensions(&self) -> Result<Extensions> {
        self.validate()?;
        let mut entries = self.extensions.0.clone();
        let (tag, bytes) = match &self.delivery {
            QueryDelivery::Inline(bytes) => (
                crate::schema::terminal::QUERY_EXTENSION_INLINE_BYTES as u16,
                bytes.clone(),
            ),
            QueryDelivery::Transfer(descriptor) => (
                crate::schema::terminal::QUERY_EXTENSION_TRANSFER as u16,
                descriptor.encode()?,
            ),
        };
        entries.push(Extension {
            tag,
            required: true,
            value: bytes,
        });
        if let Some(cursor) = &self.next_cursor {
            entries.push(Extension {
                tag: crate::schema::terminal::QUERY_EXTENSION_NEXT_CURSOR as u16,
                required: false,
                value: cursor.encode_for(self.content_kind)?,
            });
        }
        if let Some(total_lines) = self.total_lines {
            entries.push(Extension {
                tag: crate::schema::terminal::QUERY_EXTENSION_TOTAL_LINES as u16,
                required: false,
                value: total_lines.to_le_bytes().to_vec(),
            });
        }
        if let Some(revision) = self.satisfying_state_revision {
            entries.push(Extension {
                tag: crate::schema::terminal::QUERY_EXTENSION_SATISFYING_STATE_REVISION as u16,
                required: false,
                value: revision.to_le_bytes().to_vec(),
            });
        }
        entries.sort_by_key(|entry| entry.tag);
        let extensions = Extensions(entries);
        extensions.validate()?;
        Ok(extensions)
    }
}

impl Encode for QueryBody {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        out.push(match self.delivery {
            QueryDelivery::Inline(_) => crate::schema::terminal::QUERY_INLINE as u8,
            QueryDelivery::Transfer(_) => crate::schema::terminal::QUERY_TRANSFER as u8,
        });
        out.push(self.content_kind);
        out.push(self.encoding);
        out.push(0);
        put_u16(out, self.flags);
        put_u16(out, 0);
        self.wire_extensions()?.encode_tail(out)
    }
}

impl Decode for QueryBody {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let representation = decoder.u8()?;
        let content_kind = decoder.u8()?;
        let encoding = decoder.u8()?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Terminal query reserved byte"));
        }
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Terminal query reserved field"));
        }
        let mut wire = decoder.extensions()?.0;
        decoder.finish()?;
        let expected_tag = match representation {
            value if value == crate::schema::terminal::QUERY_INLINE as u8 => {
                crate::schema::terminal::QUERY_EXTENSION_INLINE_BYTES as u16
            }
            value if value == crate::schema::terminal::QUERY_TRANSFER as u8 => {
                crate::schema::terminal::QUERY_EXTENSION_TRANSFER as u16
            }
            _ => return Err(Error::Invalid("Terminal query representation")),
        };
        let position = wire
            .iter()
            .position(|entry| entry.tag == expected_tag)
            .ok_or(Error::Invalid("missing Terminal query delivery"))?;
        let delivery_extension = wire.remove(position);
        if !delivery_extension.required {
            return Err(Error::Invalid("Terminal query delivery is optional"));
        }
        let other_delivery =
            if expected_tag == crate::schema::terminal::QUERY_EXTENSION_INLINE_BYTES as u16 {
                crate::schema::terminal::QUERY_EXTENSION_TRANSFER as u16
            } else {
                crate::schema::terminal::QUERY_EXTENSION_INLINE_BYTES as u16
            };
        if wire.iter().any(|entry| entry.tag == other_delivery) {
            return Err(Error::Invalid("multiple Terminal query deliveries"));
        }
        let delivery = if representation == crate::schema::terminal::QUERY_INLINE as u8 {
            if delivery_extension.value.len() > MAX_INLINE_QUERY_BYTES {
                return Err(Error::LimitExceeded {
                    limit: "inline Terminal query",
                    actual: delivery_extension.value.len() as u64,
                    maximum: MAX_INLINE_QUERY_BYTES as u64,
                });
            }
            QueryDelivery::Inline(delivery_extension.value)
        } else {
            QueryDelivery::Transfer(Descriptor::decode(&delivery_extension.value)?)
        };
        let next_cursor = wire
            .iter()
            .position(|entry| {
                entry.tag == crate::schema::terminal::QUERY_EXTENSION_NEXT_CURSOR as u16
            })
            .map(|position| QueryNextCursor::decode_for(content_kind, &wire.remove(position).value))
            .transpose()?;
        let total_lines = if let Some(position) = wire.iter().position(|entry| {
            entry.tag == crate::schema::terminal::QUERY_EXTENSION_TOTAL_LINES as u16
        }) {
            let value = wire.remove(position).value;
            if value.len() != 8 {
                return Err(Error::Invalid("Terminal total-lines extension"));
            }
            Some(u64::from_le_bytes(value.try_into().unwrap()))
        } else {
            None
        };
        let satisfying_state_revision = if let Some(position) = wire.iter().position(|entry| {
            entry.tag == crate::schema::terminal::QUERY_EXTENSION_SATISFYING_STATE_REVISION as u16
        }) {
            let value = wire.remove(position).value;
            if value.len() != 8 {
                return Err(Error::Invalid("Terminal satisfying-revision extension"));
            }
            let revision = u64::from_le_bytes(value.try_into().unwrap());
            if revision == 0 {
                return Err(Error::Invalid("zero Terminal satisfying revision"));
            }
            Some(revision)
        } else {
            None
        };
        let value = Self {
            content_kind,
            encoding,
            flags,
            delivery,
            next_cursor,
            total_lines,
            satisfying_state_revision,
            extensions: Extensions(wire),
        };
        value.validate()?;
        Ok(value)
    }
}

fn validate_query_transfer(descriptor: &Descriptor) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Byte
        || descriptor.direction != Direction::SENDER_TO_RECEIVER
        || descriptor.content_family != crate::family::TERMINAL
        || descriptor.content_kind != crate::schema::terminal::QUERY_CONTENT_KIND as u16
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("Terminal query Transfer descriptor"));
    }
    Ok(())
}

fn validate_query_inline(content_kind: u8, bytes: &[u8]) -> Result<()> {
    match content_kind {
        kind if kind == crate::schema::terminal::CONTENT_TEXT as u8 => {
            core::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8)?;
        }
        kind if kind == crate::schema::terminal::CONTENT_PATH as u8 => {}
        kind if kind == crate::schema::terminal::CONTENT_STYLED_LINES as u8 => {
            StyledLines::decode(bytes)?;
        }
        kind if kind == crate::schema::terminal::CONTENT_SEARCH_RESULTS as u8 => {
            SearchResults::decode(bytes)?;
        }
        kind if kind == crate::schema::terminal::CONTENT_JOURNAL as u8 => {
            JournalResult::decode(bytes)?;
        }
        kind if kind == crate::schema::terminal::CONTENT_OUTPUT as u8 => {
            OutputResult::decode(bytes)?;
        }
        kind if kind == crate::schema::terminal::CONTENT_TEXT_AND_STYLED as u8 => {
            TextAndStyled::decode(bytes)?;
        }
        _ => return Err(Error::Invalid("Terminal query content kind")),
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextAndStyled {
    pub plain: String,
    pub styled: StyledLines,
}

impl Encode for TextAndStyled {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        put_string_u32(out, &self.plain)?;
        put_bytes_u32(out, &self.styled.encode()?)
    }
}

impl Decode for TextAndStyled {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let plain = decoder.string_u32()?;
        let styled = StyledLines::decode(decoder.len_bytes_u32()?)?;
        decoder.finish()?;
        Ok(Self { plain, styled })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchMatch {
    pub start_row: u64,
    pub start_col: u32,
    pub end_row: u64,
    pub end_col: u32,
    pub preview: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchResults(pub Vec<SearchMatch>);

impl Encode for SearchResults {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.0.len() > MAX_QUERY_RECORDS {
            return Err(Error::Invalid("Terminal search result count"));
        }
        put_len_u32(out, self.0.len())?;
        for result in &self.0 {
            if (result.start_row, result.start_col) > (result.end_row, result.end_col) {
                return Err(Error::Invalid("Terminal search range"));
            }
            put_u64(out, result.start_row);
            put_u32(out, result.start_col);
            put_u64(out, result.end_row);
            put_u32(out, result.end_col);
            put_string_u32(out, &result.preview)?;
        }
        Ok(())
    }
}

impl Decode for SearchResults {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
        if count > MAX_QUERY_RECORDS || count > decoder.remaining() / 28 {
            return Err(Error::Invalid("Terminal search result count"));
        }
        let mut results = Vec::with_capacity(count);
        for _ in 0..count {
            let result = SearchMatch {
                start_row: decoder.u64()?,
                start_col: decoder.u32()?,
                end_row: decoder.u64()?,
                end_col: decoder.u32()?,
                preview: decoder.string_u32()?,
            };
            if (result.start_row, result.start_col) > (result.end_row, result.end_col) {
                return Err(Error::Invalid("Terminal search range"));
            }
            results.push(result);
        }
        decoder.finish()?;
        Ok(Self(results))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSearchEntry {
    pub terminal_handle: u64,
    pub generation: u32,
    pub score: u32,
    pub primary_source: u8,
    pub matched_sources: u8,
    /// Lines above the live viewport. Zero means that there is no scrollback
    /// jump for the primary result.
    pub scroll_offset: u64,
    pub context: String,
}

impl CatalogSearchEntry {
    fn validate(&self) -> Result<()> {
        validate_query_identity(self.terminal_handle, self.generation)?;
        if self.primary_source > crate::schema::terminal::CATALOG_SEARCH_SOURCE_SCROLLBACK as u8
            || self.matched_sources == 0
            || self.matched_sources & !(crate::schema::terminal::CATALOG_SEARCH_MATCH_MASK as u8)
                != 0
            || self.matched_sources & (1 << self.primary_source) == 0
            || self.context.len() > MAX_CATALOG_SEARCH_CONTEXT_BYTES
        {
            return Err(Error::Invalid("Terminal catalogue search entry"));
        }
        Ok(())
    }
}

impl Encode for CatalogSearchEntry {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.terminal_handle);
        put_u32(out, self.generation);
        put_u32(out, self.score);
        out.push(self.primary_source);
        out.push(self.matched_sources);
        put_u16(out, 0);
        put_u64(out, self.scroll_offset);
        put_string_u32(out, &self.context)
    }
}

impl Decode for CatalogSearchEntry {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = decode_catalog_search_entry(&mut decoder)?;
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

fn decode_catalog_search_entry(decoder: &mut Decoder<'_>) -> Result<CatalogSearchEntry> {
    let terminal_handle = decoder.u64()?;
    let generation = decoder.u32()?;
    let score = decoder.u32()?;
    let primary_source = decoder.u8()?;
    let matched_sources = decoder.u8()?;
    if decoder.u16()? != 0 {
        return Err(Error::Invalid(
            "Terminal catalogue search entry reserved field",
        ));
    }
    let value = CatalogSearchEntry {
        terminal_handle,
        generation,
        score,
        primary_source,
        matched_sources,
        scroll_offset: decoder.u64()?,
        context: decoder.string_u32()?,
    };
    value.validate()?;
    Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSearchResult {
    pub flags: u16,
    pub entries: Vec<CatalogSearchEntry>,
    pub extensions: Extensions,
}

impl CatalogSearchResult {
    fn validate(&self) -> Result<()> {
        if self.flags & !(crate::schema::terminal::CATALOG_SEARCH_RESULT_FLAGS as u16) != 0
            || self.entries.len() > MAX_QUERY_RECORDS
        {
            return Err(Error::Invalid("Terminal catalogue search result"));
        }
        reject_unknown_required_extensions(
            &self.extensions,
            &[],
            "unknown required Terminal catalogue search result extension",
        )?;
        for entry in &self.entries {
            entry.validate()?;
        }
        Ok(())
    }
}

impl Encode for CatalogSearchResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_len_u32(out, self.entries.len())?;
        for entry in &self.entries {
            entry.encode_to(out)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for CatalogSearchResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid(
                "Terminal catalogue search result reserved field",
            ));
        }
        let count = usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
        if count > MAX_QUERY_RECORDS || count > decoder.remaining() / 32 {
            return Err(Error::Invalid("Terminal catalogue search result count"));
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            entries.push(decode_catalog_search_entry(&mut decoder)?);
        }
        let value = Self {
            flags,
            entries,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalRecord {
    pub index: u64,
    pub generation: u32,
    pub flags: u16,
    pub exit_code: i32,
    pub start_seq: u64,
    pub end_seq: u64,
    pub started_unix_ms: u64,
    pub ended_unix_ms: u64,
    pub command: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalResult {
    pub oldest_index: u64,
    pub next_index: u64,
    pub records: Vec<JournalRecord>,
}

const JOURNAL_FLAGS: u16 = crate::schema::terminal::JOURNAL_RUNNING as u16
    | crate::schema::terminal::JOURNAL_HAS_EXIT as u16
    | crate::schema::terminal::JOURNAL_NO_COMMAND as u16
    | crate::schema::terminal::JOURNAL_INCOMPLETE as u16
    | crate::schema::terminal::JOURNAL_EVICTED as u16
    | crate::schema::terminal::JOURNAL_PTY_EXITED as u16;

fn validate_journal_record(record: &JournalRecord) -> Result<()> {
    if record.generation == 0
        || record.flags & !JOURNAL_FLAGS != 0
        || record.start_seq > record.end_seq
        || record.started_unix_ms > record.ended_unix_ms && record.ended_unix_ms != 0
        || (record.flags & crate::schema::terminal::JOURNAL_NO_COMMAND as u16 != 0)
            != record.command.is_empty()
    {
        return Err(Error::Invalid("Terminal journal record"));
    }
    Ok(())
}

impl Encode for JournalResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.oldest_index > self.next_index || self.records.len() > MAX_QUERY_RECORDS {
            return Err(Error::Invalid("Terminal journal bounds"));
        }
        put_u64(out, self.oldest_index);
        put_u64(out, self.next_index);
        put_len_u32(out, self.records.len())?;
        let mut previous = None;
        for record in &self.records {
            validate_journal_record(record)?;
            if previous.is_some_and(|index| index >= record.index)
                || record.index < self.oldest_index
                || record.index >= self.next_index
            {
                return Err(Error::Invalid("Terminal journal record order"));
            }
            previous = Some(record.index);
            put_u64(out, record.index);
            put_u32(out, record.generation);
            put_u16(out, record.flags);
            put_u16(out, 0);
            put_i32(out, record.exit_code);
            put_u64(out, record.start_seq);
            put_u64(out, record.end_seq);
            put_u64(out, record.started_unix_ms);
            put_u64(out, record.ended_unix_ms);
            put_string_u32(out, &record.command)?;
        }
        Ok(())
    }
}

impl Decode for JournalResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let oldest_index = decoder.u64()?;
        let next_index = decoder.u64()?;
        let count = usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
        if count > MAX_QUERY_RECORDS || count > decoder.remaining() / 56 {
            return Err(Error::Invalid("Terminal journal record count"));
        }
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            let index = decoder.u64()?;
            let generation = decoder.u32()?;
            let flags = decoder.u16()?;
            if decoder.u16()? != 0 {
                return Err(Error::Invalid("Terminal journal reserved field"));
            }
            records.push(JournalRecord {
                index,
                generation,
                flags,
                exit_code: decoder.i32()?,
                start_seq: decoder.u64()?,
                end_seq: decoder.u64()?,
                started_unix_ms: decoder.u64()?,
                ended_unix_ms: decoder.u64()?,
                command: decoder.string_u32()?,
            });
        }
        decoder.finish()?;
        let value = Self {
            oldest_index,
            next_index,
            records,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputResult {
    pub generation: u32,
    pub flags: u16,
    pub start_seq: u64,
    pub start_col: u32,
    pub next_seq: u64,
    pub next_col: u32,
    pub text: Vec<u8>,
}

const OUTPUT_FLAGS: u16 = crate::schema::terminal::OUTPUT_TRUNCATED as u16
    | crate::schema::terminal::OUTPUT_EVICTED as u16
    | crate::schema::terminal::OUTPUT_ALT_SCREEN as u16
    | crate::schema::terminal::OUTPUT_MATCHED as u16;

impl Encode for OutputResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.generation == 0
            || self.flags & !OUTPUT_FLAGS != 0
            || (self.start_seq, self.start_col) > (self.next_seq, self.next_col)
        {
            return Err(Error::Invalid("Terminal output result"));
        }
        put_u32(out, self.generation);
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_u64(out, self.start_seq);
        put_u32(out, self.start_col);
        put_u64(out, self.next_seq);
        put_u32(out, self.next_col);
        put_bytes_u32(out, &self.text)
    }
}

impl Decode for OutputResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let generation = decoder.u32()?;
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Terminal output reserved field"));
        }
        let value = Self {
            generation,
            flags,
            start_seq: decoder.u64()?,
            start_col: decoder.u32()?,
            next_seq: decoder.u64()?,
            next_col: decoder.u32()?,
            text: decoder.len_bytes_u32()?.to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledOverflow {
    pub cell_offset: u32,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledHyperlink {
    pub start_col: u32,
    pub cell_count: u32,
    pub uri: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledLine {
    pub row: i64,
    pub start_col: u32,
    pub cells: Vec<Cell>,
    pub overflow: Vec<StyledOverflow>,
    pub hyperlinks: Vec<StyledHyperlink>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyledLines(pub Vec<StyledLine>);

fn validate_styled_line(line: &StyledLine) -> Result<()> {
    let cell_count = u32::try_from(line.cells.len()).map_err(|_| Error::LengthOverflow)?;
    let line_end = line
        .start_col
        .checked_add(cell_count)
        .ok_or(Error::LengthOverflow)?;
    let mut previous = None;
    for overflow in &line.overflow {
        if overflow.cell_offset >= cell_count
            || previous.is_some_and(|offset| offset >= overflow.cell_offset)
        {
            return Err(Error::Invalid("Terminal styled overflow order"));
        }
        previous = Some(overflow.cell_offset);
    }
    let mut end = line.start_col;
    for link in &line.hyperlinks {
        if link.cell_count == 0
            || link.start_col < end
            || link
                .start_col
                .checked_add(link.cell_count)
                .is_none_or(|next| next > line_end)
            || link.uri.len() > MAX_HYPERLINK_URI_BYTES
        {
            return Err(Error::Invalid("Terminal styled hyperlink"));
        }
        end = link.start_col + link.cell_count;
    }
    Ok(())
}

impl Encode for StyledLines {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.0.len() > MAX_QUERY_RECORDS {
            return Err(Error::Invalid("Terminal styled line count"));
        }
        put_len_u32(out, self.0.len())?;
        let mut previous_row = None;
        for line in &self.0 {
            if previous_row.is_some_and(|row| row >= line.row) {
                return Err(Error::Invalid("Terminal styled line order"));
            }
            previous_row = Some(line.row);
            validate_styled_line(line)?;
            put_i64(out, line.row);
            put_u32(out, line.start_col);
            put_len_u32(out, line.cells.len())?;
            for cell in &line.cells {
                out.extend_from_slice(cell);
            }
            put_len_u32(out, line.overflow.len())?;
            for overflow in &line.overflow {
                put_u32(out, overflow.cell_offset);
                put_string_u32(out, &overflow.text)?;
            }
            put_len_u32(out, line.hyperlinks.len())?;
            for link in &line.hyperlinks {
                put_u32(out, link.start_col);
                put_u32(out, link.cell_count);
                put_bytes_u16(out, link.uri.as_bytes())?;
            }
        }
        Ok(())
    }
}

impl Decode for StyledLines {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
        if count > MAX_QUERY_RECORDS || count > decoder.remaining() / 24 {
            return Err(Error::Invalid("Terminal styled line count"));
        }
        let mut lines = Vec::with_capacity(count);
        for _ in 0..count {
            let row = decoder.i64()?;
            let start_col = decoder.u32()?;
            let cell_count = usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
            let cells_bytes = decoder.take(
                cell_count
                    .checked_mul(CELL_BYTES)
                    .ok_or(Error::LengthOverflow)?,
            )?;
            let cells = cells_bytes.as_chunks::<CELL_BYTES>().0.to_vec();
            let overflow_count =
                usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
            if overflow_count > MAX_QUERY_RECORDS || overflow_count > decoder.remaining() / 8 {
                return Err(Error::Invalid("Terminal styled overflow count"));
            }
            let mut overflow = Vec::with_capacity(overflow_count);
            for _ in 0..overflow_count {
                overflow.push(StyledOverflow {
                    cell_offset: decoder.u32()?,
                    text: decoder.string_u32()?,
                });
            }
            let hyperlink_count =
                usize::try_from(decoder.u32()?).map_err(|_| Error::LengthOverflow)?;
            if hyperlink_count > MAX_QUERY_RECORDS || hyperlink_count > decoder.remaining() / 10 {
                return Err(Error::Invalid("Terminal styled hyperlink count"));
            }
            let mut hyperlinks = Vec::with_capacity(hyperlink_count);
            for _ in 0..hyperlink_count {
                hyperlinks.push(StyledHyperlink {
                    start_col: decoder.u32()?,
                    cell_count: decoder.u32()?,
                    uri: decoder.string_u16()?,
                });
            }
            let line = StyledLine {
                row,
                start_col,
                cells,
                overflow,
                hyperlinks,
            };
            validate_styled_line(&line)?;
            lines.push(line);
        }
        decoder.finish()?;
        let value = Self(lines);
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewFeedback {
    pub view_id: u32,
    pub presented_sequence: u32,
    pub decoder_queue_depth: u8,
    pub available_frame_slots: u8,
}

impl ViewFeedback {
    fn encode_into(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_view(self.view_id)?;
        put_u32(out, self.view_id);
        put_u32(out, self.presented_sequence);
        out.push(self.decoder_queue_depth);
        out.push(self.available_frame_slots);
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            view_id: decoder.u32()?,
            presented_sequence: decoder.u32()?,
            decoder_queue_depth: decoder.u8()?,
            available_frame_slots: decoder.u8()?,
        };
        nonzero_view(value.view_id)?;
        Ok(value)
    }
}

impl Encode for ViewFeedback {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_into(out)
    }
}

impl Decode for ViewFeedback {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub terminal_handle: u64,
    pub data: Vec<u8>,
}

impl Encode for Write {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_handle(self.terminal_handle, "zero terminal handle")?;
        if self.data.is_empty() || self.data.len() > MAX_INPUT_BYTES {
            return Err(Error::Invalid("Terminal WRITE data length"));
        }
        put_u64(out, self.terminal_handle);
        out.extend_from_slice(&self.data);
        Ok(())
    }
}

impl Decode for Write {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            terminal_handle: decoder.u64()?,
            data: decoder.rest().to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub feedback: ViewFeedback,
    pub data: Vec<u8>,
}

impl Encode for Input {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.data.is_empty() || self.data.len() > MAX_INPUT_BYTES {
            return Err(Error::Invalid("Terminal INPUT data length"));
        }
        self.feedback.encode_into(out)?;
        out.extend_from_slice(&self.data);
        Ok(())
    }
}

impl Decode for Input {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            feedback: ViewFeedback::decode_from(&mut decoder)?,
            data: decoder.rest().to_vec(),
        };
        decoder.finish()?;
        if value.data.is_empty() || value.data.len() > MAX_INPUT_BYTES {
            return Err(Error::Invalid("Terminal INPUT data length"));
        }
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mouse {
    pub feedback: ViewFeedback,
    pub client_monotonic_ns: u64,
    pub action: u8,
    pub button: u8,
    pub modifiers: u16,
    pub column: i32,
    pub row: i32,
}

impl Encode for Mouse {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.feedback.encode_into(out)?;
        put_u64(out, self.client_monotonic_ns);
        out.push(self.action);
        out.push(self.button);
        put_u16(out, self.modifiers);
        put_i32(out, self.column);
        put_i32(out, self.row);
        Ok(())
    }
}

impl Decode for Mouse {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            feedback: ViewFeedback::decode_from(&mut decoder)?,
            client_monotonic_ns: decoder.u64()?,
            action: decoder.u8()?,
            button: decoder.u8()?,
            modifiers: decoder.u16()?,
            column: decoder.i32()?,
            row: decoder.i32()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wheel {
    pub feedback: ViewFeedback,
    pub client_monotonic_ns: u64,
    pub source: u8,
    pub dx_32_32: i64,
    pub dy_32_32: i64,
}

impl Encode for Wheel {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.feedback.encode_into(out)?;
        put_u64(out, self.client_monotonic_ns);
        out.push(self.source);
        out.extend_from_slice(&[0; 3]);
        put_i64(out, self.dx_32_32);
        put_i64(out, self.dy_32_32);
        Ok(())
    }
}

impl Decode for Wheel {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let feedback = ViewFeedback::decode_from(&mut decoder)?;
        let client_monotonic_ns = decoder.u64()?;
        let source = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Terminal WHEEL reserved bytes"));
        }
        let value = Self {
            feedback,
            client_monotonic_ns,
            source,
            dx_32_32: decoder.i64()?,
            dy_32_32: decoder.i64()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

/// A wheel turn that knows where the pointer was.
///
/// `WHEEL` carries no position, so every report it produces lands on the
/// origin cell — which is the wrong pane in anything that splits its window.
/// Peers that advertise this event get the cell; the older one stays exactly
/// as it was for peers that do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WheelAt {
    pub feedback: ViewFeedback,
    pub client_monotonic_ns: u64,
    pub source: u8,
    pub dx_32_32: i64,
    pub dy_32_32: i64,
    pub column: i32,
    pub row: i32,
}

impl Encode for WheelAt {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.feedback.encode_into(out)?;
        put_u64(out, self.client_monotonic_ns);
        out.push(self.source);
        out.extend_from_slice(&[0; 3]);
        put_i64(out, self.dx_32_32);
        put_i64(out, self.dy_32_32);
        put_i32(out, self.column);
        put_i32(out, self.row);
        Ok(())
    }
}

impl Decode for WheelAt {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let feedback = ViewFeedback::decode_from(&mut decoder)?;
        let client_monotonic_ns = decoder.u64()?;
        let source = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Terminal WHEEL_AT reserved bytes"));
        }
        let value = Self {
            feedback,
            client_monotonic_ns,
            source,
            dx_32_32: decoder.i64()?,
            dy_32_32: decoder.i64()?,
            column: decoder.i32()?,
            row: decoder.i32()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalFrame {
    pub view_id: u32,
    pub frame_sequence: u32,
    pub frame_flags: u16,
    pub base_sequence: Option<u32>,
    /// Raw codec-specific grid payload (including the codec-local LZ4 wrapper
    /// when `FRAME_CODEC_COMPRESSED` is set).
    pub grid_payload: Vec<u8>,
}

impl TerminalFrame {
    pub const KNOWN_FLAGS: u16 = crate::schema::terminal::FRAME_KEYFRAME as u16
        | crate::schema::terminal::FRAME_FINAL_STATE as u16
        | crate::schema::terminal::FRAME_DIMENSIONS as u16
        | crate::schema::terminal::FRAME_CURSOR as u16
        | crate::schema::terminal::FRAME_MODES as u16
        | crate::schema::terminal::FRAME_SCROLLBACK as u16
        | crate::schema::terminal::FRAME_VIEW_OFFSET as u16
        | crate::schema::terminal::FRAME_TITLE as u16
        | crate::schema::terminal::FRAME_COMPONENTS as u16
        | crate::schema::terminal::FRAME_CODEC_COMPRESSED as u16
        | crate::schema::terminal::FRAME_EXPLICIT_BASE as u16;

    fn validate_logical(&self) -> Result<usize> {
        nonzero_view(self.view_id)?;
        if self.frame_flags & !Self::KNOWN_FLAGS != 0 || self.grid_payload.is_empty() {
            return Err(Error::Invalid("Terminal FRAME flags or payload"));
        }
        let explicit = self.frame_flags & crate::schema::terminal::FRAME_EXPLICIT_BASE as u16 != 0;
        let keyframe = self.frame_flags & crate::schema::terminal::FRAME_KEYFRAME as u16 != 0;
        if explicit != self.base_sequence.is_some() || (explicit && keyframe) {
            return Err(Error::Invalid("Terminal FRAME base"));
        }
        if keyframe {
            let required = crate::schema::terminal::FRAME_DIMENSIONS as u16
                | crate::schema::terminal::FRAME_CURSOR as u16
                | crate::schema::terminal::FRAME_MODES as u16
                | crate::schema::terminal::FRAME_SCROLLBACK as u16
                | crate::schema::terminal::FRAME_VIEW_OFFSET as u16
                | crate::schema::terminal::FRAME_TITLE as u16;
            if self.frame_flags & required != required {
                return Err(Error::Invalid("incomplete Terminal keyframe"));
            }
        } else if self.frame_flags & crate::schema::terminal::FRAME_DIMENSIONS as u16 != 0 {
            return Err(Error::Invalid("dimensions on Terminal delta"));
        }
        let logical_len = 2usize
            .checked_add(usize::from(explicit) * 4)
            .and_then(|length| length.checked_add(self.grid_payload.len()))
            .ok_or(Error::LengthOverflow)?;
        if logical_len > crate::frame::HARD_MAX_DECODED_FRAME as usize {
            return Err(Error::LimitExceeded {
                limit: "encoded Terminal logical frame",
                actual: logical_len as u64,
                maximum: u64::from(crate::frame::HARD_MAX_DECODED_FRAME),
            });
        }
        Ok(logical_len)
    }

    fn validate(&self) -> Result<()> {
        self.validate_logical()?;
        if self.grid_payload.len() > crate::frame::HARD_MAX_BULK_CHUNK as usize {
            return Err(Error::Invalid("Terminal FRAME flags or payload"));
        }
        Ok(())
    }

    /// Encode the logical body fragmented by `FRAME_CHUNK`: flags, optional
    /// explicit base, and the codec-specific grid payload. Unlike ordinary
    /// `FRAME` encoding, this may exceed one canonical bulk chunk.
    pub fn encode_logical_body(&self) -> Result<Vec<u8>> {
        let logical_len = self.validate_logical()?;
        let mut out = Vec::with_capacity(logical_len);
        put_u16(&mut out, self.frame_flags);
        if let Some(base) = self.base_sequence {
            put_u32(&mut out, base);
        }
        out.extend_from_slice(&self.grid_payload);
        debug_assert_eq!(out.len(), logical_len);
        Ok(out)
    }

    pub fn decode_grid_codec1(
        &self,
        max_decoded_frame: u32,
        base_dimensions: Option<(u16, u16)>,
    ) -> Result<Grid> {
        Grid::decode_codec1(
            self.frame_flags,
            &self.grid_payload,
            max_decoded_frame,
            base_dimensions,
        )
    }
}

impl Encode for TerminalFrame {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.view_id);
        put_u32(out, self.frame_sequence);
        put_u16(out, self.frame_flags);
        if let Some(base) = self.base_sequence {
            put_u32(out, base);
        }
        out.extend_from_slice(&self.grid_payload);
        Ok(())
    }
}

impl Decode for TerminalFrame {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let view_id = decoder.u32()?;
        let frame_sequence = decoder.u32()?;
        let frame_flags = decoder.u16()?;
        let base_sequence =
            if frame_flags & crate::schema::terminal::FRAME_EXPLICIT_BASE as u16 != 0 {
                Some(decoder.u32()?)
            } else {
                None
            };
        let value = Self {
            view_id,
            frame_sequence,
            frame_flags,
            base_sequence,
            grid_payload: decoder.rest().to_vec(),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameChunk {
    pub view_id: u32,
    pub frame_sequence: u32,
    pub chunk_index: u16,
    pub chunk_count: u16,
    pub logical_frame_len: u32,
    pub chunk: Vec<u8>,
}

impl Encode for FrameChunk {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_view(self.view_id)?;
        if self.chunk_count == 0
            || self.chunk_index >= self.chunk_count
            || self.logical_frame_len == 0
            || self.chunk.is_empty()
            || self.chunk.len() > self.logical_frame_len as usize
            || self.chunk.len() > crate::frame::HARD_MAX_BULK_CHUNK as usize
        {
            return Err(Error::Invalid("Terminal FRAME_CHUNK coordinates"));
        }
        put_u32(out, self.view_id);
        put_u32(out, self.frame_sequence);
        put_u16(out, self.chunk_index);
        put_u16(out, self.chunk_count);
        put_u32(out, self.logical_frame_len);
        out.extend_from_slice(&self.chunk);
        Ok(())
    }
}

impl Decode for FrameChunk {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            view_id: decoder.u32()?,
            frame_sequence: decoder.u32()?,
            chunk_index: decoder.u16()?,
            chunk_count: decoder.u16()?,
            logical_frame_len: decoder.u32()?,
            chunk: decoder.rest().to_vec(),
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

pub type Cell = [u8; CELL_BYTES];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GridOperation {
    PatchRun {
        start_cell: u32,
        cells: Vec<Cell>,
    },
    PatchList {
        indices: Vec<u32>,
        cells: Vec<Cell>,
    },
    PatchBitmap {
        start_cell: u32,
        span: u32,
        bitmap: Vec<u8>,
        cells: Vec<Cell>,
    },
    CopyRect {
        src_row: u16,
        src_col: u16,
        dst_row: u16,
        dst_col: u16,
        rows: u16,
        cols: u16,
    },
    FillRect {
        row: u16,
        col: u16,
        rows: u16,
        cols: u16,
        cell: Cell,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub kind: u8,
    pub required: bool,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grid {
    pub dimensions: Option<(u16, u16)>,
    pub cursor: Option<(u16, u16)>,
    pub modes: Option<u16>,
    pub scrollback_lines: Option<u32>,
    pub scroll_offset: Option<i64>,
    pub title: Option<String>,
    pub operations: Vec<GridOperation>,
    pub components: Vec<Component>,
}

impl Grid {
    pub fn decode_codec1(
        frame_flags: u16,
        payload: &[u8],
        max_decoded_frame: u32,
        base_dimensions: Option<(u16, u16)>,
    ) -> Result<Self> {
        if frame_flags & !TerminalFrame::KNOWN_FLAGS != 0 {
            return Err(Error::Invalid("Terminal FRAME flags"));
        }
        let decoded_storage;
        let decoded = if has_flag(frame_flags, crate::schema::terminal::FRAME_CODEC_COMPRESSED) {
            let mut wrapper = Decoder::new(payload);
            let len = usize::try_from(wrapper.u32()?).map_err(|_| Error::LengthOverflow)?;
            if len > max_decoded_frame as usize {
                return Err(Error::LimitExceeded {
                    limit: "decoded Terminal grid",
                    actual: len as u64,
                    maximum: u64::from(max_decoded_frame),
                });
            }
            let block = wrapper.rest();
            if payload.len().checked_add(8).ok_or(Error::LengthOverflow)? > len {
                return Err(Error::Invalid("unprofitable Terminal grid compression"));
            }
            decoded_storage = decompress(block, len).map_err(|_| Error::Compression)?;
            decoded_storage.as_slice()
        } else {
            if payload.len() > max_decoded_frame as usize {
                return Err(Error::LimitExceeded {
                    limit: "decoded Terminal grid",
                    actual: payload.len() as u64,
                    maximum: u64::from(max_decoded_frame),
                });
            }
            payload
        };
        let mut decoder = Decoder::new(decoded);
        let dimensions = if has_flag(frame_flags, crate::schema::terminal::FRAME_DIMENSIONS) {
            Some((decoder.u16()?, decoder.u16()?))
        } else {
            None
        };
        let cursor = if has_flag(frame_flags, crate::schema::terminal::FRAME_CURSOR) {
            Some((decoder.u16()?, decoder.u16()?))
        } else {
            None
        };
        let modes = if has_flag(frame_flags, crate::schema::terminal::FRAME_MODES) {
            Some(decoder.u16()?)
        } else {
            None
        };
        let scrollback_lines = if has_flag(frame_flags, crate::schema::terminal::FRAME_SCROLLBACK) {
            Some(decoder.u32()?)
        } else {
            None
        };
        let scroll_offset = if has_flag(frame_flags, crate::schema::terminal::FRAME_VIEW_OFFSET) {
            Some(decoder.i64()?)
        } else {
            None
        };
        let title = if has_flag(frame_flags, crate::schema::terminal::FRAME_TITLE) {
            Some(decoder.string_u16()?)
        } else {
            None
        };
        let effective_dimensions = dimensions
            .or(base_dimensions)
            .ok_or(Error::Invalid("Terminal grid has no effective dimensions"))?;
        if effective_dimensions.0 == 0 || effective_dimensions.1 == 0 {
            return Err(Error::Invalid("Terminal grid dimensions"));
        }
        let operation_count =
            usize::try_from(get_uleb(&mut decoder)?).map_err(|_| Error::LengthOverflow)?;
        if operation_count > decoder.remaining() / 13 {
            return Err(Error::Invalid("Terminal grid operation count"));
        }
        let mut operations = Vec::with_capacity(operation_count);
        for _ in 0..operation_count {
            operations.push(decode_grid_operation(&mut decoder, effective_dimensions)?);
        }
        let mut components = Vec::new();
        if has_flag(frame_flags, crate::schema::terminal::FRAME_COMPONENTS) {
            let count =
                usize::try_from(get_uleb(&mut decoder)?).map_err(|_| Error::LengthOverflow)?;
            if count > usize::from(u8::MAX) + 1 || count > decoder.remaining() / 3 {
                return Err(Error::Invalid("Terminal component count"));
            }
            components.reserve(count);
            let mut previous = None;
            for _ in 0..count {
                let kind = decoder.u8()?;
                if previous.is_some_and(|old| old >= kind) {
                    return Err(Error::Invalid("Terminal component order"));
                }
                previous = Some(kind);
                let flags = decoder.u8()?;
                if flags & !(crate::schema::terminal::COMPONENT_REQUIRED as u8) != 0 {
                    return Err(Error::Invalid("Terminal component flags"));
                }
                let len =
                    usize::try_from(get_uleb(&mut decoder)?).map_err(|_| Error::LengthOverflow)?;
                let component = Component {
                    kind,
                    required: flags & crate::schema::terminal::COMPONENT_REQUIRED as u8 != 0,
                    body: decoder.take(len)?.to_vec(),
                };
                validate_component(&component, effective_dimensions)?;
                components.push(component);
            }
        }
        decoder.finish()?;
        let value = Self {
            dimensions,
            cursor,
            modes,
            scrollback_lines,
            scroll_offset,
            title,
            operations,
            components,
        };
        value.validate(frame_flags, base_dimensions)?;
        Ok(value)
    }

    pub fn encode_codec1(
        &self,
        frame_flags: u16,
        max_decoded_frame: u32,
        base_dimensions: Option<(u16, u16)>,
    ) -> Result<Vec<u8>> {
        let dimensions = self.validate(frame_flags, base_dimensions)?;
        let mut raw = Vec::new();
        if let Some((rows, cols)) = self.dimensions {
            put_u16(&mut raw, rows);
            put_u16(&mut raw, cols);
        }
        if let Some((row, col)) = self.cursor {
            put_u16(&mut raw, row);
            put_u16(&mut raw, col);
        }
        if let Some(modes) = self.modes {
            put_u16(&mut raw, modes);
        }
        if let Some(scrollback) = self.scrollback_lines {
            put_u32(&mut raw, scrollback);
        }
        if let Some(offset) = self.scroll_offset {
            put_i64(&mut raw, offset);
        }
        if let Some(title) = &self.title {
            put_bytes_u16(&mut raw, title.as_bytes())?;
        }
        put_uleb(
            &mut raw,
            u32::try_from(self.operations.len()).map_err(|_| Error::LengthOverflow)?,
        );
        for operation in &self.operations {
            encode_grid_operation(operation, dimensions, &mut raw)?;
        }
        if has_flag(frame_flags, crate::schema::terminal::FRAME_COMPONENTS) {
            put_uleb(
                &mut raw,
                u32::try_from(self.components.len()).map_err(|_| Error::LengthOverflow)?,
            );
            for component in &self.components {
                raw.push(component.kind);
                raw.push(if component.required {
                    crate::schema::terminal::COMPONENT_REQUIRED as u8
                } else {
                    0
                });
                put_uleb(
                    &mut raw,
                    u32::try_from(component.body.len()).map_err(|_| Error::LengthOverflow)?,
                );
                raw.extend_from_slice(&component.body);
            }
        }
        if raw.len() > max_decoded_frame as usize {
            return Err(Error::LimitExceeded {
                limit: "decoded Terminal grid",
                actual: raw.len() as u64,
                maximum: u64::from(max_decoded_frame),
            });
        }
        if has_flag(frame_flags, crate::schema::terminal::FRAME_CODEC_COMPRESSED) {
            let block = compress(&raw);
            let encoded_len = 4usize
                .checked_add(block.len())
                .ok_or(Error::LengthOverflow)?;
            if encoded_len.checked_add(8).ok_or(Error::LengthOverflow)? > raw.len() {
                return Err(Error::Invalid("unprofitable Terminal grid compression"));
            }
            let mut out = Vec::with_capacity(encoded_len);
            put_len_u32(&mut out, raw.len())?;
            out.extend_from_slice(&block);
            Ok(out)
        } else {
            Ok(raw)
        }
    }

    fn validate(
        &self,
        frame_flags: u16,
        base_dimensions: Option<(u16, u16)>,
    ) -> Result<(u16, u16)> {
        field_presence(
            frame_flags,
            crate::schema::terminal::FRAME_DIMENSIONS,
            self.dimensions.is_some(),
            "dimensions",
        )?;
        field_presence(
            frame_flags,
            crate::schema::terminal::FRAME_CURSOR,
            self.cursor.is_some(),
            "cursor",
        )?;
        field_presence(
            frame_flags,
            crate::schema::terminal::FRAME_MODES,
            self.modes.is_some(),
            "modes",
        )?;
        field_presence(
            frame_flags,
            crate::schema::terminal::FRAME_SCROLLBACK,
            self.scrollback_lines.is_some(),
            "scrollback",
        )?;
        field_presence(
            frame_flags,
            crate::schema::terminal::FRAME_VIEW_OFFSET,
            self.scroll_offset.is_some(),
            "view offset",
        )?;
        field_presence(
            frame_flags,
            crate::schema::terminal::FRAME_TITLE,
            self.title.is_some(),
            "title",
        )?;
        if !has_flag(frame_flags, crate::schema::terminal::FRAME_COMPONENTS)
            && !self.components.is_empty()
        {
            return Err(Error::Invalid("components"));
        }
        let dimensions = self
            .dimensions
            .or(base_dimensions)
            .ok_or(Error::Invalid("Terminal grid has no effective dimensions"))?;
        if dimensions.0 == 0 || dimensions.1 == 0 {
            return Err(Error::Invalid("Terminal grid dimensions"));
        }
        if let Some((row, col)) = self.cursor
            && (row >= dimensions.0 || col >= dimensions.1)
        {
            return Err(Error::Invalid("Terminal cursor bounds"));
        }
        let mut previous = None;
        for component in &self.components {
            if previous.is_some_and(|kind| kind >= component.kind) {
                return Err(Error::Invalid("Terminal component order"));
            }
            previous = Some(component.kind);
            validate_component(component, dimensions)?;
        }
        for operation in &self.operations {
            validate_grid_operation(operation, dimensions)?;
        }
        Ok(dimensions)
    }
}

fn has_flag(flags: u16, flag: u64) -> bool {
    flags & flag as u16 != 0
}

fn field_presence(flags: u16, flag: u64, present: bool, name: &'static str) -> Result<()> {
    if has_flag(flags, flag) == present {
        Ok(())
    } else {
        Err(Error::Invalid(name))
    }
}

fn put_uleb(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn get_uleb(decoder: &mut Decoder<'_>) -> Result<u32> {
    let mut value = 0u32;
    for index in 0..5 {
        let byte = decoder.u8()?;
        if index == 4 && byte & 0xf0 != 0 {
            return Err(Error::Invalid("Terminal ULEB128 overflow"));
        }
        value |= u32::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            if index != 0 && byte == 0 {
                return Err(Error::Invalid("noncanonical Terminal ULEB128"));
            }
            return Ok(value);
        }
    }
    Err(Error::Invalid("Terminal ULEB128 overflow"))
}

fn decode_cells(decoder: &mut Decoder<'_>, count: usize) -> Result<Vec<Cell>> {
    let encoded_len = count.checked_mul(CELL_BYTES).ok_or(Error::LengthOverflow)?;
    let encoded = decoder.take(encoded_len)?;
    let mut cells = vec![[0; CELL_BYTES]; count];
    for plane in 0..CELL_BYTES {
        for index in 0..count {
            cells[index][plane] = encoded[plane * count + index];
        }
    }
    Ok(cells)
}

fn encode_cells(out: &mut Vec<u8>, cells: &[Cell]) {
    for plane in 0..CELL_BYTES {
        for cell in cells {
            out.push(cell[plane]);
        }
    }
}

fn grid_cells(dimensions: (u16, u16)) -> u32 {
    u32::from(dimensions.0) * u32::from(dimensions.1)
}

fn decode_grid_operation(
    decoder: &mut Decoder<'_>,
    dimensions: (u16, u16),
) -> Result<GridOperation> {
    let operation = match decoder.u8()? {
        value if value == crate::schema::terminal::GRID_PATCH_RUN as u8 => {
            let start_cell = get_uleb(decoder)?;
            let count = usize::try_from(get_uleb(decoder)?).map_err(|_| Error::LengthOverflow)?;
            GridOperation::PatchRun {
                start_cell,
                cells: decode_cells(decoder, count)?,
            }
        }
        value if value == crate::schema::terminal::GRID_PATCH_LIST as u8 => {
            let count = usize::try_from(get_uleb(decoder)?).map_err(|_| Error::LengthOverflow)?;
            if count == 0
                || count > grid_cells(dimensions) as usize
                || count > decoder.remaining() / CELL_BYTES
            {
                return Err(Error::Invalid("Terminal PATCH_LIST count"));
            }
            let mut indices = Vec::with_capacity(count);
            if count != 0 {
                indices.push(get_uleb(decoder)?);
                for _ in 1..count {
                    let delta = get_uleb(decoder)?;
                    if delta == 0 {
                        return Err(Error::Invalid("Terminal PATCH_LIST delta"));
                    }
                    indices.push(
                        indices
                            .last()
                            .unwrap()
                            .checked_add(delta)
                            .ok_or(Error::LengthOverflow)?,
                    );
                }
            }
            GridOperation::PatchList {
                indices,
                cells: decode_cells(decoder, count)?,
            }
        }
        value if value == crate::schema::terminal::GRID_PATCH_BITMAP as u8 => {
            let start_cell = get_uleb(decoder)?;
            let span = get_uleb(decoder)?;
            let bitmap_len =
                usize::try_from(span.div_ceil(8)).map_err(|_| Error::LengthOverflow)?;
            let bitmap = decoder.take(bitmap_len)?.to_vec();
            let count = bitmap.iter().map(|byte| byte.count_ones() as usize).sum();
            GridOperation::PatchBitmap {
                start_cell,
                span,
                bitmap,
                cells: decode_cells(decoder, count)?,
            }
        }
        value if value == crate::schema::terminal::GRID_COPY_RECT as u8 => {
            GridOperation::CopyRect {
                src_row: decoder.u16()?,
                src_col: decoder.u16()?,
                dst_row: decoder.u16()?,
                dst_col: decoder.u16()?,
                rows: decoder.u16()?,
                cols: decoder.u16()?,
            }
        }
        value if value == crate::schema::terminal::GRID_FILL_RECT as u8 => {
            GridOperation::FillRect {
                row: decoder.u16()?,
                col: decoder.u16()?,
                rows: decoder.u16()?,
                cols: decoder.u16()?,
                cell: decoder.take(CELL_BYTES)?.try_into().unwrap(),
            }
        }
        _ => return Err(Error::Invalid("Terminal grid opcode")),
    };
    validate_grid_operation(&operation, dimensions)?;
    Ok(operation)
}

fn encode_grid_operation(
    operation: &GridOperation,
    dimensions: (u16, u16),
    out: &mut Vec<u8>,
) -> Result<()> {
    validate_grid_operation(operation, dimensions)?;
    match operation {
        GridOperation::PatchRun { start_cell, cells } => {
            out.push(crate::schema::terminal::GRID_PATCH_RUN as u8);
            put_uleb(out, *start_cell);
            put_uleb(
                out,
                u32::try_from(cells.len()).map_err(|_| Error::LengthOverflow)?,
            );
            encode_cells(out, cells);
        }
        GridOperation::PatchList { indices, cells } => {
            out.push(crate::schema::terminal::GRID_PATCH_LIST as u8);
            put_uleb(
                out,
                u32::try_from(indices.len()).map_err(|_| Error::LengthOverflow)?,
            );
            put_uleb(out, indices[0]);
            for pair in indices.windows(2) {
                put_uleb(out, pair[1] - pair[0]);
            }
            encode_cells(out, cells);
        }
        GridOperation::PatchBitmap {
            start_cell,
            span,
            bitmap,
            cells,
        } => {
            out.push(crate::schema::terminal::GRID_PATCH_BITMAP as u8);
            put_uleb(out, *start_cell);
            put_uleb(out, *span);
            out.extend_from_slice(bitmap);
            encode_cells(out, cells);
        }
        GridOperation::CopyRect {
            src_row,
            src_col,
            dst_row,
            dst_col,
            rows,
            cols,
        } => {
            out.push(crate::schema::terminal::GRID_COPY_RECT as u8);
            put_u16(out, *src_row);
            put_u16(out, *src_col);
            put_u16(out, *dst_row);
            put_u16(out, *dst_col);
            put_u16(out, *rows);
            put_u16(out, *cols);
        }
        GridOperation::FillRect {
            row,
            col,
            rows,
            cols,
            cell,
        } => {
            out.push(crate::schema::terminal::GRID_FILL_RECT as u8);
            put_u16(out, *row);
            put_u16(out, *col);
            put_u16(out, *rows);
            put_u16(out, *cols);
            out.extend_from_slice(cell);
        }
    }
    Ok(())
}

fn validate_grid_operation(operation: &GridOperation, dimensions: (u16, u16)) -> Result<()> {
    let total = grid_cells(dimensions);
    match operation {
        GridOperation::PatchRun { start_cell, cells } => {
            let count = u32::try_from(cells.len()).map_err(|_| Error::LengthOverflow)?;
            if count == 0 || start_cell.checked_add(count).is_none_or(|end| end > total) {
                return Err(Error::Invalid("Terminal PATCH_RUN bounds"));
            }
        }
        GridOperation::PatchList { indices, cells } => {
            if indices.is_empty()
                || indices.len() != cells.len()
                || indices.last().is_none_or(|index| *index >= total)
                || indices.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(Error::Invalid("Terminal PATCH_LIST bounds"));
            }
        }
        GridOperation::PatchBitmap {
            start_cell,
            span,
            bitmap,
            cells,
        } => {
            let expected = usize::try_from(span.div_ceil(8)).map_err(|_| Error::LengthOverflow)?;
            let count: usize = bitmap.iter().map(|byte| byte.count_ones() as usize).sum();
            let final_bit = span
                .checked_sub(1)
                .ok_or(Error::Invalid("zero bitmap span"))?;
            let trailing_mask = if span % 8 == 0 {
                u8::MAX
            } else {
                (1u8 << (span % 8)) - 1
            };
            if bitmap.len() != expected
                || bitmap.first().is_none_or(|byte| byte & 1 == 0)
                || bitmap
                    .get((final_bit / 8) as usize)
                    .is_none_or(|byte| byte & (1 << (final_bit % 8)) == 0)
                || bitmap.last().is_some_and(|byte| byte & !trailing_mask != 0)
                || count != cells.len()
                || start_cell.checked_add(*span).is_none_or(|end| end > total)
            {
                return Err(Error::Invalid("Terminal PATCH_BITMAP bounds"));
            }
        }
        GridOperation::CopyRect {
            src_row,
            src_col,
            dst_row,
            dst_col,
            rows,
            cols,
        } => {
            if *rows == 0
                || *cols == 0
                || src_row
                    .checked_add(*rows)
                    .is_none_or(|end| end > dimensions.0)
                || src_col
                    .checked_add(*cols)
                    .is_none_or(|end| end > dimensions.1)
                || dst_row
                    .checked_add(*rows)
                    .is_none_or(|end| end > dimensions.0)
                || dst_col
                    .checked_add(*cols)
                    .is_none_or(|end| end > dimensions.1)
            {
                return Err(Error::Invalid("Terminal COPY_RECT bounds"));
            }
        }
        GridOperation::FillRect {
            row,
            col,
            rows,
            cols,
            ..
        } => {
            if *rows == 0
                || *cols == 0
                || row.checked_add(*rows).is_none_or(|end| end > dimensions.0)
                || col.checked_add(*cols).is_none_or(|end| end > dimensions.1)
            {
                return Err(Error::Invalid("Terminal FILL_RECT bounds"));
            }
        }
    }
    Ok(())
}

fn validate_component(component: &Component, dimensions: (u16, u16)) -> Result<()> {
    match component.kind {
        value if value == crate::schema::terminal::COMPONENT_LINE_FLAGS as u8 => {
            validate_line_flags(&component.body, dimensions.0)
        }
        value if value == crate::schema::terminal::COMPONENT_OVERFLOW_STRINGS as u8 => {
            validate_overflow_strings(&component.body, grid_cells(dimensions))
        }
        value if value == crate::schema::terminal::COMPONENT_HYPERLINKS as u8 => {
            validate_hyperlinks(&component.body, grid_cells(dimensions))
        }
        _ if component.required => Err(Error::Invalid("unknown required Terminal component")),
        _ => Ok(()),
    }
}

fn validate_line_flags(input: &[u8], rows: u16) -> Result<()> {
    let mut decoder = Decoder::new(input);
    let count = get_uleb(&mut decoder)?;
    let mut end = 0u32;
    for _ in 0..count {
        let start = get_uleb(&mut decoder)?;
        let length = get_uleb(&mut decoder)?;
        if length == 0 || start < end {
            return Err(Error::Invalid("Terminal LINE_FLAGS runs"));
        }
        end = start.checked_add(length).ok_or(Error::LengthOverflow)?;
        if end > u32::from(rows) {
            return Err(Error::Invalid("Terminal LINE_FLAGS bounds"));
        }
        decoder.u8()?;
    }
    decoder.finish()
}

fn validate_overflow_strings(input: &[u8], total_cells: u32) -> Result<()> {
    let mut decoder = Decoder::new(input);
    let count = get_uleb(&mut decoder)?;
    let mut previous = None;
    for _ in 0..count {
        let index = get_uleb(&mut decoder)?;
        if index >= total_cells || previous.is_some_and(|old| old >= index) {
            return Err(Error::Invalid("Terminal OVERFLOW_STRINGS indices"));
        }
        previous = Some(index);
        let len = usize::try_from(get_uleb(&mut decoder)?).map_err(|_| Error::LengthOverflow)?;
        core::str::from_utf8(decoder.take(len)?).map_err(|_| Error::InvalidUtf8)?;
    }
    decoder.finish()
}

fn validate_hyperlinks(input: &[u8], total_cells: u32) -> Result<()> {
    let mut decoder = Decoder::new(input);
    let uri_count = get_uleb(&mut decoder)?;
    let mut ids = BTreeSet::new();
    for _ in 0..uri_count {
        let id = get_uleb(&mut decoder)?;
        if id == 0 || !ids.insert(id) {
            return Err(Error::Invalid("Terminal hyperlink ID"));
        }
        let len = usize::try_from(get_uleb(&mut decoder)?).map_err(|_| Error::LengthOverflow)?;
        if len > MAX_HYPERLINK_URI_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Terminal hyperlink URI",
                actual: len as u64,
                maximum: MAX_HYPERLINK_URI_BYTES as u64,
            });
        }
        core::str::from_utf8(decoder.take(len)?).map_err(|_| Error::InvalidUtf8)?;
    }
    let run_count = get_uleb(&mut decoder)?;
    let mut end = 0u32;
    for _ in 0..run_count {
        let start = get_uleb(&mut decoder)?;
        let length = get_uleb(&mut decoder)?;
        let id = get_uleb(&mut decoder)?;
        if length == 0 || start < end || !ids.contains(&id) {
            return Err(Error::Invalid("Terminal hyperlink runs"));
        }
        end = start.checked_add(length).ok_or(Error::LengthOverflow)?;
        if end > total_cells {
            return Err(Error::Invalid("Terminal hyperlink bounds"));
        }
    }
    decoder.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::{Frame, FrameCodec, FrameHeader};

    fn truncations<T: Decode>(bytes: &[u8]) {
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
        T::decode(bytes).unwrap();
    }

    #[test]
    fn create_launch_golden_and_truncation() {
        let value = Create {
            rows: 24,
            cols: 80,
            operation_id: [0x11; 16],
            launch: Launch {
                command: Command::Argv(vec![b"sh".to_vec(), b"-l".to_vec()]),
                cwd: Cwd::Path(b"/tmp".to_vec()),
                environment_base: EnvironmentBase::Empty,
                environment: vec![
                    EnvironmentEntry {
                        key: b"LANG".to_vec(),
                        value: EnvironmentValue::Set(b"C".to_vec()),
                    },
                    EnvironmentEntry {
                        key: b"TERM".to_vec(),
                        value: EnvironmentValue::Remove,
                    },
                ],
                extensions: Extensions::default(),
            },
            extensions: Extensions::default(),
        };
        let encoded = value.encode().unwrap();
        assert_eq!(Create::decode(&encoded).unwrap(), value);
        assert_eq!(
            hex(&encoded),
            "18005000000000001111111111111111111111111111111137000000010101000200020000007368020000002d6c040000002f746d70020004004c414e4700010000004304005445524d01000000000000000000000000"
        );
        truncations::<Create>(&encoded);
    }

    #[test]
    fn byte_budget_frame_is_exact_and_grid_decodes() {
        let cell = [0x00, 0x08, 0, 0, 0, 0, 0, 0, b'x', 0, 0, 0];
        let grid = Grid {
            dimensions: None,
            cursor: Some((23, 79)),
            modes: None,
            scrollback_lines: None,
            scroll_offset: None,
            title: None,
            operations: vec![GridOperation::PatchRun {
                start_cell: 1918,
                cells: vec![cell],
            }],
            components: vec![],
        };
        let flags = crate::schema::terminal::FRAME_CURSOR as u16;
        let grid_payload = grid.encode_codec1(flags, 4096, Some((24, 80))).unwrap();
        let terminal_frame = TerminalFrame {
            view_id: 1,
            frame_sequence: 2,
            frame_flags: flags,
            base_sequence: None,
            grid_payload,
        };
        let codec = FrameCodec::new(crate::frame::FrameLimits::recommended(), []).unwrap();
        let mut header = FrameHeader::event(
            crate::family::TERMINAL,
            crate::schema::terminal::event::FRAME,
        );
        header.sensitive = true;
        let frame = Frame {
            header,
            payload: terminal_frame.encode().unwrap(),
        };
        let encoded = codec.encode(&frame).unwrap();
        assert_eq!(encoded.len(), 36);
        assert_eq!(
            hex(&encoded),
            "10002000080100000002000000080017004f000100fe0e01000800000000000078000000"
        );
        let decoded_frame = codec.decode(&encoded).unwrap();
        let terminal = TerminalFrame::decode(&decoded_frame.payload).unwrap();
        assert_eq!(
            terminal.decode_grid_codec1(4096, Some((24, 80))).unwrap(),
            grid
        );
        let oversized = TerminalFrame {
            view_id: 1,
            frame_sequence: 3,
            frame_flags: flags,
            base_sequence: None,
            grid_payload: vec![0; crate::frame::HARD_MAX_BULK_CHUNK as usize + 1],
        };
        assert!(oversized.encode().is_err());
        let oversized_logical = oversized.encode_logical_body().unwrap();
        assert_eq!(
            oversized_logical.len(),
            crate::frame::HARD_MAX_BULK_CHUNK as usize + 3,
        );
        assert_eq!(&oversized_logical[..2], &flags.to_le_bytes());
        let oversized_chunk = FrameChunk {
            view_id: 1,
            frame_sequence: 3,
            chunk_index: 0,
            chunk_count: 1,
            logical_frame_len: crate::frame::HARD_MAX_BULK_CHUNK + 1,
            chunk: vec![0; crate::frame::HARD_MAX_BULK_CHUNK as usize + 1],
        };
        assert!(oversized_chunk.encode().is_err());
        for end in 0..encoded.len() {
            let result = codec.decode(&encoded[..end]).and_then(|frame| {
                let terminal = TerminalFrame::decode(&frame.payload)?;
                terminal
                    .decode_grid_codec1(4096, Some((24, 80)))
                    .map(|_| ())
            });
            assert!(result.is_err(), "accepted frame prefix {end}");
        }
    }

    #[test]
    fn query_body_inline_and_transfer_are_typed() {
        let value = QueryBody {
            content_kind: crate::schema::terminal::CONTENT_TEXT as u8,
            encoding: crate::schema::terminal::QUERY_ENCODING_UTF8 as u8,
            flags: crate::schema::terminal::QUERY_TRUNCATED as u16,
            delivery: QueryDelivery::Inline(b"hello".to_vec()),
            next_cursor: Some(QueryNextCursor::Read(QueryCursor {
                kind: crate::schema::terminal::READ_CURSOR_ABSOLUTE as u8,
                a: 9,
                b: 2,
            })),
            total_lines: Some(12),
            satisfying_state_revision: None,
            extensions: Extensions::default(),
        };
        let encoded = value.encode().unwrap();
        assert_eq!(QueryBody::decode(&encoded).unwrap(), value);
        truncations::<QueryBody>(&encoded);

        let descriptor = Descriptor {
            transfer_id: 2,
            mode: Mode::Byte,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: 4096,
            max_item_bytes: 0,
            max_chunk_bytes: 1024,
            content_family: crate::family::TERMINAL,
            content_kind: crate::schema::terminal::QUERY_CONTENT_KIND as u16,
            content_version: VERSION,
            extensions: Extensions(vec![Extension {
                tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        };
        let transfer = QueryBody {
            content_kind: crate::schema::terminal::CONTENT_OUTPUT as u8,
            encoding: crate::schema::terminal::QUERY_ENCODING_TERMINAL_RECORDS as u8,
            flags: 0,
            delivery: QueryDelivery::Transfer(descriptor),
            next_cursor: Some(QueryNextCursor::Output(QueryCursor {
                kind: crate::schema::terminal::OUTPUT_CURSOR_SEQUENCE as u8,
                a: 10,
                b: 3,
            })),
            total_lines: None,
            satisfying_state_revision: Some(7),
            extensions: Extensions::default(),
        };
        let encoded = transfer.encode().unwrap();
        assert_eq!(QueryBody::decode(&encoded).unwrap(), transfer);
        truncations::<QueryBody>(&encoded);
        transfer.validate_receive_credit(4096).unwrap();
        assert!(transfer.validate_receive_credit(0).is_err());
        assert!(transfer.validate_receive_credit(4095).is_err());
    }

    #[test]
    fn catalogue_search_round_trip_is_typed_and_bounded() {
        let request = CatalogSearch {
            max_results: 20,
            query: "cargo|shell".into(),
            extensions: Extensions::default(),
        };
        let bytes = request.encode().unwrap();
        assert_eq!(CatalogSearch::decode(&bytes).unwrap(), request);
        truncations::<CatalogSearch>(&bytes);

        let result = CatalogSearchResult {
            flags: crate::schema::terminal::CATALOG_SEARCH_TRUNCATED as u16,
            entries: vec![CatalogSearchEntry {
                terminal_handle: 7,
                generation: 3,
                score: 100,
                primary_source: crate::schema::terminal::CATALOG_SEARCH_SOURCE_VISIBLE as u8,
                matched_sources: (crate::schema::terminal::CATALOG_SEARCH_MATCH_TITLE
                    | crate::schema::terminal::CATALOG_SEARCH_MATCH_VISIBLE)
                    as u8,
                scroll_offset: 42,
                context: "cargo test".into(),
            }],
            extensions: Extensions::default(),
        };
        let bytes = result.encode().unwrap();
        assert_eq!(CatalogSearchResult::decode(&bytes).unwrap(), result);
        truncations::<CatalogSearchResult>(&bytes);

        let mut invalid = result;
        invalid.entries[0].matched_sources =
            crate::schema::terminal::CATALOG_SEARCH_MATCH_TITLE as u8;
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn query_cursor_requests_and_styled_partial_rows_are_exact() {
        let cursor = QueryCursor {
            kind: crate::schema::terminal::READ_CURSOR_TAIL as u8,
            a: 7,
            b: 3,
        };
        let bytes = cursor.encode().unwrap();
        assert_eq!(bytes.len(), 13);
        assert_eq!(QueryCursor::decode(&bytes).unwrap(), cursor);
        truncations::<QueryCursor>(&bytes);

        let styled = StyledLines(vec![StyledLine {
            row: -1,
            start_col: 5,
            cells: vec![[0; CELL_BYTES], [1; CELL_BYTES]],
            overflow: vec![StyledOverflow {
                cell_offset: 1,
                text: "wide".into(),
            }],
            hyperlinks: vec![StyledHyperlink {
                start_col: 5,
                cell_count: 2,
                uri: "https://example.test".into(),
            }],
        }]);
        let bytes = styled.encode().unwrap();
        assert_eq!(StyledLines::decode(&bytes).unwrap(), styled);
        truncations::<StyledLines>(&bytes);

        let combined = TextAndStyled {
            plain: "hi".into(),
            styled,
        };
        let bytes = combined.encode().unwrap();
        assert_eq!(TextAndStyled::decode(&bytes).unwrap(), combined);
        truncations::<TextAndStyled>(&bytes);

        let read = Read {
            terminal_handle: 1,
            generation: 1,
            cursor_kind: crate::schema::terminal::READ_CURSOR_ABSOLUTE as u8,
            representation: crate::schema::terminal::QUERY_REPRESENTATION_BOTH as u8,
            flags: 0,
            cursor_a: 3,
            cursor_b: 20,
            max_bytes: 4096,
            initial_receive_credit: 8192,
            extensions: Extensions::default(),
        };
        let bytes = read.encode().unwrap();
        assert_eq!(Read::decode(&bytes).unwrap(), read);
        truncations::<Read>(&bytes);
        let mut invalid = read;
        invalid.flags = 1;
        assert!(invalid.encode().is_err());

        let wait = Wait {
            terminal_handle: 1,
            generation: 1,
            wait_kind: crate::schema::terminal::WAIT_OUTPUT as u8,
            flags: 0,
            cursor_a: 3,
            cursor_b: 4,
            max_bytes: 4096,
            timeout_ns: 1_000_000,
            needle: b"ready".to_vec(),
            initial_receive_credit: 8192,
            extensions: Extensions::default(),
        };
        let bytes = wait.encode().unwrap();
        assert_eq!(Wait::decode(&bytes).unwrap(), wait);
        truncations::<Wait>(&bytes);
        let mut invalid = wait;
        invalid.needle.clear();
        assert!(invalid.encode().is_err());
    }

    #[test]
    fn close_view_is_exact_and_idempotent_shape() {
        let bytes = CloseView { view_id: 7 }.encode().unwrap();
        assert_eq!(bytes, [7, 0, 0, 0]);
        truncations::<CloseView>(&bytes);
    }

    #[test]
    fn malicious_huge_grid_counts_fail_before_allocation() {
        let huge = [0xff, 0xff, 0xff, 0xff, 0x0f];
        assert_eq!(
            Grid::decode_codec1(0, &huge, 64, Some((24, 80))),
            Err(Error::Invalid("Terminal grid operation count"))
        );

        let mut patch_list = vec![1, crate::schema::terminal::GRID_PATCH_LIST as u8];
        patch_list.extend_from_slice(&huge);
        patch_list.extend_from_slice(&[0; 13]);
        assert_eq!(
            Grid::decode_codec1(0, &patch_list, 64, Some((24, 80))),
            Err(Error::Invalid("Terminal PATCH_LIST count"))
        );

        let components = vec![0, 0xff, 0xff, 0xff, 0xff, 0x0f];
        assert_eq!(
            Grid::decode_codec1(
                crate::schema::terminal::FRAME_COMPONENTS as u16,
                &components,
                64,
                Some((24, 80)),
            ),
            Err(Error::Invalid("Terminal component count"))
        );
    }

    #[test]
    fn minimal_no_command_journal_record_round_trips() {
        let value = JournalResult {
            oldest_index: 4,
            next_index: 5,
            records: vec![JournalRecord {
                index: 4,
                generation: 1,
                flags: crate::schema::terminal::JOURNAL_NO_COMMAND as u16,
                exit_code: 0,
                start_seq: 0,
                end_seq: 0,
                started_unix_ms: 0,
                ended_unix_ms: 0,
                command: String::new(),
            }],
        };
        let bytes = value.encode().unwrap();
        assert_eq!(JournalResult::decode(&bytes).unwrap(), value);
        for end in 0..bytes.len() {
            assert!(JournalResult::decode(&bytes[..end]).is_err());
        }
    }

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);

        let mut invalid = Limits::HARD;
        invalid.max_views_per_session = 0;
        assert!(invalid.to_extensions().is_err());
        assert!(Limits::from_extensions(&Extensions::default()).is_err());
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}
