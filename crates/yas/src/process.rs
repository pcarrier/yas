//! YAS native non-PTY process family wire values.

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, put_bytes_u16, put_bytes_u32,
    put_i32, put_len_u16, put_u16, put_u64,
};
use crate::prelude::*;
use crate::state::{Record, RecordKind};
use crate::transfer::{Descriptor, Direction, Mode};

pub const VERSION: u16 = crate::schema::process::VERSION;
pub const MAX_ARGC: usize = crate::schema::process::MAX_ARGC as usize;
pub const MAX_ARG_BYTES: usize = crate::schema::process::MAX_ARG_BYTES as usize;
pub const MAX_ARG_LEN: usize = crate::schema::process::MAX_ARG_LEN as usize;
pub const MAX_ENVC: usize = crate::schema::process::MAX_ENVC as usize;
pub const MAX_ENV_BYTES: usize = crate::schema::process::MAX_ENV_BYTES as usize;
pub const MAX_ENV_KEY_BYTES: usize = crate::schema::process::MAX_ENV_KEY_BYTES as usize;
pub const MAX_ENV_VALUE_BYTES: usize = crate::schema::process::MAX_ENV_VALUE_BYTES as usize;
pub const MAX_CWD_BYTES: usize = crate::schema::process::MAX_CWD_BYTES as usize;
pub const MAX_PATH_COMPONENTS: usize = crate::schema::process::MAX_PATH_COMPONENTS as usize;
pub const MAX_STREAM_BUFFER_BYTES: u64 = crate::schema::process::MAX_STREAM_BUFFER_BYTES;

pub mod request_kind {
    pub use crate::schema::process::request::*;
}

pub mod event_kind {
    pub use crate::schema::process::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum EnvironmentKind {
    Empty = crate::schema::process::ENV_EMPTY as u8,
    Session = crate::schema::process::ENV_SESSION as u8,
}

impl TryFrom<u8> for EnvironmentKind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::process::ENV_EMPTY as u8 => Ok(Self::Empty),
            value if value == crate::schema::process::ENV_SESSION as u8 => Ok(Self::Session),
            _ => Err(Error::Invalid("Process environment kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cwd {
    ServerDefault,
    Path(Vec<u8>),
    Terminal(u64),
    Fs {
        root_handle: u64,
        components: Vec<Vec<u8>>,
    },
}

impl Cwd {
    fn validate(&self) -> Result<()> {
        match self {
            Self::ServerDefault => Ok(()),
            Self::Path(path) => validate_native_path(path),
            Self::Terminal(handle) => validate_handle(*handle, "Process cwd terminal handle"),
            Self::Fs {
                root_handle,
                components,
            } => {
                validate_handle(*root_handle, "Process cwd FS root handle")?;
                if components.len() > MAX_PATH_COMPONENTS {
                    return Err(limit(
                        "Process cwd path components",
                        components.len() as u64,
                        MAX_PATH_COMPONENTS as u64,
                    ));
                }
                let mut total = 0usize;
                for component in components {
                    validate_component(component)?;
                    total = total
                        .checked_add(component.len())
                        .ok_or(Error::LengthOverflow)?;
                    if total > MAX_CWD_BYTES {
                        return Err(limit(
                            "Process cwd bytes",
                            total as u64,
                            MAX_CWD_BYTES as u64,
                        ));
                    }
                }
                Ok(())
            }
        }
    }
}

impl Encode for Cwd {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        match self {
            Self::ServerDefault => {
                out.push(crate::schema::process::CWD_SERVER_DEFAULT as u8);
                out.extend_from_slice(&[0; 3]);
            }
            Self::Path(path) => {
                out.push(crate::schema::process::CWD_PATH as u8);
                out.extend_from_slice(&[0; 3]);
                put_bytes_u32(out, path)?;
            }
            Self::Terminal(handle) => {
                out.push(crate::schema::process::CWD_TERMINAL as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *handle);
            }
            Self::Fs {
                root_handle,
                components,
            } => {
                out.push(crate::schema::process::CWD_FS as u8);
                out.extend_from_slice(&[0; 3]);
                put_u64(out, *root_handle);
                put_len_u16(out, components.len())?;
                for component in components {
                    put_bytes_u16(out, component)?;
                }
            }
        }
        Ok(())
    }
}

impl Decode for Cwd {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Process cwd reserved bytes"));
        }
        let value = match kind {
            value if value == crate::schema::process::CWD_SERVER_DEFAULT as u8 => {
                Self::ServerDefault
            }
            value if value == crate::schema::process::CWD_PATH as u8 => {
                Self::Path(decoder.len_bytes_u32()?.to_vec())
            }
            value if value == crate::schema::process::CWD_TERMINAL as u8 => {
                Self::Terminal(decoder.u64()?)
            }
            value if value == crate::schema::process::CWD_FS as u8 => {
                let root_handle = decoder.u64()?;
                let count = usize::from(decoder.u16()?);
                if count > MAX_PATH_COMPONENTS || count > decoder.remaining() / 2 {
                    return Err(Error::Invalid("Process cwd path component count"));
                }
                let mut components = Vec::with_capacity(count);
                for _ in 0..count {
                    components.push(decoder.len_bytes_u16()?.to_vec());
                }
                Self::Fs {
                    root_handle,
                    components,
                }
            }
            _ => return Err(Error::Invalid("Process cwd kind")),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvEntry {
    pub key: Vec<u8>,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spawn {
    pub operation_id: [u8; 16],
    pub flags: u16,
    pub environment_kind: EnvironmentKind,
    pub cwd: Cwd,
    pub argv: Vec<Vec<u8>>,
    pub env: Vec<EnvEntry>,
    pub stdout_receive_credit: u64,
    pub stderr_receive_credit: u64,
    pub extensions: Extensions,
}

impl Spawn {
    fn validate(&self) -> Result<()> {
        validate_operation_id(&self.operation_id)?;
        if self.flags & !(crate::schema::process::SPAWN_FLAGS as u16) != 0 {
            return Err(Error::Invalid("Process spawn flags"));
        }
        self.cwd.validate()?;
        validate_argv(&self.argv)?;
        validate_env(&self.env)?;
        if self.stdout_receive_credit == 0 {
            return Err(Error::Invalid("zero Process stdout receive credit"));
        }
        let merged = self.flags & crate::schema::process::SPAWN_MERGE_STDERR as u16 != 0;
        if merged != (self.stderr_receive_credit == 0) {
            return Err(Error::Invalid("Process stderr receive credit"));
        }
        validate_spawn_extensions(&self.extensions)
    }

    pub fn surface_app_handle(&self) -> Result<Option<u64>> {
        extension_u64(
            &self.extensions,
            crate::schema::process::SPAWN_SURFACE_APP_EXTENSION,
            "Process surface application extension",
        )
    }

    pub fn resource_tag(&self) -> Result<Option<&[u8]>> {
        let Some(extension) = self.extensions.0.iter().find(|extension| {
            extension.tag == crate::schema::process::SPAWN_RESOURCE_TAG_EXTENSION as u16
        }) else {
            return Ok(None);
        };
        if extension.value.len() > 4096 {
            return Err(limit(
                "Process resource tag bytes",
                extension.value.len() as u64,
                4096,
            ));
        }
        Ok(Some(&extension.value))
    }
}

impl Encode for Spawn {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.extend_from_slice(&self.operation_id);
        put_u16(out, self.flags);
        out.push(self.environment_kind as u8);
        out.push(0);
        let cwd = self.cwd.encode()?;
        put_bytes_u32(out, &cwd)?;
        put_len_u16(out, self.argv.len())?;
        for arg in &self.argv {
            put_bytes_u32(out, arg)?;
        }
        put_len_u16(out, self.env.len())?;
        for entry in &self.env {
            put_bytes_u16(out, &entry.key)?;
            put_bytes_u32(out, &entry.value)?;
        }
        put_u64(out, self.stdout_receive_credit);
        put_u64(out, self.stderr_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Spawn {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let operation_id = decoder.array_16()?;
        let flags = decoder.u16()?;
        let environment_kind = EnvironmentKind::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Process spawn reserved byte"));
        }
        let cwd = Cwd::decode(decoder.len_bytes_u32()?)?;
        let argc = usize::from(decoder.u16()?);
        if argc == 0 || argc > MAX_ARGC || argc > decoder.remaining() / 4 {
            return Err(Error::Invalid("Process argv count"));
        }
        let mut argv = Vec::with_capacity(argc);
        for _ in 0..argc {
            argv.push(decoder.len_bytes_u32()?.to_vec());
        }
        let envc = usize::from(decoder.u16()?);
        if envc > MAX_ENVC || envc > decoder.remaining() / 6 {
            return Err(Error::Invalid("Process environment count"));
        }
        let mut env = Vec::with_capacity(envc);
        for _ in 0..envc {
            env.push(EnvEntry {
                key: decoder.len_bytes_u16()?.to_vec(),
                value: decoder.len_bytes_u32()?.to_vec(),
            });
        }
        let value = Self {
            operation_id,
            flags,
            environment_kind,
            cwd,
            argv,
            env,
            stdout_receive_credit: decoder.u64()?,
            stderr_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attach {
    pub process_handle: u64,
    pub flags: u16,
    pub stdout_receive_credit: u64,
    pub stderr_receive_credit: u64,
    pub extensions: Extensions,
}

impl Attach {
    fn validate(&self) -> Result<()> {
        validate_handle(self.process_handle, "Process handle")?;
        if self.flags & !(crate::schema::process::ATTACH_STDIN as u16) != 0 {
            return Err(Error::Invalid("Process attach flags"));
        }
        if self.stdout_receive_credit == 0 {
            return Err(Error::Invalid("zero Process stdout receive credit"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for Attach {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.process_handle);
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_u64(out, self.stdout_receive_credit);
        put_u64(out, self.stderr_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Attach {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let process_handle = decoder.u64()?;
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Process attach reserved field"));
        }
        let value = Self {
            process_handle,
            flags,
            stdout_receive_credit: decoder.u64()?,
            stderr_receive_credit: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlAction {
    Signal = crate::schema::process::CONTROL_SIGNAL as u8,
    Terminate = crate::schema::process::CONTROL_TERMINATE as u8,
    Kill = crate::schema::process::CONTROL_KILL as u8,
    Detach = crate::schema::process::CONTROL_DETACH as u8,
}

impl TryFrom<u8> for ControlAction {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::process::CONTROL_SIGNAL as u8 => Ok(Self::Signal),
            value if value == crate::schema::process::CONTROL_TERMINATE as u8 => {
                Ok(Self::Terminate)
            }
            value if value == crate::schema::process::CONTROL_KILL as u8 => Ok(Self::Kill),
            value if value == crate::schema::process::CONTROL_DETACH as u8 => Ok(Self::Detach),
            _ => Err(Error::Invalid("Process control action")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub process_handle: u64,
    pub operation_id: [u8; 16],
    pub action: ControlAction,
    pub value: u16,
    pub extensions: Extensions,
}

impl Control {
    fn validate(&self) -> Result<()> {
        validate_handle(self.process_handle, "Process handle")?;
        validate_operation_id(&self.operation_id)?;
        match self.action {
            ControlAction::Signal => validate_signal(self.value)?,
            _ if self.value != 0 => return Err(Error::Invalid("Process control value")),
            _ => {}
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for Control {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.process_handle);
        out.extend_from_slice(&self.operation_id);
        out.push(self.action as u8);
        out.push(0);
        put_u16(out, self.value);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Control {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let process_handle = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let action = ControlAction::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Process control reserved byte"));
        }
        let value = Self {
            process_handle,
            operation_id,
            action,
            value: decoder.u16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlResult {
    pub state_revision: u64,
}

impl Encode for ControlResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.state_revision == 0 {
            return Err(Error::Invalid("zero Process state revision"));
        }
        put_u64(out, self.state_revision);
        Ok(())
    }
}

impl Decode for ControlResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            state_revision: decoder.u64()?,
        };
        decoder.finish()?;
        if value.state_revision == 0 {
            return Err(Error::Invalid("zero Process state revision"));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wait {
    pub process_handle: u64,
    pub timeout_ns: u64,
    pub extensions: Extensions,
}

impl Encode for Wait {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.process_handle, "Process handle")?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.process_handle);
        put_u64(out, self.timeout_ns);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Wait {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            process_handle: decoder.u64()?,
            timeout_ns: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        validate_handle(value.process_handle, "Process handle")?;
        reject_unknown_required(&value.extensions, &[])?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitKind {
    Code = crate::schema::process::EXIT_KIND_CODE as u8,
    Signal = crate::schema::process::EXIT_KIND_SIGNAL as u8,
    Killed = crate::schema::process::EXIT_KIND_KILLED as u8,
    Other = crate::schema::process::EXIT_KIND_OTHER as u8,
}

impl TryFrom<u8> for ExitKind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::process::EXIT_KIND_CODE as u8 => Ok(Self::Code),
            value if value == crate::schema::process::EXIT_KIND_SIGNAL as u8 => Ok(Self::Signal),
            value if value == crate::schema::process::EXIT_KIND_KILLED as u8 => Ok(Self::Killed),
            value if value == crate::schema::process::EXIT_KIND_OTHER as u8 => Ok(Self::Other),
            _ => Err(Error::Invalid("Process exit kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitRecord {
    pub kind: ExitKind,
    pub reason: u8,
    pub code: i32,
    pub exited_server_ns: u64,
    pub detail: Vec<u8>,
}

impl ExitRecord {
    fn validate(&self) -> Result<()> {
        let unknown = crate::schema::process::EXIT_REASON_UNKNOWN as u8;
        let max_reason = crate::schema::process::EXIT_REASON_SERVER_SHUTDOWN as u8;
        if self.reason > max_reason || self.exited_server_ns == 0 || self.detail.len() > 4096 {
            return Err(Error::Invalid("Process exit record"));
        }
        match self.kind {
            ExitKind::Code if self.reason == unknown => Ok(()),
            ExitKind::Signal
                if (crate::schema::process::EXIT_REASON_INTERRUPT as u8
                    ..=crate::schema::process::EXIT_REASON_HANGUP as u8)
                    .contains(&self.reason) =>
            {
                Ok(())
            }
            ExitKind::Killed
                if (crate::schema::process::EXIT_REASON_CLIENT as u8..=max_reason)
                    .contains(&self.reason)
                    && self.code == 0 =>
            {
                Ok(())
            }
            ExitKind::Other
                if self.reason == unknown && self.code == 0 && !self.detail.is_empty() =>
            {
                Ok(())
            }
            _ => Err(Error::Invalid("Process exit field combination")),
        }
    }
}

impl Encode for ExitRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.kind as u8);
        out.push(self.reason);
        put_u16(out, 0);
        put_i32(out, self.code);
        put_u64(out, self.exited_server_ns);
        put_bytes_u32(out, &self.detail)
    }
}

impl Decode for ExitRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = ExitKind::try_from(decoder.u8()?)?;
        let reason = decoder.u8()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Process exit reserved field"));
        }
        let value = Self {
            kind,
            reason,
            code: decoder.i32()?,
            exited_server_ns: decoder.u64()?,
            detail: decoder.len_bytes_u32()?.to_vec(),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamBundle {
    pub process_handle: u64,
    pub stdout_lifetime_offset: u64,
    pub stderr_lifetime_offset: u64,
    pub stdin: Option<Descriptor>,
    pub stdout: Descriptor,
    pub stderr: Option<Descriptor>,
    pub merged_stderr: bool,
    pub extensions: Extensions,
}

impl StreamBundle {
    fn validate(&self) -> Result<()> {
        validate_handle(self.process_handle, "Process handle")?;
        if self.merged_stderr != self.stderr.is_none() {
            return Err(Error::Invalid("Process stderr bundle shape"));
        }
        if let Some(stdin) = &self.stdin {
            validate_stream_transfer(
                stdin,
                crate::schema::process::STREAM_STDIN_CONTENT_KIND as u16,
                Direction::RECEIVER_TO_SENDER,
            )?;
        }
        validate_stream_transfer(
            &self.stdout,
            crate::schema::process::STREAM_STDOUT_CONTENT_KIND as u16,
            Direction::SENDER_TO_RECEIVER,
        )?;
        if let Some(stderr) = &self.stderr {
            validate_stream_transfer(
                stderr,
                crate::schema::process::STREAM_STDERR_CONTENT_KIND as u16,
                Direction::SENDER_TO_RECEIVER,
            )?;
        }
        let mut ids = vec![self.stdout.transfer_id];
        if let Some(stdin) = &self.stdin {
            ids.push(stdin.transfer_id);
        }
        if let Some(stderr) = &self.stderr {
            ids.push(stderr.transfer_id);
        }
        ids.sort_unstable();
        if ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(Error::Invalid("reused Process stream Transfer ID"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for StreamBundle {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        let mut flags = crate::schema::process::BUNDLE_STDOUT as u16;
        if self.stdin.is_some() {
            flags |= crate::schema::process::BUNDLE_STDIN as u16;
        }
        if self.stderr.is_some() {
            flags |= crate::schema::process::BUNDLE_STDERR as u16;
        }
        if self.merged_stderr {
            flags |= crate::schema::process::BUNDLE_MERGED_STDERR as u16;
        }
        put_u64(out, self.process_handle);
        put_u16(out, flags);
        put_u16(out, 0);
        put_u64(out, self.stdout_lifetime_offset);
        put_u64(out, self.stderr_lifetime_offset);
        if let Some(descriptor) = &self.stdin {
            put_bytes_u32(out, &descriptor.encode()?)?;
        }
        put_bytes_u32(out, &self.stdout.encode()?)?;
        if let Some(descriptor) = &self.stderr {
            put_bytes_u32(out, &descriptor.encode()?)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for StreamBundle {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let process_handle = decoder.u64()?;
        let flags = decoder.u16()?;
        if flags & !(crate::schema::process::BUNDLE_FLAGS as u16) != 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("Process stream bundle flags"));
        }
        if flags & crate::schema::process::BUNDLE_STDOUT as u16 == 0 {
            return Err(Error::Invalid("Process stdout descriptor missing"));
        }
        let stdout_lifetime_offset = decoder.u64()?;
        let stderr_lifetime_offset = decoder.u64()?;
        let stdin = if flags & crate::schema::process::BUNDLE_STDIN as u16 != 0 {
            Some(Descriptor::decode(decoder.len_bytes_u32()?)?)
        } else {
            None
        };
        let stdout = Descriptor::decode(decoder.len_bytes_u32()?)?;
        let stderr = if flags & crate::schema::process::BUNDLE_STDERR as u16 != 0 {
            Some(Descriptor::decode(decoder.len_bytes_u32()?)?)
        } else {
            None
        };
        let value = Self {
            process_handle,
            stdout_lifetime_offset,
            stderr_lifetime_offset,
            stdin,
            stdout,
            stderr,
            merged_stderr: flags & crate::schema::process::BUNDLE_MERGED_STDERR as u16 != 0,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessRecord {
    pub process_handle: u64,
    pub lifecycle: u8,
    pub stream_state: u8,
    pub flags: u16,
    pub native_pid: u64,
    pub owner_session: [u8; 16],
    pub argv0: Vec<u8>,
    pub stdin_received: u64,
    pub stdout_produced: u64,
    pub stderr_produced: u64,
    pub retention_deadline_server_ns: u64,
    pub exit: Option<ExitRecord>,
    pub extensions: Extensions,
}

impl ProcessRecord {
    fn validate(&self) -> Result<()> {
        validate_handle(self.process_handle, "Process handle")?;
        let running = self.lifecycle == crate::schema::process::LIFECYCLE_RUNNING as u8;
        let exited = self.lifecycle == crate::schema::process::LIFECYCLE_EXITED as u8;
        if (!running && !exited)
            || self.stream_state & !(crate::schema::process::STREAM_STATE_FLAGS as u8) != 0
            || self.flags & !(crate::schema::process::SPAWN_FLAGS as u16) != 0
            || self.native_pid == 0
            || self.owner_session.iter().all(|byte| *byte == 0)
        {
            return Err(Error::Invalid("Process record identity or flags"));
        }
        validate_arg(&self.argv0)?;
        if exited != self.exit.is_some() {
            return Err(Error::Invalid("Process lifecycle and exit record"));
        }
        if exited && self.stream_state != 0 {
            return Err(Error::Invalid("exited Process has open streams"));
        }
        if self.flags & crate::schema::process::SPAWN_DETACHABLE as u16 == 0
            && self.retention_deadline_server_ns != 0
        {
            return Err(Error::Invalid("ordinary Process retention deadline"));
        }
        if let Some(exit) = &self.exit {
            exit.validate()?;
        }
        reject_unknown_required(&self.extensions, &[])
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Process state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }

    pub fn from_state_record(record: &Record) -> Result<Self> {
        if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Process state record kind"));
        }
        Self::decode(&record.body)
    }
}

impl Encode for ProcessRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.process_handle);
        out.push(self.lifecycle);
        out.push(self.stream_state);
        put_u16(out, self.flags);
        put_u64(out, self.native_pid);
        out.extend_from_slice(&self.owner_session);
        put_bytes_u32(out, &self.argv0)?;
        put_u64(out, self.stdin_received);
        put_u64(out, self.stdout_produced);
        put_u64(out, self.stderr_produced);
        put_u64(out, self.retention_deadline_server_ns);
        out.push(u8::from(self.exit.is_some()));
        out.extend_from_slice(&[0; 7]);
        if let Some(exit) = &self.exit {
            put_bytes_u32(out, &exit.encode()?)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for ProcessRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let process_handle = decoder.u64()?;
        let lifecycle = decoder.u8()?;
        let stream_state = decoder.u8()?;
        let flags = decoder.u16()?;
        let native_pid = decoder.u64()?;
        let owner_session = decoder.array_16()?;
        let argv0 = decoder.len_bytes_u32()?.to_vec();
        let stdin_received = decoder.u64()?;
        let stdout_produced = decoder.u64()?;
        let stderr_produced = decoder.u64()?;
        let retention_deadline_server_ns = decoder.u64()?;
        let exit_present = decoder.u8()?;
        if exit_present > 1 || decoder.take(7)? != [0; 7] {
            return Err(Error::Invalid("Process exit presence or reserved bytes"));
        }
        let exit = if exit_present != 0 {
            Some(ExitRecord::decode(decoder.len_bytes_u32()?)?)
        } else {
            None
        };
        let value = Self {
            process_handle,
            lifecycle,
            stream_state,
            flags,
            native_pid,
            owner_session,
            argv0,
            stdin_received,
            stdout_produced,
            stderr_produced,
            retention_deadline_server_ns,
            exit,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovedProcess {
    pub process_handle: u64,
}

impl RemovedProcess {
    pub fn state_record(self) -> Result<Record> {
        validate_handle(self.process_handle, "Process handle")?;
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for RemovedProcess {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.process_handle, "Process handle")?;
        put_u64(out, self.process_handle);
        Ok(())
    }
}

impl Decode for RemovedProcess {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            process_handle: decoder.u64()?,
        };
        decoder.finish()?;
        validate_handle(value.process_handle, "Process handle")?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_argc: u32,
    pub max_arg_bytes: u32,
    pub max_envc: u32,
    pub max_env_bytes: u32,
    pub max_processes_per_session: u32,
    pub max_processes: u32,
    pub max_pending_spawns: u32,
    pub max_stream_buffer_bytes: u64,
    pub max_detached_retention_ns: u64,
    pub max_mutation_replays: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_argc: crate::schema::process::MAX_ARGC as u32,
        max_arg_bytes: crate::schema::process::MAX_ARG_BYTES as u32,
        max_envc: crate::schema::process::MAX_ENVC as u32,
        max_env_bytes: crate::schema::process::MAX_ENV_BYTES as u32,
        max_processes_per_session: crate::schema::process::MAX_PROCESSES_PER_SESSION as u32,
        max_processes: crate::schema::process::MAX_PROCESSES as u32,
        max_pending_spawns: crate::schema::process::MAX_PENDING_SPAWNS as u32,
        max_stream_buffer_bytes: crate::schema::process::MAX_STREAM_BUFFER_BYTES,
        max_detached_retention_ns: crate::schema::process::MAX_DETACHED_RETENTION_NS,
        max_mutation_replays: crate::schema::process::MAX_MUTATION_REPLAYS as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let valid_u32 = |value: u32, maximum: u32| value != 0 && value <= maximum;
        if !valid_u32(self.max_argc, hard.max_argc)
            || !valid_u32(self.max_arg_bytes, hard.max_arg_bytes)
            || !valid_u32(self.max_envc, hard.max_envc)
            || !valid_u32(self.max_env_bytes, hard.max_env_bytes)
            || !valid_u32(
                self.max_processes_per_session,
                hard.max_processes_per_session,
            )
            || !valid_u32(self.max_processes, hard.max_processes)
            || !valid_u32(self.max_pending_spawns, hard.max_pending_spawns)
            || self.max_stream_buffer_bytes == 0
            || self.max_stream_buffer_bytes > hard.max_stream_buffer_bytes
            || self.max_detached_retention_ns == 0
            || self.max_detached_retention_ns > hard.max_detached_retention_ns
            || !valid_u32(self.max_mutation_replays, hard.max_mutation_replays)
        {
            return Err(Error::Invalid("Process family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(crate::schema::process::LIMIT_MAX_ARGC, self.max_argc),
            limit_u32(
                crate::schema::process::LIMIT_MAX_ARG_BYTES,
                self.max_arg_bytes,
            ),
            limit_u32(crate::schema::process::LIMIT_MAX_ENVC, self.max_envc),
            limit_u32(
                crate::schema::process::LIMIT_MAX_ENV_BYTES,
                self.max_env_bytes,
            ),
            limit_u32(
                crate::schema::process::LIMIT_MAX_PROCESSES_PER_SESSION,
                self.max_processes_per_session,
            ),
            limit_u32(
                crate::schema::process::LIMIT_MAX_PROCESSES,
                self.max_processes,
            ),
            limit_u32(
                crate::schema::process::LIMIT_MAX_PENDING_SPAWNS,
                self.max_pending_spawns,
            ),
            limit_u64(
                crate::schema::process::LIMIT_MAX_STREAM_BUFFER_BYTES,
                self.max_stream_buffer_bytes,
            ),
            limit_u64(
                crate::schema::process::LIMIT_MAX_DETACHED_RETENTION_NS,
                self.max_detached_retention_ns,
            ),
            limit_u32(
                crate::schema::process::LIMIT_MAX_MUTATION_REPLAYS,
                self.max_mutation_replays,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        let known = [
            crate::schema::process::LIMIT_MAX_ARGC as u16,
            crate::schema::process::LIMIT_MAX_ARG_BYTES as u16,
            crate::schema::process::LIMIT_MAX_ENVC as u16,
            crate::schema::process::LIMIT_MAX_ENV_BYTES as u16,
            crate::schema::process::LIMIT_MAX_PROCESSES_PER_SESSION as u16,
            crate::schema::process::LIMIT_MAX_PROCESSES as u16,
            crate::schema::process::LIMIT_MAX_PENDING_SPAWNS as u16,
            crate::schema::process::LIMIT_MAX_STREAM_BUFFER_BYTES as u16,
            crate::schema::process::LIMIT_MAX_DETACHED_RETENTION_NS as u16,
            crate::schema::process::LIMIT_MAX_MUTATION_REPLAYS as u16,
        ];
        reject_unknown_required(extensions, &known)?;
        let value = Self {
            max_argc: read_limit_u32(extensions, crate::schema::process::LIMIT_MAX_ARGC)?,
            max_arg_bytes: read_limit_u32(extensions, crate::schema::process::LIMIT_MAX_ARG_BYTES)?,
            max_envc: read_limit_u32(extensions, crate::schema::process::LIMIT_MAX_ENVC)?,
            max_env_bytes: read_limit_u32(extensions, crate::schema::process::LIMIT_MAX_ENV_BYTES)?,
            max_processes_per_session: read_limit_u32(
                extensions,
                crate::schema::process::LIMIT_MAX_PROCESSES_PER_SESSION,
            )?,
            max_processes: read_limit_u32(extensions, crate::schema::process::LIMIT_MAX_PROCESSES)?,
            max_pending_spawns: read_limit_u32(
                extensions,
                crate::schema::process::LIMIT_MAX_PENDING_SPAWNS,
            )?,
            max_stream_buffer_bytes: read_limit_u64(
                extensions,
                crate::schema::process::LIMIT_MAX_STREAM_BUFFER_BYTES,
            )?,
            max_detached_retention_ns: read_limit_u64(
                extensions,
                crate::schema::process::LIMIT_MAX_DETACHED_RETENTION_NS,
            )?,
            max_mutation_replays: read_limit_u32(
                extensions,
                crate::schema::process::LIMIT_MAX_MUTATION_REPLAYS,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn validate_argv(argv: &[Vec<u8>]) -> Result<()> {
    if argv.is_empty() || argv.len() > MAX_ARGC {
        return Err(Error::Invalid("Process argv count"));
    }
    let mut total = 0usize;
    for arg in argv {
        validate_arg(arg)?;
        total = total.checked_add(arg.len()).ok_or(Error::LengthOverflow)?;
        if total > MAX_ARG_BYTES {
            return Err(limit(
                "Process argument bytes",
                total as u64,
                MAX_ARG_BYTES as u64,
            ));
        }
    }
    Ok(())
}

fn validate_arg(arg: &[u8]) -> Result<()> {
    if arg.is_empty() || arg.len() > MAX_ARG_LEN || arg.contains(&0) {
        return Err(Error::Invalid("Process argument"));
    }
    Ok(())
}

fn validate_env(env: &[EnvEntry]) -> Result<()> {
    if env.len() > MAX_ENVC {
        return Err(limit(
            "Process environment entries",
            env.len() as u64,
            MAX_ENVC as u64,
        ));
    }
    let mut previous: Option<&[u8]> = None;
    let mut total = 0usize;
    for entry in env {
        if entry.key.is_empty()
            || entry.key.len() > MAX_ENV_KEY_BYTES
            || entry.value.len() > MAX_ENV_VALUE_BYTES
            || entry.key.contains(&0)
            || entry.key.contains(&b'=')
            || entry.value.contains(&0)
            || previous.is_some_and(|old| old >= entry.key.as_slice())
        {
            return Err(Error::Invalid("Process environment entry"));
        }
        previous = Some(&entry.key);
        total = total
            .checked_add(entry.key.len())
            .and_then(|value| value.checked_add(entry.value.len()))
            .ok_or(Error::LengthOverflow)?;
        if total > MAX_ENV_BYTES {
            return Err(limit(
                "Process environment bytes",
                total as u64,
                MAX_ENV_BYTES as u64,
            ));
        }
    }
    Ok(())
}

fn validate_native_path(path: &[u8]) -> Result<()> {
    if path.is_empty() || path.len() > MAX_CWD_BYTES || path.contains(&0) {
        return Err(Error::Invalid("Process native cwd path"));
    }
    Ok(())
}

fn validate_component(component: &[u8]) -> Result<()> {
    if component.is_empty()
        || component == b"."
        || component == b".."
        || component.contains(&0)
        || component.contains(&b'/')
        || component.contains(&b'\\')
    {
        return Err(Error::Invalid("Process FS cwd component"));
    }
    Ok(())
}

fn validate_signal(signal: u16) -> Result<()> {
    if !(crate::schema::process::SIGNAL_INTERRUPT as u16
        ..=crate::schema::process::SIGNAL_HANGUP as u16)
        .contains(&signal)
    {
        return Err(Error::Invalid("Process portable signal"));
    }
    Ok(())
}

fn validate_stream_transfer(
    descriptor: &Descriptor,
    content_kind: u16,
    direction: Direction,
) -> Result<()> {
    let sensitive = descriptor.extensions.0.iter().any(|extension| {
        extension.tag == crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16
            && extension.required
            && extension.value.is_empty()
    });
    if descriptor.mode != Mode::Byte
        || descriptor.direction != direction
        || descriptor.content_family != crate::family::PROCESS
        || descriptor.content_kind != content_kind
        || descriptor.content_version != VERSION
        || !sensitive
    {
        return Err(Error::Invalid("Process stream Transfer descriptor"));
    }
    descriptor.validate()
}

fn validate_spawn_extensions(extensions: &Extensions) -> Result<()> {
    let known = [
        crate::schema::process::SPAWN_SURFACE_APP_EXTENSION as u16,
        crate::schema::process::SPAWN_RESOURCE_TAG_EXTENSION as u16,
    ];
    reject_unknown_required(extensions, &known)?;
    if let Some(handle) = extension_u64(
        extensions,
        crate::schema::process::SPAWN_SURFACE_APP_EXTENSION,
        "Process surface application extension",
    )? {
        validate_handle(handle, "Process surface application handle")?;
    }
    if let Some(tag) = extensions
        .0
        .iter()
        .find(|extension| {
            extension.tag == crate::schema::process::SPAWN_RESOURCE_TAG_EXTENSION as u16
        })
        .map(|extension| extension.value.as_slice())
        && tag.len() > 4096
    {
        return Err(limit("Process resource tag bytes", tag.len() as u64, 4096));
    }
    Ok(())
}

fn extension_u64(extensions: &Extensions, tag: u64, name: &'static str) -> Result<Option<u64>> {
    let Some(extension) = extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
    else {
        return Ok(None);
    };
    let value = u64::from_le_bytes(
        extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid(name))?,
    );
    Ok(Some(value))
}

fn validate_operation_id(operation_id: &[u8; 16]) -> Result<()> {
    if operation_id.iter().all(|byte| *byte == 0) {
        return Err(Error::Invalid("zero Process operation ID"));
    }
    Ok(())
}

fn validate_handle(handle: u64, name: &'static str) -> Result<()> {
    if handle == 0 {
        return Err(Error::Invalid(name));
    }
    Ok(())
}

fn reject_unknown_required(extensions: &Extensions, known: &[u16]) -> Result<()> {
    extensions.validate()?;
    if extensions
        .0
        .iter()
        .any(|extension| extension.required && !known.contains(&extension.tag))
    {
        return Err(Error::Invalid("unknown required Process extension"));
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

fn limit_u32(tag: u64, value: u32) -> Extension {
    Extension {
        tag: tag as u16,
        required: false,
        value: value.to_le_bytes().to_vec(),
    }
}

fn limit_u64(tag: u64, value: u64) -> Extension {
    Extension {
        tag: tag as u16,
        required: false,
        value: value.to_le_bytes().to_vec(),
    }
}

fn read_limit_u32(extensions: &Extensions, tag: u64) -> Result<u32> {
    let extension = extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
        .ok_or(Error::Invalid("missing Process family limit"))?;
    Ok(u32::from_le_bytes(
        extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("Process family limit length"))?,
    ))
}

fn read_limit_u64(extensions: &Extensions, tag: u64) -> Result<u64> {
    let extension = extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
        .ok_or(Error::Invalid("missing Process family limit"))?;
    Ok(u64::from_le_bytes(
        extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("Process family limit length"))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_truncation<T>(value: &T)
    where
        T: Encode + Decode + PartialEq + std::fmt::Debug,
    {
        let bytes = value.encode().unwrap();
        assert_eq!(T::decode(&bytes).unwrap(), *value);
        for end in 0..bytes.len() {
            assert!(T::decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
    }

    fn stream(id: u32, kind: u16, direction: Direction, send_credit: u64) -> Descriptor {
        Descriptor {
            transfer_id: id,
            mode: Mode::Byte,
            direction,
            receiver_send_credit: if direction.receiver_to_sender {
                65_536
            } else {
                0
            },
            sender_send_credit: if direction.sender_to_receiver {
                send_credit
            } else {
                0
            },
            max_item_bytes: 0,
            max_chunk_bytes: 4096,
            content_family: crate::family::PROCESS,
            content_kind: kind,
            content_version: VERSION,
            extensions: Extensions(vec![Extension {
                tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        }
    }

    #[test]
    fn cwd_and_spawn_reject_every_truncation() {
        every_truncation(&Cwd::Fs {
            root_handle: 9,
            components: vec![b"src".to_vec(), b"main.rs".to_vec()],
        });
        every_truncation(&Spawn {
            operation_id: [1; 16],
            flags: 0,
            environment_kind: EnvironmentKind::Session,
            cwd: Cwd::Path(b"/tmp".to_vec()),
            argv: vec![b"printf".to_vec(), b"hello".to_vec()],
            env: vec![EnvEntry {
                key: b"LANG".to_vec(),
                value: b"C".to_vec(),
            }],
            stdout_receive_credit: 65_536,
            stderr_receive_credit: 65_536,
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn bundle_and_exit_reject_every_truncation() {
        every_truncation(&StreamBundle {
            process_handle: 1,
            stdout_lifetime_offset: 2,
            stderr_lifetime_offset: 3,
            stdin: Some(stream(
                2,
                crate::schema::process::STREAM_STDIN_CONTENT_KIND as u16,
                Direction::RECEIVER_TO_SENDER,
                0,
            )),
            stdout: stream(
                4,
                crate::schema::process::STREAM_STDOUT_CONTENT_KIND as u16,
                Direction::SENDER_TO_RECEIVER,
                65_536,
            ),
            stderr: Some(stream(
                6,
                crate::schema::process::STREAM_STDERR_CONTENT_KIND as u16,
                Direction::SENDER_TO_RECEIVER,
                65_536,
            )),
            merged_stderr: false,
            extensions: Extensions::default(),
        });
        every_truncation(&ExitRecord {
            kind: ExitKind::Code,
            reason: crate::schema::process::EXIT_REASON_UNKNOWN as u8,
            code: 7,
            exited_server_ns: 123,
            detail: Vec::new(),
        });
    }

    #[test]
    fn state_control_attach_and_limits_round_trip() {
        every_truncation(&ProcessRecord {
            process_handle: 1,
            lifecycle: crate::schema::process::LIFECYCLE_RUNNING as u8,
            stream_state: crate::schema::process::STREAM_STDIN_OPEN as u8
                | crate::schema::process::STREAM_STDOUT_OPEN as u8
                | crate::schema::process::STREAM_STDERR_OPEN as u8,
            flags: 0,
            native_pid: 42,
            owner_session: [2; 16],
            argv0: b"sleep".to_vec(),
            stdin_received: 0,
            stdout_produced: 4,
            stderr_produced: 5,
            retention_deadline_server_ns: 0,
            exit: None,
            extensions: Extensions::default(),
        });
        every_truncation(&Attach {
            process_handle: 1,
            flags: crate::schema::process::ATTACH_STDIN as u16,
            stdout_receive_credit: 4096,
            stderr_receive_credit: 4096,
            extensions: Extensions::default(),
        });
        every_truncation(&Control {
            process_handle: 1,
            operation_id: [3; 16],
            action: ControlAction::Signal,
            value: crate::schema::process::SIGNAL_INTERRUPT as u16,
            extensions: Extensions::default(),
        });
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);
    }

    #[test]
    fn invalid_environment_and_transfer_policy_fail() {
        let mut spawn = Spawn {
            operation_id: [1; 16],
            flags: 0,
            environment_kind: EnvironmentKind::Empty,
            cwd: Cwd::ServerDefault,
            argv: vec![b"true".to_vec()],
            env: vec![
                EnvEntry {
                    key: b"B".to_vec(),
                    value: Vec::new(),
                },
                EnvEntry {
                    key: b"A".to_vec(),
                    value: Vec::new(),
                },
            ],
            stdout_receive_credit: 1,
            stderr_receive_credit: 1,
            extensions: Extensions::default(),
        };
        assert!(spawn.encode().is_err());
        spawn.env.reverse();
        assert!(spawn.encode().is_ok());
        let mut output = stream(
            2,
            crate::schema::process::STREAM_STDOUT_CONTENT_KIND as u16,
            Direction::SENDER_TO_RECEIVER,
            1,
        );
        output.extensions = Extensions::default();
        assert!(
            validate_stream_transfer(
                &output,
                crate::schema::process::STREAM_STDOUT_CONTENT_KIND as u16,
                Direction::SENDER_TO_RECEIVER,
            )
            .is_err()
        );
    }
}
