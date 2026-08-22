//! YAS Wasmi/QuickJS extension-supervisor family wire values.

use crate::codec::{
    Decode, Decoder, Encode, Error, Extensions, Result, limit_u32, limit_u64, put_bytes_u32,
    put_i32, put_len_u16, put_string_u16, put_string_u32, put_u16, put_u32, put_u64,
    read_limit_u32, read_limit_u64,
};
use crate::prelude::*;
use crate::state::{Record, RecordKind};
use crate::transfer::{Descriptor, Direction, Mode, Reset, UploadStage};

pub const VERSION: u16 = crate::schema::extension::VERSION;
pub const MAX_NAME_BYTES: usize = crate::schema::extension::MAX_NAME_BYTES as usize;
pub const MAX_ARGS: usize = crate::schema::extension::MAX_ARGS as usize;
pub const MAX_ARG_BYTES: usize = crate::schema::extension::MAX_ARG_BYTES as usize;
pub const MAX_ARGUMENT_BYTES: usize = crate::schema::extension::MAX_ARGUMENT_BYTES as usize;
pub const MAX_OBJECT_BYTES: u64 = crate::schema::extension::MAX_OBJECT_BYTES;
pub const MAX_OUTPUT_RECORD_BYTES: usize =
    crate::schema::extension::MAX_OUTPUT_RECORD_BYTES as usize;
pub const MAX_OUTPUT_BATCH_BYTES: usize = crate::schema::extension::MAX_OUTPUT_BATCH_BYTES as usize;
pub const MAX_COMMAND_DESCRIPTOR_BYTES: usize =
    crate::schema::extension::MAX_COMMAND_DESCRIPTOR_BYTES as usize;
pub const MAX_COMMAND_RECORDS: usize = crate::schema::extension::MAX_COMMAND_RECORDS as usize;
pub const MAX_COMMAND_PAGE_BYTES: usize = crate::schema::extension::MAX_COMMAND_PAGE_BYTES as usize;
pub const MAX_NEXT_START_UNIX_MS: u64 = crate::schema::extension::MAX_NEXT_START_UNIX_MS;

pub mod request_kind {
    pub use crate::schema::extension::request::*;
}

pub mod event_kind {
    pub use crate::schema::extension::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_name_bytes: u32,
    pub max_args: u32,
    pub max_argument_bytes: u64,
    pub max_object_bytes: u64,
    pub max_output_record_bytes: u32,
    pub max_command_descriptor_bytes: u32,
    pub max_command_records: u32,
    pub max_definitions: u32,
    pub max_object_stages_per_session: u32,
    pub max_follows_per_session: u32,
    pub max_running_attempts: u32,
    pub max_memory_bytes: u64,
    pub max_job_bytes: u64,
    pub max_mutation_replays: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_name_bytes: crate::schema::extension::MAX_NAME_BYTES as u32,
        max_args: crate::schema::extension::MAX_ARGS as u32,
        max_argument_bytes: crate::schema::extension::MAX_ARGUMENT_BYTES,
        max_object_bytes: crate::schema::extension::MAX_OBJECT_BYTES,
        max_output_record_bytes: crate::schema::extension::MAX_OUTPUT_RECORD_BYTES as u32,
        max_command_descriptor_bytes: crate::schema::extension::MAX_COMMAND_DESCRIPTOR_BYTES as u32,
        max_command_records: crate::schema::extension::MAX_COMMAND_RECORDS as u32,
        max_definitions: crate::schema::extension::MAX_DEFINITIONS as u32,
        max_object_stages_per_session: crate::schema::extension::MAX_OBJECT_STAGES_PER_SESSION
            as u32,
        max_follows_per_session: crate::schema::extension::MAX_FOLLOWS_PER_SESSION as u32,
        max_running_attempts: crate::schema::extension::MAX_RUNNING_ATTEMPTS as u32,
        max_memory_bytes: crate::schema::extension::MAX_MEMORY_BYTES,
        max_job_bytes: crate::schema::extension::MAX_JOB_BYTES,
        max_mutation_replays: crate::schema::extension::MAX_MUTATION_REPLAYS as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        if [
            (self.max_name_bytes, hard.max_name_bytes),
            (self.max_args, hard.max_args),
            (self.max_output_record_bytes, hard.max_output_record_bytes),
            (
                self.max_command_descriptor_bytes,
                hard.max_command_descriptor_bytes,
            ),
            (self.max_command_records, hard.max_command_records),
            (self.max_definitions, hard.max_definitions),
            (
                self.max_object_stages_per_session,
                hard.max_object_stages_per_session,
            ),
            (self.max_follows_per_session, hard.max_follows_per_session),
            (self.max_running_attempts, hard.max_running_attempts),
            (self.max_mutation_replays, hard.max_mutation_replays),
        ]
        .into_iter()
        .any(|(value, maximum)| value == 0 || value > maximum)
            || [
                (self.max_argument_bytes, hard.max_argument_bytes),
                (self.max_object_bytes, hard.max_object_bytes),
                (self.max_memory_bytes, hard.max_memory_bytes),
                (self.max_job_bytes, hard.max_job_bytes),
            ]
            .into_iter()
            .any(|(value, maximum)| value == 0 || value > maximum)
        {
            return Err(Error::Invalid("Extension family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::extension::LIMIT_MAX_NAME_BYTES,
                self.max_name_bytes,
            ),
            limit_u32(crate::schema::extension::LIMIT_MAX_ARGS, self.max_args),
            limit_u64(
                crate::schema::extension::LIMIT_MAX_ARGUMENT_BYTES,
                self.max_argument_bytes,
            ),
            limit_u64(
                crate::schema::extension::LIMIT_MAX_OBJECT_BYTES,
                self.max_object_bytes,
            ),
            limit_u32(
                crate::schema::extension::LIMIT_MAX_OUTPUT_RECORD_BYTES,
                self.max_output_record_bytes,
            ),
            limit_u32(
                crate::schema::extension::LIMIT_MAX_COMMAND_DESCRIPTOR_BYTES,
                self.max_command_descriptor_bytes,
            ),
            limit_u32(
                crate::schema::extension::LIMIT_MAX_COMMAND_RECORDS,
                self.max_command_records,
            ),
            limit_u32(
                crate::schema::extension::LIMIT_MAX_DEFINITIONS,
                self.max_definitions,
            ),
            limit_u32(
                crate::schema::extension::LIMIT_MAX_OBJECT_STAGES_PER_SESSION,
                self.max_object_stages_per_session,
            ),
            limit_u32(
                crate::schema::extension::LIMIT_MAX_FOLLOWS_PER_SESSION,
                self.max_follows_per_session,
            ),
            limit_u32(
                crate::schema::extension::LIMIT_MAX_RUNNING_ATTEMPTS,
                self.max_running_attempts,
            ),
            limit_u64(
                crate::schema::extension::LIMIT_MAX_MEMORY_BYTES,
                self.max_memory_bytes,
            ),
            limit_u64(
                crate::schema::extension::LIMIT_MAX_JOB_BYTES,
                self.max_job_bytes,
            ),
            limit_u32(
                crate::schema::extension::LIMIT_MAX_MUTATION_REPLAYS,
                self.max_mutation_replays,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required(
            extensions,
            &[
                crate::schema::extension::LIMIT_MAX_NAME_BYTES as u16,
                crate::schema::extension::LIMIT_MAX_ARGS as u16,
                crate::schema::extension::LIMIT_MAX_ARGUMENT_BYTES as u16,
                crate::schema::extension::LIMIT_MAX_OBJECT_BYTES as u16,
                crate::schema::extension::LIMIT_MAX_OUTPUT_RECORD_BYTES as u16,
                crate::schema::extension::LIMIT_MAX_COMMAND_DESCRIPTOR_BYTES as u16,
                crate::schema::extension::LIMIT_MAX_COMMAND_RECORDS as u16,
                crate::schema::extension::LIMIT_MAX_DEFINITIONS as u16,
                crate::schema::extension::LIMIT_MAX_OBJECT_STAGES_PER_SESSION as u16,
                crate::schema::extension::LIMIT_MAX_FOLLOWS_PER_SESSION as u16,
                crate::schema::extension::LIMIT_MAX_RUNNING_ATTEMPTS as u16,
                crate::schema::extension::LIMIT_MAX_MEMORY_BYTES as u16,
                crate::schema::extension::LIMIT_MAX_JOB_BYTES as u16,
                crate::schema::extension::LIMIT_MAX_MUTATION_REPLAYS as u16,
            ],
        )?;
        let value = Self {
            max_name_bytes: read_limit_u32(
                extensions,
                crate::schema::extension::LIMIT_MAX_NAME_BYTES,
            )?,
            max_args: read_limit_u32(extensions, crate::schema::extension::LIMIT_MAX_ARGS)?,
            max_argument_bytes: read_limit_u64(
                extensions,
                crate::schema::extension::LIMIT_MAX_ARGUMENT_BYTES,
            )?,
            max_object_bytes: read_limit_u64(
                extensions,
                crate::schema::extension::LIMIT_MAX_OBJECT_BYTES,
            )?,
            max_output_record_bytes: read_limit_u32(
                extensions,
                crate::schema::extension::LIMIT_MAX_OUTPUT_RECORD_BYTES,
            )?,
            max_command_descriptor_bytes: read_limit_u32(
                extensions,
                crate::schema::extension::LIMIT_MAX_COMMAND_DESCRIPTOR_BYTES,
            )?,
            max_command_records: read_limit_u32(
                extensions,
                crate::schema::extension::LIMIT_MAX_COMMAND_RECORDS,
            )?,
            max_definitions: read_limit_u32(
                extensions,
                crate::schema::extension::LIMIT_MAX_DEFINITIONS,
            )?,
            max_object_stages_per_session: read_limit_u32(
                extensions,
                crate::schema::extension::LIMIT_MAX_OBJECT_STAGES_PER_SESSION,
            )?,
            max_follows_per_session: read_limit_u32(
                extensions,
                crate::schema::extension::LIMIT_MAX_FOLLOWS_PER_SESSION,
            )?,
            max_running_attempts: read_limit_u32(
                extensions,
                crate::schema::extension::LIMIT_MAX_RUNNING_ATTEMPTS,
            )?,
            max_memory_bytes: read_limit_u64(
                extensions,
                crate::schema::extension::LIMIT_MAX_MEMORY_BYTES,
            )?,
            max_job_bytes: read_limit_u64(
                extensions,
                crate::schema::extension::LIMIT_MAX_JOB_BYTES,
            )?,
            max_mutation_replays: read_limit_u32(
                extensions,
                crate::schema::extension::LIMIT_MAX_MUTATION_REPLAYS,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Runtime {
    Auto = crate::schema::extension::RUNTIME_AUTO as u8,
    Wasmi = crate::schema::extension::RUNTIME_WASMI as u8,
    QuickJs = crate::schema::extension::RUNTIME_QUICKJS as u8,
}

impl TryFrom<u8> for Runtime {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Auto as u8 => Ok(Self::Auto),
            value if value == Self::Wasmi as u8 => Ok(Self::Wasmi),
            value if value == Self::QuickJs as u8 => Ok(Self::QuickJs),
            _ => Err(Error::Invalid("Extension runtime")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RestartPolicy {
    Never = crate::schema::extension::RESTART_NEVER as u8,
    OnFailure = crate::schema::extension::RESTART_ON_FAILURE as u8,
    Always = crate::schema::extension::RESTART_ALWAYS as u8,
}

impl TryFrom<u8> for RestartPolicy {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Never as u8 => Ok(Self::Never),
            value if value == Self::OnFailure as u8 => Ok(Self::OnFailure),
            value if value == Self::Always as u8 => Ok(Self::Always),
            _ => Err(Error::Invalid("Extension restart policy")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    NeedObject = crate::schema::extension::PHASE_NEED_OBJECT as u8,
    Validating = crate::schema::extension::PHASE_VALIDATING as u8,
    Queued = crate::schema::extension::PHASE_QUEUED as u8,
    Running = crate::schema::extension::PHASE_RUNNING as u8,
    Backoff = crate::schema::extension::PHASE_BACKOFF as u8,
    Stopped = crate::schema::extension::PHASE_STOPPED as u8,
    Blocked = crate::schema::extension::PHASE_BLOCKED as u8,
    Stopping = crate::schema::extension::PHASE_STOPPING as u8,
}

impl TryFrom<u8> for Phase {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::NeedObject as u8 => Ok(Self::NeedObject),
            value if value == Self::Validating as u8 => Ok(Self::Validating),
            value if value == Self::Queued as u8 => Ok(Self::Queued),
            value if value == Self::Running as u8 => Ok(Self::Running),
            value if value == Self::Backoff as u8 => Ok(Self::Backoff),
            value if value == Self::Stopped as u8 => Ok(Self::Stopped),
            value if value == Self::Blocked as u8 => Ok(Self::Blocked),
            value if value == Self::Stopping as u8 => Ok(Self::Stopping),
            _ => Err(Error::Invalid("Extension phase")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ControlAction {
    Stop = crate::schema::extension::CONTROL_STOP as u8,
    Start = crate::schema::extension::CONTROL_START as u8,
    Restart = crate::schema::extension::CONTROL_RESTART as u8,
    Enable = crate::schema::extension::CONTROL_ENABLE as u8,
    Disable = crate::schema::extension::CONTROL_DISABLE as u8,
    Remove = crate::schema::extension::CONTROL_REMOVE as u8,
}

impl TryFrom<u8> for ControlAction {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Stop as u8 => Ok(Self::Stop),
            value if value == Self::Start as u8 => Ok(Self::Start),
            value if value == Self::Restart as u8 => Ok(Self::Restart),
            value if value == Self::Enable as u8 => Ok(Self::Enable),
            value if value == Self::Disable as u8 => Ok(Self::Disable),
            value if value == Self::Remove as u8 => Ok(Self::Remove),
            _ => Err(Error::Invalid("Extension control action")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitKind {
    Returned = crate::schema::extension::EXIT_RETURNED as u8,
    Trapped = crate::schema::extension::EXIT_TRAPPED as u8,
    Cancelled = crate::schema::extension::EXIT_CANCELLED as u8,
    Updated = crate::schema::extension::EXIT_UPDATED as u8,
    SlowConsumer = crate::schema::extension::EXIT_SLOW_CONSUMER as u8,
    ProtocolViolation = crate::schema::extension::EXIT_PROTOCOL_VIOLATION as u8,
    HostFailure = crate::schema::extension::EXIT_HOST_FAILURE as u8,
    ServerShutdown = crate::schema::extension::EXIT_SERVER_SHUTDOWN as u8,
    ResourceLimit = crate::schema::extension::EXIT_RESOURCE_LIMIT as u8,
}

impl TryFrom<u8> for ExitKind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Returned as u8 => Ok(Self::Returned),
            value if value == Self::Trapped as u8 => Ok(Self::Trapped),
            value if value == Self::Cancelled as u8 => Ok(Self::Cancelled),
            value if value == Self::Updated as u8 => Ok(Self::Updated),
            value if value == Self::SlowConsumer as u8 => Ok(Self::SlowConsumer),
            value if value == Self::ProtocolViolation as u8 => Ok(Self::ProtocolViolation),
            value if value == Self::HostFailure as u8 => Ok(Self::HostFailure),
            value if value == Self::ServerShutdown as u8 => Ok(Self::ServerShutdown),
            value if value == Self::ResourceLimit as u8 => Ok(Self::ResourceLimit),
            _ => Err(Error::Invalid("Extension exit kind")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OutputKind {
    Stdout = crate::schema::extension::OUTPUT_STDOUT as u8,
    Stderr = crate::schema::extension::OUTPUT_STDERR as u8,
    Log = crate::schema::extension::OUTPUT_LOG as u8,
    Gap = crate::schema::extension::OUTPUT_GAP as u8,
}

impl TryFrom<u8> for OutputKind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Stdout as u8 => Ok(Self::Stdout),
            value if value == Self::Stderr as u8 => Ok(Self::Stderr),
            value if value == Self::Log as u8 => Ok(Self::Log),
            value if value == Self::Gap as u8 => Ok(Self::Gap),
            _ => Err(Error::Invalid("Extension output kind")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeLimits {
    /// Zero requests the server default; state records report applied values.
    pub memory_bytes: u64,
    pub stack_bytes: u64,
    pub max_active_jobs: u32,
    pub max_pending_jobs: u32,
    pub max_job_bytes: u64,
    pub slow_consumer_timeout_ns: u64,
    pub extensions: Extensions,
}

impl RuntimeLimits {
    fn validate(&self) -> Result<()> {
        if self.memory_bytes > crate::schema::extension::MAX_MEMORY_BYTES
            || self.stack_bytes > crate::schema::extension::MAX_STACK_BYTES
            || self.max_active_jobs > crate::schema::extension::MAX_ACTIVE_JOBS as u32
            || self.max_pending_jobs > crate::schema::extension::MAX_PENDING_JOBS as u32
            || self.max_job_bytes > crate::schema::extension::MAX_JOB_BYTES
        {
            return Err(Error::Invalid("Extension runtime limits"));
        }
        reject_unknown_required(&self.extensions, &[])
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            memory_bytes: decoder.u64()?,
            stack_bytes: decoder.u64()?,
            max_active_jobs: decoder.u32()?,
            max_pending_jobs: decoder.u32()?,
            max_job_bytes: decoder.u64()?,
            slow_consumer_timeout_ns: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for RuntimeLimits {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.memory_bytes);
        put_u64(out, self.stack_bytes);
        put_u32(out, self.max_active_jobs);
        put_u32(out, self.max_pending_jobs);
        put_u64(out, self.max_job_bytes);
        put_u64(out, self.slow_consumer_timeout_ns);
        self.extensions.encode_tail(out)
    }
}

impl Decode for RuntimeLimits {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectBegin {
    pub operation_id: [u8; 16],
    pub content_hash: [u8; 32],
    pub byte_len: u64,
    pub extensions: Extensions,
}

impl Encode for ObjectBegin {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_operation_id(&self.operation_id)?;
        validate_object_len(self.byte_len)?;
        reject_unknown_required(&self.extensions, &[])?;
        out.extend_from_slice(&self.operation_id);
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.byte_len);
        self.extensions.encode_tail(out)
    }
}

impl Decode for ObjectBegin {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            operation_id: decoder.array_16()?,
            content_hash: decoder.array_32()?,
            byte_len: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectDisposition {
    AlreadyPresent = crate::schema::extension::OBJECT_ALREADY_PRESENT as u8,
    Upload = crate::schema::extension::OBJECT_UPLOAD as u8,
}

impl TryFrom<u8> for ObjectDisposition {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::AlreadyPresent as u8 => Ok(Self::AlreadyPresent),
            value if value == Self::Upload as u8 => Ok(Self::Upload),
            _ => Err(Error::Invalid("Extension object disposition")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectBeginResult {
    pub disposition: ObjectDisposition,
    pub staging_handle: u64,
    pub descriptor: Option<Descriptor>,
    pub extensions: Extensions,
}

impl ObjectBeginResult {
    fn validate(&self) -> Result<()> {
        match (self.disposition, self.staging_handle, &self.descriptor) {
            (ObjectDisposition::AlreadyPresent, 0, None) => {}
            (ObjectDisposition::Upload, handle, Some(descriptor)) if handle != 0 => {
                validate_object_descriptor(descriptor)?;
                descriptor.require_upload_stage(handle)?;
            }
            _ => return Err(Error::Invalid("Extension object-begin Result")),
        }
        reject_unknown_required(&self.extensions, &[])
    }

    /// Return the upload stage atomically discarded by `reset`, or `None`
    /// when the RESET belongs to another Transfer.
    pub fn stage_discarded_by(&self, reset: &Reset) -> Result<Option<UploadStage>> {
        self.validate()?;
        match &self.descriptor {
            Some(descriptor) => {
                reset.disposed_upload_stage_from(self.staging_handle, core::iter::once(descriptor))
            }
            None => Ok(None),
        }
    }
}

impl Encode for ObjectBeginResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.disposition as u8);
        out.extend_from_slice(&[0; 7]);
        put_u64(out, self.staging_handle);
        match &self.descriptor {
            Some(descriptor) => put_bytes_u32(out, &descriptor.encode()?)?,
            None => put_bytes_u32(out, &[])?,
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for ObjectBeginResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let disposition = ObjectDisposition::try_from(decoder.u8()?)?;
        if decoder.take(7)? != [0; 7] {
            return Err(Error::Invalid("Extension object-begin reserved bytes"));
        }
        let staging_handle = decoder.u64()?;
        let descriptor = decoder.len_bytes_u32()?;
        let descriptor = if descriptor.is_empty() {
            None
        } else {
            Some(Descriptor::decode(descriptor)?)
        };
        let value = Self {
            disposition,
            staging_handle,
            descriptor,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectCommit {
    pub staging_handle: u64,
    pub operation_id: [u8; 16],
    pub content_hash: [u8; 32],
    pub byte_len: u64,
    pub extensions: Extensions,
}

impl Encode for ObjectCommit {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.staging_handle, "Extension staging handle")?;
        validate_operation_id(&self.operation_id)?;
        validate_object_len(self.byte_len)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.staging_handle);
        out.extend_from_slice(&self.operation_id);
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.byte_len);
        self.extensions.encode_tail(out)
    }
}

impl Decode for ObjectCommit {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            staging_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
            content_hash: decoder.array_32()?,
            byte_len: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Deploy {
    pub operation_id: [u8; 16],
    /// Zero only for creation; replacement requires the complete current
    /// handle/generation/revision tuple to prevent name-reuse ABA races.
    pub expected_extension_handle: u64,
    pub expected_generation: u64,
    pub expected_definition_revision: u64,
    pub flags: u16,
    pub runtime: Runtime,
    pub restart_policy: RestartPolicy,
    pub name: String,
    pub content_hash: [u8; 32],
    pub argv: Vec<Vec<u8>>,
    pub runtime_limits: RuntimeLimits,
    pub extensions: Extensions,
}

impl Deploy {
    fn validate(&self) -> Result<()> {
        validate_operation_id(&self.operation_id)?;
        let expected = (
            self.expected_extension_handle,
            self.expected_generation,
            self.expected_definition_revision,
        );
        if !matches!(expected, (0, 0, 0)) && (expected.0 == 0 || expected.1 == 0 || expected.2 == 0)
        {
            return Err(Error::Invalid("partial Extension deploy identity"));
        }
        validate_definition_flags(self.flags)?;
        validate_name(
            &self.name,
            self.flags & crate::schema::extension::DEFINITION_PERSISTENT as u16 != 0,
        )?;
        validate_argv(&self.argv)?;
        self.runtime_limits.validate()?;
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for Deploy {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.extend_from_slice(&self.operation_id);
        put_u64(out, self.expected_extension_handle);
        put_u64(out, self.expected_generation);
        put_u64(out, self.expected_definition_revision);
        put_u16(out, self.flags);
        out.push(self.runtime as u8);
        out.push(self.restart_policy as u8);
        put_string_u16(out, &self.name)?;
        out.extend_from_slice(&self.content_hash);
        put_len_u16(out, self.argv.len())?;
        for arg in &self.argv {
            put_bytes_u32(out, arg)?;
        }
        put_bytes_u32(out, &self.runtime_limits.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for Deploy {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let operation_id = decoder.array_16()?;
        let expected_extension_handle = decoder.u64()?;
        let expected_generation = decoder.u64()?;
        let expected_definition_revision = decoder.u64()?;
        let flags = decoder.u16()?;
        let runtime = Runtime::try_from(decoder.u8()?)?;
        let restart_policy = RestartPolicy::try_from(decoder.u8()?)?;
        let name = decoder.string_u16()?;
        let content_hash = decoder.array_32()?;
        let argc = usize::from(decoder.u16()?);
        if argc > MAX_ARGS || argc > decoder.remaining() / 4 {
            return Err(Error::Invalid("Extension argument count"));
        }
        let mut argv = Vec::with_capacity(argc);
        for _ in 0..argc {
            argv.push(decoder.len_bytes_u32()?.to_vec());
        }
        let value = Self {
            operation_id,
            expected_extension_handle,
            expected_generation,
            expected_definition_revision,
            flags,
            runtime,
            restart_policy,
            name,
            content_hash,
            argv,
            runtime_limits: RuntimeLimits::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefinitionIdentity {
    pub extension_handle: u64,
    pub generation: u64,
    pub definition_revision: u64,
    pub extensions: Extensions,
}

impl DefinitionIdentity {
    fn validate(&self) -> Result<()> {
        validate_identity(self.extension_handle, self.generation)?;
        if self.definition_revision == 0 {
            return Err(Error::Invalid("zero Extension definition revision"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for DefinitionIdentity {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.extension_handle);
        put_u64(out, self.generation);
        put_u64(out, self.definition_revision);
        self.extensions.encode_tail(out)
    }
}

impl Decode for DefinitionIdentity {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            extension_handle: decoder.u64()?,
            generation: decoder.u64()?,
            definition_revision: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub extension_handle: u64,
    pub generation: u64,
    /// Zero means no revision precondition.
    pub expected_definition_revision: u64,
    pub operation_id: [u8; 16],
    pub action: ControlAction,
    pub extensions: Extensions,
}

impl Encode for Control {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_identity(self.extension_handle, self.generation)?;
        validate_operation_id(&self.operation_id)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.extension_handle);
        put_u64(out, self.generation);
        put_u64(out, self.expected_definition_revision);
        out.extend_from_slice(&self.operation_id);
        out.push(self.action as u8);
        out.extend_from_slice(&[0; 7]);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Control {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let extension_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let expected_definition_revision = decoder.u64()?;
        let operation_id = decoder.array_16()?;
        let action = ControlAction::try_from(decoder.u8()?)?;
        if decoder.take(7)? != [0; 7] {
            return Err(Error::Invalid("Extension control reserved bytes"));
        }
        let value = Self {
            extension_handle,
            generation,
            expected_definition_revision,
            operation_id,
            action,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Follow {
    pub extension_handle: u64,
    pub generation: u64,
    /// Zero follows the current/latest attempt.
    pub attempt: u64,
    pub from_sequence: u64,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Encode for Follow {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_identity(self.extension_handle, self.generation)?;
        if self.initial_receive_credit == 0 {
            return Err(Error::Invalid("zero Extension follow receive credit"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.extension_handle);
        put_u64(out, self.generation);
        put_u64(out, self.attempt);
        put_u64(out, self.from_sequence);
        put_u64(out, self.initial_receive_credit);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Follow {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            extension_handle: decoder.u64()?,
            generation: decoder.u64()?,
            attempt: decoder.u64()?,
            from_sequence: decoder.u64()?,
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
pub struct FollowResult {
    pub attempt: u64,
    pub first_sequence: u64,
    pub through_sequence: u64,
    pub descriptor: Descriptor,
    pub extensions: Extensions,
}

impl FollowResult {
    fn validate(&self) -> Result<()> {
        if self.attempt == 0 || self.first_sequence > self.through_sequence.saturating_add(1) {
            return Err(Error::Invalid("Extension follow sequence range"));
        }
        validate_follow_descriptor(&self.descriptor)?;
        reject_unknown_required(&self.extensions, &[])
    }

    /// Whether terminating `transfer_id` with CLOSE or RESET ends this
    /// session's native attachment. Dropping the follow or its session has
    /// the same mandatory detach/unfollow effect.
    pub fn transfer_termination_detaches(&self, transfer_id: u32) -> Result<bool> {
        self.validate()?;
        Ok(self.descriptor.transfer_id == transfer_id)
    }
}

impl Encode for FollowResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.attempt);
        put_u64(out, self.first_sequence);
        put_u64(out, self.through_sequence);
        put_bytes_u32(out, &self.descriptor.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for FollowResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            attempt: decoder.u64()?,
            first_sequence: decoder.u64()?,
            through_sequence: decoder.u64()?,
            descriptor: Descriptor::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitRecord {
    pub kind: ExitKind,
    pub code: i32,
    pub attempt: u64,
    pub server_ns: u64,
    pub detail: String,
    pub extensions: Extensions,
}

impl ExitRecord {
    fn validate(&self) -> Result<()> {
        if self.attempt == 0 || self.detail.len() > 4096 {
            return Err(Error::Invalid("Extension exit record"));
        }
        if self.kind != ExitKind::Returned && self.code != 0 {
            return Err(Error::Invalid("Extension non-return exit code"));
        }
        reject_unknown_required(&self.extensions, &[])
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let kind = ExitKind::try_from(decoder.u8()?)?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Extension exit reserved bytes"));
        }
        let value = Self {
            kind,
            code: decoder.i32()?,
            attempt: decoder.u64()?,
            server_ns: decoder.u64()?,
            detail: decoder.string_u32()?,
            extensions: decoder.extensions()?,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for ExitRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.kind as u8);
        out.extend_from_slice(&[0; 3]);
        put_i32(out, self.code);
        put_u64(out, self.attempt);
        put_u64(out, self.server_ns);
        put_string_u32(out, &self.detail)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for ExitRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionRecord {
    pub extension_handle: u64,
    pub generation: u64,
    pub definition_revision: u64,
    pub phase: Phase,
    pub runtime: Runtime,
    pub restart_policy: RestartPolicy,
    pub flags: u16,
    pub attempt: u64,
    pub last_running_attempt: u64,
    pub task_id: u32,
    /// Exact Unix time in milliseconds; zero means no scheduled restart.
    pub next_start_unix_ms: u64,
    pub directory_revision: u64,
    pub content_hash: [u8; 32],
    pub name: String,
    pub last_exit: Option<ExitRecord>,
    pub runtime_limits: RuntimeLimits,
    pub extensions: Extensions,
}

impl ExtensionRecord {
    fn validate(&self) -> Result<()> {
        validate_identity(self.extension_handle, self.generation)?;
        if self.definition_revision == 0
            || self.runtime == Runtime::Auto && self.phase != Phase::NeedObject
            || self.last_running_attempt > self.attempt
        {
            return Err(Error::Invalid("Extension state identity"));
        }
        validate_definition_flags(self.flags)?;
        validate_name(
            &self.name,
            self.flags & crate::schema::extension::DEFINITION_PERSISTENT as u16 != 0,
        )?;
        if self.phase == Phase::Running && (self.attempt == 0 || self.task_id == 0) {
            return Err(Error::Invalid("running Extension attempt identity"));
        }
        if self.phase == Phase::Backoff
            && (self.next_start_unix_ms == 0 || self.next_start_unix_ms > MAX_NEXT_START_UNIX_MS)
        {
            return Err(Error::Invalid("Extension backoff deadline"));
        }
        if self.phase != Phase::Backoff && self.next_start_unix_ms != 0 {
            return Err(Error::Invalid("Extension non-backoff deadline"));
        }
        if let Some(exit) = &self.last_exit {
            exit.validate()?;
            if exit.attempt > self.attempt {
                return Err(Error::Invalid("Extension exit attempt"));
            }
        }
        self.runtime_limits.validate()?;
        reject_unknown_required(&self.extensions, &[])
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Extension state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }

    pub fn from_state_record(record: &Record) -> Result<Self> {
        if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Extension state record kind"));
        }
        Self::decode(&record.body)
    }
}

impl Encode for ExtensionRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.extension_handle);
        put_u64(out, self.generation);
        put_u64(out, self.definition_revision);
        out.push(self.phase as u8);
        out.push(self.runtime as u8);
        out.push(self.restart_policy as u8);
        out.push(0);
        put_u16(out, self.flags);
        put_u16(out, 0);
        put_u64(out, self.attempt);
        put_u64(out, self.last_running_attempt);
        put_u32(out, self.task_id);
        put_u32(out, 0);
        put_u64(out, self.next_start_unix_ms);
        put_u64(out, self.directory_revision);
        out.extend_from_slice(&self.content_hash);
        put_string_u16(out, &self.name)?;
        match &self.last_exit {
            Some(exit) => put_bytes_u32(out, &exit.encode()?)?,
            None => put_bytes_u32(out, &[])?,
        }
        put_bytes_u32(out, &self.runtime_limits.encode()?)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for ExtensionRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let extension_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let definition_revision = decoder.u64()?;
        let phase = Phase::try_from(decoder.u8()?)?;
        let runtime = Runtime::try_from(decoder.u8()?)?;
        let restart_policy = RestartPolicy::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Extension state reserved byte"));
        }
        let flags = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Extension state reserved field"));
        }
        let attempt = decoder.u64()?;
        let last_running_attempt = decoder.u64()?;
        let task_id = decoder.u32()?;
        if decoder.u32()? != 0 {
            return Err(Error::Invalid("Extension state reserved field"));
        }
        let next_start_unix_ms = decoder.u64()?;
        let directory_revision = decoder.u64()?;
        let content_hash = decoder.array_32()?;
        let name = decoder.string_u16()?;
        let exit = decoder.len_bytes_u32()?;
        let last_exit = if exit.is_empty() {
            None
        } else {
            Some(ExitRecord::decode(exit)?)
        };
        let value = Self {
            extension_handle,
            generation,
            definition_revision,
            phase,
            runtime,
            restart_policy,
            flags,
            attempt,
            last_running_attempt,
            task_id,
            next_start_unix_ms,
            directory_revision,
            content_hash,
            name,
            last_exit,
            runtime_limits: RuntimeLimits::decode(decoder.len_bytes_u32()?)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovedExtension {
    pub extension_handle: u64,
    pub generation: u64,
}

impl RemovedExtension {
    pub fn state_record(self) -> Result<Record> {
        validate_identity(self.extension_handle, self.generation)?;
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body: self.encode()?,
        })
    }

    pub fn from_state_record(record: &Record) -> Result<Self> {
        if record.kind != RecordKind::Remove {
            return Err(Error::Invalid("Extension remove state record kind"));
        }
        Self::decode(&record.body)
    }
}

impl Encode for RemovedExtension {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_identity(self.extension_handle, self.generation)?;
        put_u64(out, self.extension_handle);
        put_u64(out, self.generation);
        Ok(())
    }
}

impl Decode for RemovedExtension {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            extension_handle: decoder.u64()?,
            generation: decoder.u64()?,
        };
        decoder.finish()?;
        validate_identity(value.extension_handle, value.generation)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptContext {
    pub extension_handle: u64,
    pub generation: u64,
    pub definition_revision: u64,
    pub attempt: u64,
    pub task_id: u32,
    pub flags: u16,
    pub runtime: Runtime,
    pub content_hash: [u8; 32],
    pub name: String,
    pub argv: Vec<Vec<u8>>,
    pub extensions: Extensions,
}

impl AttemptContext {
    fn validate(&self) -> Result<()> {
        validate_identity(self.extension_handle, self.generation)?;
        if self.definition_revision == 0
            || self.attempt == 0
            || self.task_id == 0
            || self.runtime == Runtime::Auto
        {
            return Err(Error::Invalid("Extension attempt context identity"));
        }
        validate_definition_flags(self.flags)?;
        validate_name(
            &self.name,
            self.flags & crate::schema::extension::DEFINITION_PERSISTENT as u16 != 0,
        )?;
        validate_argv(&self.argv)?;
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for AttemptContext {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.extension_handle);
        put_u64(out, self.generation);
        put_u64(out, self.definition_revision);
        put_u64(out, self.attempt);
        put_u32(out, self.task_id);
        put_u16(out, self.flags);
        out.push(self.runtime as u8);
        out.push(0);
        out.extend_from_slice(&self.content_hash);
        put_string_u16(out, &self.name)?;
        put_len_u16(out, self.argv.len())?;
        for arg in &self.argv {
            put_bytes_u32(out, arg)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for AttemptContext {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let extension_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let definition_revision = decoder.u64()?;
        let attempt = decoder.u64()?;
        let task_id = decoder.u32()?;
        let flags = decoder.u16()?;
        let runtime = Runtime::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Extension attempt-context reserved byte"));
        }
        let content_hash = decoder.array_32()?;
        let name = decoder.string_u16()?;
        let argc = usize::from(decoder.u16()?);
        if argc > MAX_ARGS || argc > decoder.remaining() / 4 {
            return Err(Error::Invalid("Extension attempt argument count"));
        }
        let mut argv = Vec::with_capacity(argc);
        for _ in 0..argc {
            argv.push(decoder.len_bytes_u32()?.to_vec());
        }
        let value = Self {
            extension_handle,
            generation,
            definition_revision,
            attempt,
            task_id,
            flags,
            runtime,
            content_hash,
            name,
            argv,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

/// One output record emitted by the authenticated Extension attempt carried
/// by this session's [`AttemptContext`]. The server assigns the retained
/// sequence and monotonic timestamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptOutput {
    pub kind: OutputKind,
    pub data: Vec<u8>,
    pub extensions: Extensions,
}

impl AttemptOutput {
    fn validate(&self) -> Result<()> {
        if self.kind == OutputKind::Gap {
            return Err(Error::Invalid("Extension attempt output kind"));
        }
        if self.data.len() > MAX_OUTPUT_RECORD_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Extension attempt output bytes",
                actual: self.data.len() as u64,
                maximum: MAX_OUTPUT_RECORD_BYTES as u64,
            });
        }
        if self.kind == OutputKind::Log && core::str::from_utf8(&self.data).is_err() {
            return Err(Error::Invalid("Extension attempt log UTF-8"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for AttemptOutput {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.kind as u8);
        out.extend_from_slice(&[0; 3]);
        put_bytes_u32(out, &self.data)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for AttemptOutput {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = OutputKind::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 || decoder.u8()? != 0 || decoder.u8()? != 0 {
            return Err(Error::Invalid("Extension attempt-output reserved bytes"));
        }
        let value = Self {
            kind,
            data: decoder.len_bytes_u32()?.to_vec(),
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputRecord {
    pub kind: OutputKind,
    pub sequence: u64,
    pub server_ns: u64,
    pub data: Vec<u8>,
}

impl OutputRecord {
    fn validate(&self) -> Result<()> {
        if self.data.len() > MAX_OUTPUT_RECORD_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Extension output record bytes",
                actual: self.data.len() as u64,
                maximum: MAX_OUTPUT_RECORD_BYTES as u64,
            });
        }
        if self.kind == OutputKind::Gap && self.data.len() != 8 {
            return Err(Error::Invalid("Extension output gap record"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputBatch {
    pub first_sequence: u64,
    pub records: Vec<OutputRecord>,
}

impl Encode for OutputBatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.records.is_empty() || self.records.len() > u16::MAX as usize {
            return Err(Error::Invalid("Extension output record count"));
        }
        let start_len = out.len();
        put_u64(out, self.first_sequence);
        put_len_u16(out, self.records.len())?;
        put_u16(out, 0);
        for (index, record) in self.records.iter().enumerate() {
            record.validate()?;
            if record.sequence
                != self
                    .first_sequence
                    .checked_add(index as u64)
                    .ok_or(Error::LengthOverflow)?
            {
                return Err(Error::Invalid("non-consecutive Extension output records"));
            }
            out.push(record.kind as u8);
            out.extend_from_slice(&[0; 3]);
            put_u64(out, record.sequence);
            put_u64(out, record.server_ns);
            put_bytes_u32(out, &record.data)?;
        }
        let encoded_len = out.len() - start_len;
        if encoded_len > MAX_OUTPUT_BATCH_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Extension output batch bytes",
                actual: encoded_len as u64,
                maximum: MAX_OUTPUT_BATCH_BYTES as u64,
            });
        }
        Ok(())
    }
}

impl Decode for OutputBatch {
    fn decode(input: &[u8]) -> Result<Self> {
        if input.len() > MAX_OUTPUT_BATCH_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Extension output batch bytes",
                actual: input.len() as u64,
                maximum: MAX_OUTPUT_BATCH_BYTES as u64,
            });
        }
        let mut decoder = Decoder::new(input);
        let first_sequence = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0 || count == 0 || count > decoder.remaining() / 24 {
            return Err(Error::Invalid("Extension output count or reserved field"));
        }
        let mut records = Vec::with_capacity(count);
        for index in 0..count {
            let kind = OutputKind::try_from(decoder.u8()?)?;
            if decoder.take(3)? != [0; 3] {
                return Err(Error::Invalid("Extension output reserved bytes"));
            }
            let record = OutputRecord {
                kind,
                sequence: decoder.u64()?,
                server_ns: decoder.u64()?,
                data: decoder.len_bytes_u32()?.to_vec(),
            };
            record.validate()?;
            if record.sequence
                != first_sequence
                    .checked_add(index as u64)
                    .ok_or(Error::LengthOverflow)?
            {
                return Err(Error::Invalid("non-consecutive Extension output records"));
            }
            records.push(record);
        }
        decoder.finish()?;
        Ok(Self {
            first_sequence,
            records,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoverCommands {
    /// Zero accepts the current directory revision.
    pub directory_revision: u64,
    /// Zero begins a fresh stable snapshot.
    pub cursor: u64,
    /// Zero selects the server maximum.
    pub max_records: u16,
    pub extensions: Extensions,
}

impl Encode for DiscoverCommands {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.max_records as usize > MAX_COMMAND_RECORDS {
            return Err(Error::Invalid("Extension command page size"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.directory_revision);
        put_u64(out, self.cursor);
        put_u16(out, self.max_records);
        put_u16(out, 0);
        self.extensions.encode_tail(out)
    }
}

impl Decode for DiscoverCommands {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            directory_revision: decoder.u64()?,
            cursor: decoder.u64()?,
            max_records: decoder.u16()?,
            extensions: {
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Extension discovery reserved field"));
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

/// Publish the current Extension attempt's command descriptor through one
/// listener owned by the same native YAS session.  An all-zero/empty value
/// removes the attempt's current publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterCommand {
    pub listener_handle: u64,
    pub listener_generation: u64,
    pub descriptor: String,
    pub extensions: Extensions,
}

impl RegisterCommand {
    pub fn is_unregister(&self) -> bool {
        self.listener_handle == 0 && self.listener_generation == 0 && self.descriptor.is_empty()
    }

    fn validate(&self) -> Result<()> {
        let unregister = self.is_unregister();
        if !unregister
            && (self.listener_handle == 0
                || self.listener_generation == 0
                || self.descriptor.is_empty())
            || self.descriptor.len() > MAX_COMMAND_DESCRIPTOR_BYTES
        {
            return Err(Error::Invalid("Extension command registration"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for RegisterCommand {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.listener_handle);
        put_u64(out, self.listener_generation);
        put_string_u32(out, &self.descriptor)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for RegisterCommand {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            listener_handle: decoder.u64()?,
            listener_generation: decoder.u64()?,
            descriptor: decoder.string_u32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisterCommandResult {
    pub extension_handle: u64,
    pub generation: u64,
    pub definition_revision: u64,
    pub directory_revision: u64,
    pub changed: bool,
    pub extensions: Extensions,
}

impl RegisterCommandResult {
    fn validate(&self) -> Result<()> {
        validate_identity(self.extension_handle, self.generation)?;
        if self.definition_revision == 0 || self.directory_revision == 0 {
            return Err(Error::Invalid("Extension command registration result"));
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for RegisterCommandResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.extension_handle);
        put_u64(out, self.generation);
        put_u64(out, self.definition_revision);
        put_u64(out, self.directory_revision);
        out.push(u8::from(self.changed));
        out.extend_from_slice(&[0; 7]);
        self.extensions.encode_tail(out)
    }
}

impl Decode for RegisterCommandResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let extension_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let definition_revision = decoder.u64()?;
        let directory_revision = decoder.u64()?;
        let changed = match decoder.u8()? {
            0 => false,
            1 => true,
            _ => {
                return Err(Error::Invalid(
                    "Extension command registration changed flag",
                ));
            }
        };
        if decoder.take(7)? != [0; 7] {
            return Err(Error::Invalid(
                "Extension command registration reserved bytes",
            ));
        }
        let value = Self {
            extension_handle,
            generation,
            definition_revision,
            directory_revision,
            changed,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRecord {
    pub extension_handle: u64,
    pub generation: u64,
    pub definition_revision: u64,
    pub content_hash: [u8; 32],
    pub listener_handle: u64,
    pub listener_generation: u64,
    pub name: String,
    pub listener_name: String,
    pub descriptor: String,
    pub extensions: Extensions,
}

impl CommandRecord {
    fn validate(&self) -> Result<()> {
        validate_identity(self.extension_handle, self.generation)?;
        validate_identity(self.listener_handle, self.listener_generation)?;
        if self.definition_revision == 0 {
            return Err(Error::Invalid("zero Extension command definition revision"));
        }
        validate_name(&self.name, true)?;
        validate_name(&self.listener_name, true)?;
        if self.descriptor.is_empty() || self.descriptor.len() > MAX_COMMAND_DESCRIPTOR_BYTES {
            return Err(Error::Invalid("Extension command descriptor"));
        }
        reject_unknown_required(&self.extensions, &[])
    }

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.extension_handle);
        put_u64(out, self.generation);
        put_u64(out, self.definition_revision);
        out.extend_from_slice(&self.content_hash);
        put_u64(out, self.listener_handle);
        put_u64(out, self.listener_generation);
        put_string_u16(out, &self.name)?;
        put_string_u16(out, &self.listener_name)?;
        put_string_u32(out, &self.descriptor)?;
        self.extensions.encode_tail(out)
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            extension_handle: decoder.u64()?,
            generation: decoder.u64()?,
            definition_revision: decoder.u64()?,
            content_hash: decoder.array_32()?,
            listener_handle: decoder.u64()?,
            listener_generation: decoder.u64()?,
            name: decoder.string_u16()?,
            listener_name: decoder.string_u16()?,
            descriptor: decoder.string_u32()?,
            extensions: decoder.extensions()?,
        };
        value.validate()?;
        Ok(value)
    }
}

impl Encode for CommandRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.encode_body(out)
    }
}

impl Decode for CommandRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self::decode_from(&mut decoder)?;
        decoder.finish()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPage {
    pub directory_revision: u64,
    /// Zero marks the final page.
    pub next_cursor: u64,
    pub records: Vec<CommandRecord>,
}

impl Encode for CommandPage {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.directory_revision == 0 || self.records.len() > MAX_COMMAND_RECORDS {
            return Err(Error::Invalid("Extension command page"));
        }
        let start_len = out.len();
        put_u64(out, self.directory_revision);
        put_u64(out, self.next_cursor);
        put_len_u16(out, self.records.len())?;
        put_u16(out, 0);
        for record in &self.records {
            record.encode_body(out)?;
        }
        let encoded_len = out.len() - start_len;
        if encoded_len > MAX_COMMAND_PAGE_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Extension command page bytes",
                actual: encoded_len as u64,
                maximum: MAX_COMMAND_PAGE_BYTES as u64,
            });
        }
        Ok(())
    }
}

impl Decode for CommandPage {
    fn decode(input: &[u8]) -> Result<Self> {
        if input.len() > MAX_COMMAND_PAGE_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Extension command page bytes",
                actual: input.len() as u64,
                maximum: MAX_COMMAND_PAGE_BYTES as u64,
            });
        }
        let mut decoder = Decoder::new(input);
        let directory_revision = decoder.u64()?;
        let next_cursor = decoder.u64()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0 || count > MAX_COMMAND_RECORDS || count > decoder.remaining() / 84 {
            return Err(Error::Invalid("Extension command count or reserved field"));
        }
        let mut records = Vec::with_capacity(count);
        for _ in 0..count {
            records.push(CommandRecord::decode_from(&mut decoder)?);
        }
        decoder.finish()?;
        let value = Self {
            directory_revision,
            next_cursor,
            records,
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

fn validate_object_descriptor(descriptor: &Descriptor) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Byte
        || descriptor.direction != Direction::RECEIVER_TO_SENDER
        || descriptor.sender_send_credit != 0
        || descriptor.receiver_send_credit == 0
        || descriptor.max_item_bytes != 0
        || descriptor.content_family != crate::family::EXTENSION
        || descriptor.content_kind != crate::schema::extension::OBJECT_CONTENT_KIND as u16
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("Extension object Transfer descriptor"));
    }
    Ok(())
}

fn validate_follow_descriptor(descriptor: &Descriptor) -> Result<()> {
    descriptor.validate()?;
    if descriptor.mode != Mode::Message
        || descriptor.direction != Direction::SENDER_TO_RECEIVER
        || descriptor.receiver_send_credit != 0
        || descriptor.max_item_bytes == 0
        || descriptor.max_item_bytes > MAX_OUTPUT_BATCH_BYTES as u64
        || descriptor.content_family != crate::family::EXTENSION
        || descriptor.content_kind != crate::schema::extension::FOLLOW_CONTENT_KIND as u16
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
    {
        return Err(Error::Invalid("Extension follow Transfer descriptor"));
    }
    Ok(())
}

fn validate_definition_flags(flags: u16) -> Result<()> {
    let known = crate::schema::extension::DEFINITION_FLAGS as u16;
    if flags & !known != 0
        || flags & crate::schema::extension::DEFINITION_DESIRED_RUNNING as u16 != 0
            && flags & crate::schema::extension::DEFINITION_ENABLED as u16 == 0
        || flags & crate::schema::extension::DEFINITION_PERSISTENT as u16 != 0
            && flags & crate::schema::extension::DEFINITION_DETACHED as u16 == 0
    {
        return Err(Error::Invalid("Extension definition flags"));
    }
    Ok(())
}

fn validate_name(name: &str, required: bool) -> Result<()> {
    if (required && name.is_empty()) || name.len() > MAX_NAME_BYTES || name.as_bytes().contains(&0)
    {
        return Err(Error::Invalid("Extension name"));
    }
    Ok(())
}

fn validate_argv(argv: &[Vec<u8>]) -> Result<()> {
    if argv.len() > MAX_ARGS {
        return Err(Error::Invalid("Extension argument count"));
    }
    let mut total = 0usize;
    for arg in argv {
        if arg.len() > MAX_ARG_BYTES {
            return Err(Error::Invalid("Extension argument bytes"));
        }
        total = total.checked_add(arg.len()).ok_or(Error::LengthOverflow)?;
        if total > MAX_ARGUMENT_BYTES {
            return Err(Error::LimitExceeded {
                limit: "Extension argument bytes",
                actual: total as u64,
                maximum: MAX_ARGUMENT_BYTES as u64,
            });
        }
    }
    Ok(())
}

fn validate_object_len(byte_len: u64) -> Result<()> {
    if byte_len == 0 || byte_len > MAX_OBJECT_BYTES {
        Err(Error::Invalid("Extension object length"))
    } else {
        Ok(())
    }
}

fn validate_identity(handle: u64, generation: u64) -> Result<()> {
    validate_handle(handle, "Extension handle")?;
    if generation == 0 {
        return Err(Error::Invalid("zero Extension generation"));
    }
    Ok(())
}

fn validate_handle(handle: u64, what: &'static str) -> Result<()> {
    if handle == 0 {
        Err(Error::Invalid(what))
    } else {
        Ok(())
    }
}

fn validate_operation_id(value: &[u8; 16]) -> Result<()> {
    if *value == [0; 16] {
        Err(Error::Invalid("zero Extension operation ID"))
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
        return Err(Error::Invalid("unknown required Extension extension"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Extension;

    fn sensitive_extensions() -> Extensions {
        Extensions(vec![Extension {
            tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
            required: true,
            value: Vec::new(),
        }])
    }

    fn upload_extensions(staging_handle: u64) -> Extensions {
        let mut extensions = sensitive_extensions();
        extensions.0.push(
            UploadStage {
                staging_handle,
                expires_server_ns: 1,
            }
            .extension()
            .unwrap(),
        );
        extensions
    }

    fn object_descriptor() -> Descriptor {
        Descriptor {
            transfer_id: 1,
            mode: Mode::Byte,
            direction: Direction::RECEIVER_TO_SENDER,
            receiver_send_credit: 65_536,
            sender_send_credit: 0,
            max_item_bytes: 0,
            max_chunk_bytes: 65_536,
            content_family: crate::family::EXTENSION,
            content_kind: crate::schema::extension::OBJECT_CONTENT_KIND as u16,
            content_version: VERSION,
            extensions: upload_extensions(4),
        }
    }

    fn follow_descriptor() -> Descriptor {
        Descriptor {
            transfer_id: 2,
            mode: Mode::Message,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: 65_536,
            max_item_bytes: MAX_OUTPUT_BATCH_BYTES as u64,
            max_chunk_bytes: 65_536,
            content_family: crate::family::EXTENSION,
            content_kind: crate::schema::extension::FOLLOW_CONTENT_KIND as u16,
            content_version: VERSION,
            extensions: sensitive_extensions(),
        }
    }

    fn limits() -> RuntimeLimits {
        RuntimeLimits {
            memory_bytes: 64 << 20,
            stack_bytes: 1 << 20,
            max_active_jobs: 8,
            max_pending_jobs: 8,
            max_job_bytes: 16 << 20,
            slow_consumer_timeout_ns: 10_000_000_000,
            extensions: Extensions::default(),
        }
    }

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

    #[test]
    fn object_and_definition_requests_round_trip() {
        round_trip(ObjectBegin {
            operation_id: [1; 16],
            content_hash: [2; 32],
            byte_len: 4096,
            extensions: Extensions::default(),
        });
        let stage = ObjectBeginResult {
            disposition: ObjectDisposition::Upload,
            staging_handle: 4,
            descriptor: Some(object_descriptor()),
            extensions: Extensions::default(),
        };
        round_trip(stage.clone());
        let reset = Reset {
            transfer_id: 1,
            status: crate::schema::core::status::CANCELLED,
            detail: Vec::new(),
        };
        assert_eq!(
            stage.stage_discarded_by(&reset).unwrap(),
            stage.descriptor.as_ref().unwrap().upload_stage().unwrap()
        );
        round_trip(Deploy {
            operation_id: [3; 16],
            expected_extension_handle: 0,
            expected_generation: 0,
            expected_definition_revision: 0,
            flags: (crate::schema::extension::DEFINITION_PERSISTENT
                | crate::schema::extension::DEFINITION_ENABLED
                | crate::schema::extension::DEFINITION_DESIRED_RUNNING
                | crate::schema::extension::DEFINITION_DETACHED) as u16,
            runtime: Runtime::Wasmi,
            restart_policy: RestartPolicy::Always,
            name: "builder".into(),
            content_hash: [2; 32],
            argv: vec![b"builder".to_vec(), vec![0xff, 0]],
            runtime_limits: limits(),
            extensions: Extensions::default(),
        });
        let invalid = Deploy {
            operation_id: [3; 16],
            expected_extension_handle: 5,
            expected_generation: 0,
            expected_definition_revision: 2,
            flags: crate::schema::extension::DEFINITION_DETACHED as u16,
            runtime: Runtime::Wasmi,
            restart_policy: RestartPolicy::Never,
            name: String::new(),
            content_hash: [2; 32],
            argv: Vec::new(),
            runtime_limits: limits(),
            extensions: Extensions::default(),
        };
        assert_eq!(
            invalid.encode(),
            Err(Error::Invalid("partial Extension deploy identity"))
        );
    }

    #[test]
    fn state_follow_and_output_round_trip() {
        let record = ExtensionRecord {
            extension_handle: 5,
            generation: 6,
            definition_revision: 2,
            phase: Phase::Running,
            runtime: Runtime::QuickJs,
            restart_policy: RestartPolicy::OnFailure,
            flags: (crate::schema::extension::DEFINITION_PERSISTENT
                | crate::schema::extension::DEFINITION_ENABLED
                | crate::schema::extension::DEFINITION_DESIRED_RUNNING
                | crate::schema::extension::DEFINITION_DETACHED) as u16,
            attempt: 3,
            last_running_attempt: 3,
            task_id: 9,
            next_start_unix_ms: 0,
            directory_revision: 7,
            content_hash: [8; 32],
            name: "worker".into(),
            last_exit: None,
            runtime_limits: limits(),
            extensions: Extensions::default(),
        };
        round_trip(record.clone());
        assert_eq!(
            ExtensionRecord::from_state_record(&record.state_record(RecordKind::Add).unwrap())
                .unwrap(),
            record
        );
        let mut backoff = record.clone();
        backoff.phase = Phase::Backoff;
        backoff.next_start_unix_ms = 1_700_000_000_000;
        round_trip(backoff.clone());
        backoff.next_start_unix_ms = MAX_NEXT_START_UNIX_MS + 1;
        assert_eq!(
            backoff.encode(),
            Err(Error::Invalid("Extension backoff deadline"))
        );
        let follow = FollowResult {
            attempt: 3,
            first_sequence: 10,
            through_sequence: 11,
            descriptor: follow_descriptor(),
            extensions: Extensions::default(),
        };
        round_trip(follow.clone());
        assert!(follow.transfer_termination_detaches(2).unwrap());
        assert!(!follow.transfer_termination_detaches(3).unwrap());
        round_trip(AttemptOutput {
            kind: OutputKind::Stdout,
            data: b"hello".to_vec(),
            extensions: Extensions::default(),
        });
        assert_eq!(
            AttemptOutput {
                kind: OutputKind::Gap,
                data: Vec::new(),
                extensions: Extensions::default(),
            }
            .encode(),
            Err(Error::Invalid("Extension attempt output kind"))
        );
        assert_eq!(
            AttemptOutput {
                kind: OutputKind::Log,
                data: vec![0xff],
                extensions: Extensions::default(),
            }
            .encode(),
            Err(Error::Invalid("Extension attempt log UTF-8"))
        );
        round_trip(OutputBatch {
            first_sequence: 10,
            records: vec![
                OutputRecord {
                    kind: OutputKind::Stdout,
                    sequence: 10,
                    server_ns: 100,
                    data: b"hello".to_vec(),
                },
                OutputRecord {
                    kind: OutputKind::Gap,
                    sequence: 11,
                    server_ns: 101,
                    data: 3u64.to_le_bytes().to_vec(),
                },
            ],
        });
    }

    #[test]
    fn command_page_round_trip() {
        round_trip(RegisterCommand {
            listener_handle: 8,
            listener_generation: 9,
            descriptor: r#"{"protocol":"yas.command.v1","summary":"ship","commands":[]}"#.into(),
            extensions: Extensions::default(),
        });
        round_trip(RegisterCommand {
            listener_handle: 0,
            listener_generation: 0,
            descriptor: String::new(),
            extensions: Extensions::default(),
        });
        round_trip(RegisterCommandResult {
            extension_handle: 5,
            generation: 6,
            definition_revision: 2,
            directory_revision: 4,
            changed: true,
            extensions: Extensions::default(),
        });
        round_trip(CommandPage {
            directory_revision: 4,
            next_cursor: 0,
            records: vec![CommandRecord {
                extension_handle: 5,
                generation: 6,
                definition_revision: 2,
                content_hash: [7; 32],
                listener_handle: 8,
                listener_generation: 9,
                name: "ship".into(),
                listener_name: "extension/ship".into(),
                descriptor: r#"{"protocol":"yas.command.v1","summary":"ship","commands":[]}"#
                    .into(),
                extensions: Extensions::default(),
            }],
        });
    }

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);
        let mut invalid = Limits::HARD;
        invalid.max_memory_bytes = 0;
        assert!(invalid.to_extensions().is_err());
        assert!(Limits::from_extensions(&Extensions::default()).is_err());
    }
}
