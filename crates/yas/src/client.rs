//! YAS Client family version 1 payload codecs.

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, limit_u32, put_bytes_u32,
    put_len_u16, put_len_u32, put_string_u16, put_string_u32, put_u16, put_u32, put_u64,
    read_limit_u32, reject_unknown_required_extensions,
};
use crate::prelude::*;
use crate::state::{Record, RecordKind};

pub const VERSION: u16 = crate::schema::client::VERSION;

pub mod request_kind {
    pub use crate::schema::client::request::*;
}

pub mod event_kind {
    pub use crate::schema::client::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_published_clients: u32,
    pub max_active_subscriptions: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_published_clients: crate::schema::client::MAX_PUBLISHED_CLIENTS as u32,
        max_active_subscriptions: crate::schema::client::MAX_ACTIVE_SUBSCRIPTIONS as u32,
    };

    pub fn validate(self) -> Result<()> {
        if self.max_published_clients == 0
            || self.max_published_clients > Self::HARD.max_published_clients
            || self.max_active_subscriptions == 0
            || self.max_active_subscriptions > Self::HARD.max_active_subscriptions
        {
            return Err(Error::Invalid("Client family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::client::LIMIT_MAX_PUBLISHED_CLIENTS,
                self.max_published_clients,
            ),
            limit_u32(
                crate::schema::client::LIMIT_MAX_ACTIVE_SUBSCRIPTIONS,
                self.max_active_subscriptions,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required_extensions(
            extensions,
            &[
                crate::schema::client::LIMIT_MAX_PUBLISHED_CLIENTS as u16,
                crate::schema::client::LIMIT_MAX_ACTIVE_SUBSCRIPTIONS as u16,
            ],
            "unknown required Client family limit",
        )?;
        let value = Self {
            max_published_clients: read_limit_u32(
                extensions,
                crate::schema::client::LIMIT_MAX_PUBLISHED_CLIENTS,
            )?,
            max_active_subscriptions: read_limit_u32(
                extensions,
                crate::schema::client::LIMIT_MAX_ACTIVE_SUBSCRIPTIONS,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn nonzero_id(id: &[u8; 16], what: &'static str) -> Result<()> {
    if id.iter().all(|byte| *byte == 0) {
        Err(Error::Invalid(what))
    } else {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    Unix {
        peer_pid: u32,
        peer_uid: u32,
        peer_gid: u32,
        socket_path: Vec<u8>,
    },
    Ssh {
        remote_address: String,
        username: String,
    },
    Edge {
        subject: String,
        issuer: String,
    },
    Relay {
        route_handle: u64,
        generation: u64,
        depth: u16,
    },
    WebRtc {
        peer_id: String,
    },
    Extension {
        extension_id: u64,
        definition_revision: u64,
        attempt: u64,
        task_id: u32,
        name: String,
    },
    UnknownOptional {
        kind: u16,
        body: Vec<u8>,
    },
}

impl Origin {
    fn kind(&self) -> u16 {
        match self {
            Self::Unix { .. } => crate::schema::client::ORIGIN_UNIX as u16,
            Self::Ssh { .. } => crate::schema::client::ORIGIN_SSH as u16,
            Self::Edge { .. } => crate::schema::client::ORIGIN_EDGE as u16,
            Self::Relay { .. } => crate::schema::client::ORIGIN_RELAY as u16,
            Self::WebRtc { .. } => crate::schema::client::ORIGIN_WEBRTC as u16,
            Self::Extension { .. } => crate::schema::client::ORIGIN_EXTENSION as u16,
            Self::UnknownOptional { kind, .. } => *kind,
        }
    }

    fn encode_body(&self, out: &mut Vec<u8>) -> Result<()> {
        match self {
            Self::Unix {
                peer_pid,
                peer_uid,
                peer_gid,
                socket_path,
            } => {
                put_u32(out, *peer_pid);
                put_u32(out, *peer_uid);
                put_u32(out, *peer_gid);
                put_bytes_u32(out, socket_path)?;
            }
            Self::Ssh {
                remote_address,
                username,
            } => {
                put_string_u16(out, remote_address)?;
                put_string_u16(out, username)?;
            }
            Self::Edge { subject, issuer } => {
                put_string_u16(out, subject)?;
                put_string_u16(out, issuer)?;
            }
            Self::Relay {
                route_handle,
                generation,
                depth,
            } => {
                if *route_handle == 0 || *depth == 0 {
                    return Err(Error::Invalid("Client Relay origin"));
                }
                put_u64(out, *route_handle);
                put_u64(out, *generation);
                put_u16(out, *depth);
                put_u16(out, 0);
            }
            Self::WebRtc { peer_id } => put_string_u16(out, peer_id)?,
            Self::Extension {
                extension_id,
                definition_revision,
                attempt,
                task_id,
                name,
            } => {
                if *extension_id == 0 {
                    return Err(Error::Invalid("Client Extension origin"));
                }
                put_u64(out, *extension_id);
                put_u64(out, *definition_revision);
                put_u64(out, *attempt);
                put_u32(out, *task_id);
                put_string_u16(out, name)?;
            }
            Self::UnknownOptional { body, .. } => out.extend_from_slice(body),
        }
        Ok(())
    }

    fn decode_body(kind: u16, required: bool, body: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(body);
        let value = match kind {
            value if value == crate::schema::client::ORIGIN_UNIX as u16 => Self::Unix {
                peer_pid: decoder.u32()?,
                peer_uid: decoder.u32()?,
                peer_gid: decoder.u32()?,
                socket_path: decoder.len_bytes_u32()?.to_vec(),
            },
            value if value == crate::schema::client::ORIGIN_SSH as u16 => Self::Ssh {
                remote_address: decoder.string_u16()?,
                username: decoder.string_u16()?,
            },
            value if value == crate::schema::client::ORIGIN_EDGE as u16 => Self::Edge {
                subject: decoder.string_u16()?,
                issuer: decoder.string_u16()?,
            },
            value if value == crate::schema::client::ORIGIN_RELAY as u16 => {
                let route_handle = decoder.u64()?;
                let generation = decoder.u64()?;
                let depth = decoder.u16()?;
                if decoder.u16()? != 0 || route_handle == 0 || depth == 0 {
                    return Err(Error::Invalid("Client Relay origin"));
                }
                Self::Relay {
                    route_handle,
                    generation,
                    depth,
                }
            }
            value if value == crate::schema::client::ORIGIN_WEBRTC as u16 => Self::WebRtc {
                peer_id: decoder.string_u16()?,
            },
            value if value == crate::schema::client::ORIGIN_EXTENSION as u16 => {
                let extension_id = decoder.u64()?;
                if extension_id == 0 {
                    return Err(Error::Invalid("Client Extension origin"));
                }
                Self::Extension {
                    extension_id,
                    definition_revision: decoder.u64()?,
                    attempt: decoder.u64()?,
                    task_id: decoder.u32()?,
                    name: decoder.string_u16()?,
                }
            }
            _ if required => return Err(Error::Invalid("unknown required Client origin")),
            _ => {
                return Ok(Self::UnknownOptional {
                    kind,
                    body: body.to_vec(),
                });
            }
        };
        decoder.finish()?;
        Ok(value)
    }
}

impl Encode for Origin {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        let mut body = Vec::new();
        self.encode_body(&mut body)?;
        let len = body.len().checked_add(4).ok_or(Error::LengthOverflow)?;
        put_len_u32(out, len)?;
        put_u16(out, self.kind());
        put_u16(out, 0);
        out.extend_from_slice(&body);
        Ok(())
    }
}

impl Decode for Origin {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let record = decoder.len_bytes_u32()?;
        decoder.finish()?;
        let mut record = Decoder::new(record);
        let kind = record.u16()?;
        let flags = record.u16()?;
        if flags & !1 != 0 {
            return Err(Error::Invalid("Client origin flags"));
        }
        let body = record.rest();
        record.finish()?;
        Self::decode_body(kind, flags & 1 != 0, body)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientRecord {
    pub session_id: [u8; 16],
    pub client_instance: [u8; 16],
    pub connected_server_ns: u64,
    pub idle_ns: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub name: String,
    pub release: String,
    pub label: String,
    pub origin: Origin,
    pub extensions: Extensions,
}

impl ClientRecord {
    fn validate(&self) -> Result<()> {
        nonzero_id(&self.session_id, "zero Client session ID")?;
        nonzero_id(&self.client_instance, "zero Client instance ID")?;
        if self.name.is_empty() {
            return Err(Error::Invalid("empty Client name"));
        }
        validate_record_extensions(&self.extensions)
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Client complete state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }

    /// Decode the optional active-subscription accounting snapshot.
    pub fn active_subscriptions(&self) -> Result<Option<ActiveSubscriptions>> {
        decode_active_subscriptions(&self.extensions)
    }

    /// Decode the optional current sampled traffic rates.
    pub fn bandwidth_rates(&self) -> Result<Option<BandwidthRates>> {
        decode_bandwidth_rates(&self.extensions)
    }
}

impl Encode for ClientRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.extend_from_slice(&self.session_id);
        out.extend_from_slice(&self.client_instance);
        put_u64(out, self.connected_server_ns);
        put_u64(out, self.idle_ns);
        put_u64(out, self.bytes_received);
        put_u64(out, self.bytes_sent);
        put_string_u16(out, &self.name)?;
        put_string_u16(out, &self.release)?;
        put_string_u16(out, &self.label)?;
        self.origin.encode_to(out)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for ClientRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            session_id: decoder.array_16()?,
            client_instance: decoder.array_16()?,
            connected_server_ns: decoder.u64()?,
            idle_ns: decoder.u64()?,
            bytes_received: decoder.u64()?,
            bytes_sent: decoder.u64()?,
            name: decoder.string_u16()?,
            release: decoder.string_u16()?,
            label: decoder.string_u16()?,
            origin: {
                let bytes = decoder.len_bytes_u32()?;
                let mut encoded = Vec::with_capacity(bytes.len() + 4);
                put_len_u32(&mut encoded, bytes.len())?;
                encoded.extend_from_slice(bytes);
                Origin::decode(&encoded)?
            },
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientPatch {
    pub session_id: [u8; 16],
    pub extensions: Extensions,
}

impl ClientPatch {
    pub fn state_record(&self) -> Result<Record> {
        Ok(Record {
            kind: RecordKind::Patch,
            required: false,
            body: self.encode()?,
        })
    }

    /// Decode an updated active-subscription accounting snapshot, when this
    /// patch carries one.
    pub fn active_subscriptions(&self) -> Result<Option<ActiveSubscriptions>> {
        decode_active_subscriptions(&self.extensions)
    }

    /// Decode updated current sampled traffic rates, when present.
    pub fn bandwidth_rates(&self) -> Result<Option<BandwidthRates>> {
        decode_bandwidth_rates(&self.extensions)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BandwidthRates {
    pub received_bytes_per_second: u64,
    pub sent_bytes_per_second: u64,
    pub sample_window_ns: u64,
}

impl BandwidthRates {
    fn validate(self) -> Result<()> {
        if self.sample_window_ns == 0 {
            Err(Error::Invalid("zero Client bandwidth sample window"))
        } else {
            Ok(())
        }
    }

    pub fn extension(self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::client::BANDWIDTH_RATES_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for BandwidthRates {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.received_bytes_per_second);
        put_u64(out, self.sent_bytes_per_second);
        put_u64(out, self.sample_window_ns);
        Ok(())
    }
}

impl Decode for BandwidthRates {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            received_bytes_per_second: decoder.u64()?,
            sent_bytes_per_second: decoder.u64()?,
            sample_window_ns: decoder.u64()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSubscription {
    pub terminal_handle: u64,
    pub view_id: u32,
    pub rows: u16,
    pub columns: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SurfaceSubscription {
    pub surface_handle: u64,
    pub view_id: u32,
    pub width: u32,
    pub height: u32,
    pub scale_120: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuxiliarySubscription {
    pub family: u16,
    pub subscription_id: u32,
    pub resource_handle: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActiveSubscriptions {
    pub terminals: Vec<TerminalSubscription>,
    pub surfaces: Vec<SurfaceSubscription>,
    pub auxiliary: Vec<AuxiliarySubscription>,
}

impl ActiveSubscriptions {
    fn validate(&self) -> Result<()> {
        let total = self
            .terminals
            .len()
            .checked_add(self.surfaces.len())
            .and_then(|value| value.checked_add(self.auxiliary.len()))
            .ok_or(Error::LengthOverflow)?;
        if total > crate::schema::client::MAX_ACTIVE_SUBSCRIPTIONS as usize {
            return Err(Error::LimitExceeded {
                limit: "Client active subscriptions",
                actual: total as u64,
                maximum: crate::schema::client::MAX_ACTIVE_SUBSCRIPTIONS,
            });
        }
        let mut previous = None;
        for value in &self.terminals {
            if value.terminal_handle == 0
                || value.view_id == 0
                || (value.rows == 0) != (value.columns == 0)
                || previous.is_some_and(|key| key >= (value.terminal_handle, value.view_id))
            {
                return Err(Error::Invalid("Client terminal subscription"));
            }
            previous = Some((value.terminal_handle, value.view_id));
        }
        let mut previous = None;
        for value in &self.surfaces {
            let dimensions_absent = value.width == 0 && value.height == 0 && value.scale_120 == 0;
            let dimensions_present = value.width != 0 && value.height != 0 && value.scale_120 != 0;
            if value.surface_handle == 0
                || value.view_id == 0
                || (!dimensions_absent && !dimensions_present)
                || previous.is_some_and(|key| key >= (value.surface_handle, value.view_id))
            {
                return Err(Error::Invalid("Client surface subscription"));
            }
            previous = Some((value.surface_handle, value.view_id));
        }
        let mut previous = None;
        for value in &self.auxiliary {
            if value.subscription_id == 0
                || previous.is_some_and(|key| {
                    key >= (value.family, value.subscription_id, value.resource_handle)
                })
            {
                return Err(Error::Invalid("Client auxiliary subscription"));
            }
            previous = Some((value.family, value.subscription_id, value.resource_handle));
        }
        Ok(())
    }

    /// Wrap this typed snapshot in the canonical Client record extension.
    pub fn extension(&self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::client::ACTIVE_SUBSCRIPTIONS_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for ActiveSubscriptions {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_len_u16(out, self.terminals.len())?;
        put_len_u16(out, self.surfaces.len())?;
        put_len_u16(out, self.auxiliary.len())?;
        put_u16(out, 0);
        for value in &self.terminals {
            put_u64(out, value.terminal_handle);
            put_u32(out, value.view_id);
            put_u16(out, value.rows);
            put_u16(out, value.columns);
        }
        for value in &self.surfaces {
            put_u64(out, value.surface_handle);
            put_u32(out, value.view_id);
            put_u32(out, value.width);
            put_u32(out, value.height);
            put_u16(out, value.scale_120);
            put_u16(out, 0);
        }
        for value in &self.auxiliary {
            put_u16(out, value.family);
            put_u16(out, 0);
            put_u32(out, value.subscription_id);
            put_u64(out, value.resource_handle);
        }
        Ok(())
    }
}

impl Decode for ActiveSubscriptions {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let terminal_count = usize::from(decoder.u16()?);
        let surface_count = usize::from(decoder.u16()?);
        let auxiliary_count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Client subscription reserved field"));
        }
        let total = terminal_count
            .checked_add(surface_count)
            .and_then(|value| value.checked_add(auxiliary_count))
            .ok_or(Error::LengthOverflow)?;
        if total > crate::schema::client::MAX_ACTIVE_SUBSCRIPTIONS as usize
            || terminal_count > decoder.remaining() / 16
        {
            return Err(Error::Invalid("Client active subscription count"));
        }
        let mut terminals = Vec::with_capacity(terminal_count);
        for _ in 0..terminal_count {
            terminals.push(TerminalSubscription {
                terminal_handle: decoder.u64()?,
                view_id: decoder.u32()?,
                rows: decoder.u16()?,
                columns: decoder.u16()?,
            });
        }
        if surface_count > decoder.remaining() / 24 {
            return Err(Error::Invalid("Client surface subscription count"));
        }
        let mut surfaces = Vec::with_capacity(surface_count);
        for _ in 0..surface_count {
            let surface_handle = decoder.u64()?;
            let view_id = decoder.u32()?;
            let width = decoder.u32()?;
            let height = decoder.u32()?;
            let scale_120 = decoder.u16()?;
            if decoder.u16()? != 0 {
                return Err(Error::Invalid("Client surface subscription reserved field"));
            }
            surfaces.push(SurfaceSubscription {
                surface_handle,
                view_id,
                width,
                height,
                scale_120,
            });
        }
        if auxiliary_count > decoder.remaining() / 16 {
            return Err(Error::Invalid("Client auxiliary subscription count"));
        }
        let mut auxiliary = Vec::with_capacity(auxiliary_count);
        for _ in 0..auxiliary_count {
            let family = decoder.u16()?;
            if decoder.u16()? != 0 {
                return Err(Error::Invalid(
                    "Client auxiliary subscription reserved field",
                ));
            }
            auxiliary.push(AuxiliarySubscription {
                family,
                subscription_id: decoder.u32()?,
                resource_handle: decoder.u64()?,
            });
        }
        decoder.finish()?;
        let value = Self {
            terminals,
            surfaces,
            auxiliary,
        };
        value.validate()?;
        Ok(value)
    }
}

fn decode_active_subscriptions(extensions: &Extensions) -> Result<Option<ActiveSubscriptions>> {
    extensions.validate()?;
    extensions
        .0
        .iter()
        .find(|extension| {
            extension.tag == crate::schema::client::ACTIVE_SUBSCRIPTIONS_EXTENSION as u16
        })
        .map(|extension| ActiveSubscriptions::decode(&extension.value))
        .transpose()
}

fn decode_bandwidth_rates(extensions: &Extensions) -> Result<Option<BandwidthRates>> {
    extensions.validate()?;
    extensions
        .0
        .iter()
        .find(|extension| extension.tag == crate::schema::client::BANDWIDTH_RATES_EXTENSION as u16)
        .map(|extension| BandwidthRates::decode(&extension.value))
        .transpose()
}

fn validate_record_extensions(extensions: &Extensions) -> Result<()> {
    extensions.validate()?;
    for extension in &extensions.0 {
        match extension.tag {
            tag if tag == crate::schema::client::ACTIVE_SUBSCRIPTIONS_EXTENSION as u16 => {
                ActiveSubscriptions::decode(&extension.value)?;
            }
            tag if tag == crate::schema::client::BANDWIDTH_RATES_EXTENSION as u16 => {
                BandwidthRates::decode(&extension.value)?;
            }
            _ if extension.required => {
                return Err(Error::Invalid("unknown required Client record extension"));
            }
            _ => {}
        }
    }
    Ok(())
}

impl Encode for ClientPatch {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_id(&self.session_id, "zero Client session ID")?;
        validate_record_extensions(&self.extensions)?;
        out.extend_from_slice(&self.session_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for ClientPatch {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            session_id: decoder.array_16()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        nonzero_id(&value.session_id, "zero Client session ID")?;
        validate_record_extensions(&value.extensions)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovedClient {
    pub session_id: [u8; 16],
}

impl RemovedClient {
    pub fn state_record(self) -> Result<Record> {
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for RemovedClient {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_id(&self.session_id, "zero Client session ID")?;
        out.extend_from_slice(&self.session_id);
        Ok(())
    }
}

impl Decode for RemovedClient {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            session_id: decoder.array_16()?,
        };
        decoder.finish()?;
        nonzero_id(&value.session_id, "zero Client session ID")?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disconnect {
    pub session_id: [u8; 16],
    pub operation_id: [u8; 16],
    pub reason: String,
}

impl Encode for Disconnect {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        nonzero_id(&self.session_id, "zero Client session ID")?;
        out.extend_from_slice(&self.session_id);
        out.extend_from_slice(&self.operation_id);
        put_string_u32(out, &self.reason)
    }
}

impl Decode for Disconnect {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            session_id: decoder.array_16()?,
            operation_id: decoder.array_16()?,
            reason: decoder.string_u32()?,
        };
        decoder.finish()?;
        nonzero_id(&value.session_id, "zero Client session ID")?;
        Ok(value)
    }
}

pub fn client_from_state_record(record: &Record) -> Result<ClientRecord> {
    if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
        return Err(Error::Invalid("Client complete state record kind"));
    }
    ClientRecord::decode(&record.body)
}

pub fn patch_from_state_record(record: &Record) -> Result<ClientPatch> {
    if record.kind != RecordKind::Patch {
        return Err(Error::Invalid("Client patch state record kind"));
    }
    ClientPatch::decode(&record.body)
}

pub fn removal_from_state_record(record: &Record) -> Result<RemovedClient> {
    if record.kind != RecordKind::Remove {
        return Err(Error::Invalid("Client removal state record kind"));
    }
    RemovedClient::decode(&record.body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_limits_round_trip_and_bound_values() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);

        let mut invalid = Limits::HARD;
        invalid.max_published_clients = 0;
        assert!(invalid.to_extensions().is_err());

        let mut unknown = extensions;
        unknown.0.push(Extension {
            tag: 99,
            required: true,
            value: Vec::new(),
        });
        assert!(Limits::from_extensions(&unknown).is_err());
    }

    #[test]
    fn client_record_golden_and_truncation() {
        let record = ClientRecord {
            session_id: [1; 16],
            client_instance: [2; 16],
            connected_server_ns: 3,
            idle_ns: 4,
            bytes_received: 5,
            bytes_sent: 6,
            name: "web".into(),
            release: "1".into(),
            label: "desk".into(),
            origin: Origin::Extension {
                extension_id: 7,
                definition_revision: 8,
                attempt: 9,
                task_id: 10,
                name: "task".into(),
            },
            extensions: Extensions::default(),
        };
        let bytes = record.encode().unwrap();
        assert_eq!(ClientRecord::decode(&bytes).unwrap(), record);
        for end in 0..bytes.len() {
            assert!(ClientRecord::decode(&bytes[..end]).is_err());
        }
    }

    #[test]
    fn unknown_optional_origin_is_skipped_but_required_fails() {
        let mut optional = Vec::new();
        put_u32(&mut optional, 7);
        put_u16(&mut optional, 99);
        put_u16(&mut optional, 0);
        optional.extend_from_slice(&[1, 2, 3]);
        assert!(matches!(
            Origin::decode(&optional).unwrap(),
            Origin::UnknownOptional { kind: 99, .. }
        ));
        optional[6] = 1;
        assert_eq!(
            Origin::decode(&optional),
            Err(Error::Invalid("unknown required Client origin"))
        );
    }

    #[test]
    fn active_subscriptions_are_typed_sorted_and_bounded() {
        let value = ActiveSubscriptions {
            terminals: vec![TerminalSubscription {
                terminal_handle: 1,
                view_id: 2,
                rows: 24,
                columns: 80,
            }],
            surfaces: vec![SurfaceSubscription {
                surface_handle: 3,
                view_id: 4,
                width: 1920,
                height: 1080,
                scale_120: 120,
            }],
            auxiliary: vec![AuxiliarySubscription {
                family: crate::family::KV,
                subscription_id: 5,
                resource_handle: 6,
            }],
        };
        let bytes = value.encode().unwrap();
        assert_eq!(ActiveSubscriptions::decode(&bytes).unwrap(), value);
        for end in 0..bytes.len() {
            assert!(ActiveSubscriptions::decode(&bytes[..end]).is_err());
        }
        let record = ClientRecord {
            session_id: [1; 16],
            client_instance: [2; 16],
            connected_server_ns: 0,
            idle_ns: 0,
            bytes_received: 0,
            bytes_sent: 0,
            name: "web".into(),
            release: String::new(),
            label: String::new(),
            origin: Origin::Unix {
                peer_pid: 0,
                peer_uid: 0,
                peer_gid: 0,
                socket_path: Vec::new(),
            },
            extensions: Extensions(vec![value.extension().unwrap()]),
        };
        assert_eq!(record.active_subscriptions().unwrap(), Some(value));
    }

    #[test]
    fn bandwidth_rates_are_exact_and_validate_record_extensions() {
        let rates = BandwidthRates {
            received_bytes_per_second: 1_000,
            sent_bytes_per_second: 2_000,
            sample_window_ns: 500_000_000,
        };
        let bytes = rates.encode().unwrap();
        assert_eq!(bytes.len(), 24);
        assert_eq!(BandwidthRates::decode(&bytes).unwrap(), rates);
        for end in 0..bytes.len() {
            assert!(BandwidthRates::decode(&bytes[..end]).is_err());
        }

        let patch = ClientPatch {
            session_id: [1; 16],
            extensions: Extensions(vec![rates.extension().unwrap()]),
        };
        assert_eq!(patch.bandwidth_rates().unwrap(), Some(rates));
        assert_eq!(
            ClientPatch::decode(&patch.encode().unwrap()).unwrap(),
            patch
        );

        let mut invalid = rates;
        invalid.sample_window_ns = 0;
        assert!(invalid.encode().is_err());
        assert!(
            ClientPatch {
                session_id: [1; 16],
                extensions: Extensions(vec![Extension {
                    tag: 99,
                    required: true,
                    value: Vec::new(),
                }]),
            }
            .encode()
            .is_err()
        );
    }
}
