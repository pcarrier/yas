use crate::codec::{
    Decode, Decoder, Encode, Error, Extensions, Result, limit_u32, limit_u64, put_string_u16,
    put_string_u32, put_u16, put_u64, read_limit_u32, read_limit_u64,
    reject_unknown_required_extensions,
};
use crate::prelude::*;
use crate::state::{Record, RecordKind};
use crate::transfer::{Descriptor, Direction, Mode};

pub const VERSION: u16 = crate::schema::relay::VERSION;
pub const TUNNEL_CONTENT_KIND: u16 = crate::schema::relay::TUNNEL_CONTENT_KIND as u16;
pub const EARLY_DATA_EXTENSION: u16 = crate::schema::relay::EARLY_DATA_EXTENSION as u16;
pub const MAX_EARLY_DATA: usize = crate::schema::relay::MAX_EARLY_DATA as usize;

pub mod request_kind {
    pub use crate::schema::relay::request::*;
}

pub mod event_kind {
    pub use crate::schema::relay::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Availability {
    Unknown = crate::schema::relay::AVAILABILITY_UNKNOWN as u8,
    Available = crate::schema::relay::AVAILABILITY_AVAILABLE as u8,
    Degraded = crate::schema::relay::AVAILABILITY_DEGRADED as u8,
    Unavailable = crate::schema::relay::AVAILABILITY_UNAVAILABLE as u8,
}

impl TryFrom<u8> for Availability {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::relay::AVAILABILITY_UNKNOWN as u8 => Ok(Self::Unknown),
            value if value == crate::schema::relay::AVAILABILITY_AVAILABLE as u8 => {
                Ok(Self::Available)
            }
            value if value == crate::schema::relay::AVAILABILITY_DEGRADED as u8 => {
                Ok(Self::Degraded)
            }
            value if value == crate::schema::relay::AVAILABILITY_UNAVAILABLE as u8 => {
                Ok(Self::Unavailable)
            }
            _ => Err(Error::Invalid("Relay availability")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TransportHint {
    Other = crate::schema::relay::TRANSPORT_OTHER as u8,
    Local = crate::schema::relay::TRANSPORT_LOCAL as u8,
    Ssh = crate::schema::relay::TRANSPORT_SSH as u8,
    Tcp = crate::schema::relay::TRANSPORT_TCP as u8,
    WebRtc = crate::schema::relay::TRANSPORT_WEBRTC as u8,
    Uplink = crate::schema::relay::TRANSPORT_UPLINK as u8,
    Relay = crate::schema::relay::TRANSPORT_RELAY as u8,
}

impl TryFrom<u8> for TransportHint {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::relay::TRANSPORT_OTHER as u8 => Ok(Self::Other),
            value if value == crate::schema::relay::TRANSPORT_LOCAL as u8 => Ok(Self::Local),
            value if value == crate::schema::relay::TRANSPORT_SSH as u8 => Ok(Self::Ssh),
            value if value == crate::schema::relay::TRANSPORT_TCP as u8 => Ok(Self::Tcp),
            value if value == crate::schema::relay::TRANSPORT_WEBRTC as u8 => Ok(Self::WebRtc),
            value if value == crate::schema::relay::TRANSPORT_UPLINK as u8 => Ok(Self::Uplink),
            value if value == crate::schema::relay::TRANSPORT_RELAY as u8 => Ok(Self::Relay),
            _ => Err(Error::Invalid("Relay transport hint")),
        }
    }
}

pub const ROUTE_DEFAULT: u16 = crate::schema::relay::ROUTE_DEFAULT as u16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RouteRecord {
    pub route_handle: u64,
    pub generation: u64,
    pub availability: Availability,
    pub transport_hint: TransportHint,
    pub is_default: bool,
    pub name: String,
    pub label: String,
    pub description: String,
    pub extensions: Extensions,
}

impl RouteRecord {
    fn validate(&self) -> Result<()> {
        if self.route_handle == 0 || self.name.is_empty() {
            return Err(Error::Invalid("Relay route identity"));
        }
        self.extensions.validate()
    }

    pub fn state_record(&self, kind: RecordKind) -> Result<Record> {
        if !matches!(kind, RecordKind::Add | RecordKind::Replace) {
            return Err(Error::Invalid("Relay route state record kind"));
        }
        Ok(Record {
            kind,
            required: false,
            body: self.encode()?,
        })
    }
}

impl Encode for RouteRecord {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.route_handle);
        put_u64(out, self.generation);
        out.push(self.availability as u8);
        out.push(self.transport_hint as u8);
        put_u16(out, if self.is_default { ROUTE_DEFAULT } else { 0 });
        put_string_u16(out, &self.name)?;
        put_string_u16(out, &self.label)?;
        put_string_u32(out, &self.description)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for RouteRecord {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let route_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let availability = Availability::try_from(decoder.u8()?)?;
        let transport_hint = TransportHint::try_from(decoder.u8()?)?;
        let flags = decoder.u16()?;
        if flags & !ROUTE_DEFAULT != 0 {
            return Err(Error::Invalid("Relay route flags"));
        }
        let value = Self {
            route_handle,
            generation,
            availability,
            transport_hint,
            is_default: flags & ROUTE_DEFAULT != 0,
            name: decoder.string_u16()?,
            label: decoder.string_u16()?,
            description: decoder.string_u32()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RemovedRoute {
    pub route_handle: u64,
    pub generation: u64,
}

impl RemovedRoute {
    pub fn state_record(self) -> Result<Record> {
        if self.route_handle == 0 {
            return Err(Error::Invalid("Relay removed route identity"));
        }
        let mut body = Vec::with_capacity(16);
        put_u64(&mut body, self.route_handle);
        put_u64(&mut body, self.generation);
        Ok(Record {
            kind: RecordKind::Remove,
            required: false,
            body,
        })
    }

    pub fn from_state_record(record: &Record) -> Result<Self> {
        if record.kind != RecordKind::Remove {
            return Err(Error::Invalid("Relay remove state record kind"));
        }
        let mut decoder = Decoder::new(&record.body);
        let value = Self {
            route_handle: decoder.u64()?,
            generation: decoder.u64()?,
        };
        decoder.finish()?;
        if value.route_handle == 0 {
            return Err(Error::Invalid("Relay removed route identity"));
        }
        Ok(value)
    }
}

pub fn route_from_state_record(record: &Record) -> Result<RouteRecord> {
    if !matches!(record.kind, RecordKind::Add | RecordKind::Replace) {
        return Err(Error::Invalid("Relay route state record kind"));
    }
    RouteRecord::decode(&record.body)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connect {
    pub route_handle: u64,
    pub generation: u64,
    pub initial_receive_credit: u64,
    pub extensions: Extensions,
}

impl Connect {
    fn validate(&self) -> Result<()> {
        if self.route_handle == 0 {
            return Err(Error::Invalid("Relay CONNECT identity"));
        }
        self.extensions.validate()?;
        if let Some(early_data) = self
            .extensions
            .0
            .iter()
            .find(|extension| extension.tag == EARLY_DATA_EXTENSION)
            && early_data.value.len() > MAX_EARLY_DATA
        {
            return Err(Error::LimitExceeded {
                limit: "Relay early data",
                actual: early_data.value.len() as u64,
                maximum: MAX_EARLY_DATA as u64,
            });
        }
        Ok(())
    }
}

impl Encode for Connect {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.route_handle);
        put_u64(out, self.generation);
        put_u64(out, self.initial_receive_credit);
        put_u16(out, 0);
        put_u16(out, 0);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Connect {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let route_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let initial_receive_credit = decoder.u64()?;
        if decoder.u16()? != 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("Relay CONNECT flags or reserved field"));
        }
        let value = Self {
            route_handle,
            generation,
            initial_receive_credit,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectResult {
    pub relay_handle: u64,
    pub route_handle: u64,
    pub generation: u64,
    pub transfer: Descriptor,
}

impl ConnectResult {
    fn validate(&self) -> Result<()> {
        if self.relay_handle == 0 || self.route_handle == 0 {
            return Err(Error::Invalid("Relay CONNECT result identity"));
        }
        if self.transfer.mode != Mode::Byte
            || self.transfer.direction != Direction::BIDIRECTIONAL
            || self.transfer.content_family != crate::family::RELAY
            || self.transfer.content_kind != TUNNEL_CONTENT_KIND
            || self.transfer.content_version != VERSION
            || !self.transfer.sensitive_content()?
        {
            return Err(Error::Invalid("Relay tunnel Transfer descriptor"));
        }
        self.transfer.validate()
    }
}

impl Encode for ConnectResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.relay_handle);
        put_u64(out, self.route_handle);
        put_u64(out, self.generation);
        self.transfer.encode_to(out)
    }
}

impl Decode for ConnectResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let relay_handle = decoder.u64()?;
        let route_handle = decoder.u64()?;
        let generation = decoder.u64()?;
        let transfer = Descriptor::decode(decoder.rest())?;
        decoder.finish()?;
        let value = Self {
            relay_handle,
            route_handle,
            generation,
            transfer,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disconnect {
    pub relay_handle: u64,
    pub reason: String,
}

impl Encode for Disconnect {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.relay_handle == 0 {
            return Err(Error::Invalid("zero Relay handle"));
        }
        put_u64(out, self.relay_handle);
        put_string_u32(out, &self.reason)
    }
}

impl Decode for Disconnect {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            relay_handle: decoder.u64()?,
            reason: decoder.string_u32()?,
        };
        decoder.finish()?;
        if value.relay_handle == 0 {
            return Err(Error::Invalid("zero Relay handle"));
        }
        Ok(value)
    }
}

/// Typed Relay entries carried in a family descriptor's limit extensions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_routes: u32,
    pub max_links_per_session: u32,
    pub max_pending_connects: u32,
    pub max_early_data: u32,
    pub connect_timeout_ns: u64,
    pub max_buffered_per_link: u64,
}

impl Limits {
    pub const HARD: Self = Self {
        max_routes: crate::schema::relay::MAX_ROUTES as u32,
        max_links_per_session: crate::schema::relay::MAX_LINKS_PER_SESSION as u32,
        max_pending_connects: crate::schema::relay::MAX_PENDING_CONNECTS as u32,
        max_early_data: crate::schema::relay::MAX_EARLY_DATA as u32,
        connect_timeout_ns: crate::schema::relay::CONNECT_TIMEOUT_NS,
        max_buffered_per_link: crate::schema::relay::MAX_BUFFERED_PER_LINK,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let valid_u32 = |value: u32, maximum: u32| value != 0 && value <= maximum;
        if !valid_u32(self.max_routes, hard.max_routes)
            || !valid_u32(self.max_links_per_session, hard.max_links_per_session)
            || !valid_u32(self.max_pending_connects, hard.max_pending_connects)
            || self.max_pending_connects > self.max_links_per_session
            || self.max_early_data > hard.max_early_data
            || self.connect_timeout_ns == 0
            || self.connect_timeout_ns > hard.connect_timeout_ns
            || self.max_buffered_per_link == 0
            || self.max_buffered_per_link > hard.max_buffered_per_link
        {
            return Err(Error::Invalid("Relay family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(crate::schema::relay::LIMIT_MAX_ROUTES, self.max_routes),
            limit_u32(
                crate::schema::relay::LIMIT_MAX_LINKS_PER_SESSION,
                self.max_links_per_session,
            ),
            limit_u32(
                crate::schema::relay::LIMIT_MAX_PENDING_CONNECTS,
                self.max_pending_connects,
            ),
            limit_u32(
                crate::schema::relay::LIMIT_MAX_EARLY_DATA,
                self.max_early_data,
            ),
            limit_u64(
                crate::schema::relay::LIMIT_CONNECT_TIMEOUT_NS,
                self.connect_timeout_ns,
            ),
            limit_u64(
                crate::schema::relay::LIMIT_MAX_BUFFERED_PER_LINK,
                self.max_buffered_per_link,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        reject_unknown_required_extensions(
            extensions,
            &[
                crate::schema::relay::LIMIT_MAX_ROUTES as u16,
                crate::schema::relay::LIMIT_MAX_LINKS_PER_SESSION as u16,
                crate::schema::relay::LIMIT_MAX_PENDING_CONNECTS as u16,
                crate::schema::relay::LIMIT_MAX_EARLY_DATA as u16,
                crate::schema::relay::LIMIT_CONNECT_TIMEOUT_NS as u16,
                crate::schema::relay::LIMIT_MAX_BUFFERED_PER_LINK as u16,
            ],
            "unknown required Relay family limit",
        )?;
        let value = Self {
            max_routes: read_limit_u32(extensions, crate::schema::relay::LIMIT_MAX_ROUTES)?,
            max_links_per_session: read_limit_u32(
                extensions,
                crate::schema::relay::LIMIT_MAX_LINKS_PER_SESSION,
            )?,
            max_pending_connects: read_limit_u32(
                extensions,
                crate::schema::relay::LIMIT_MAX_PENDING_CONNECTS,
            )?,
            max_early_data: read_limit_u32(extensions, crate::schema::relay::LIMIT_MAX_EARLY_DATA)?,
            connect_timeout_ns: read_limit_u64(
                extensions,
                crate::schema::relay::LIMIT_CONNECT_TIMEOUT_NS,
            )?,
            max_buffered_per_link: read_limit_u64(
                extensions,
                crate::schema::relay::LIMIT_MAX_BUFFERED_PER_LINK,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Extension;

    #[test]
    fn route_golden_round_trip() {
        let route = RouteRecord {
            route_handle: 1,
            generation: 2,
            availability: Availability::Available,
            transport_hint: TransportHint::Ssh,
            is_default: true,
            name: "prod".into(),
            label: "Production".into(),
            description: "Remote".into(),
            extensions: Extensions::default(),
        };
        let bytes = route.encode().unwrap();
        assert_eq!(
            &bytes[..20],
            &[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 2, 1, 0]
        );
        assert_eq!(RouteRecord::decode(&bytes).unwrap(), route);
        for end in 0..bytes.len() {
            assert!(RouteRecord::decode(&bytes[..end]).is_err());
        }
    }

    #[test]
    fn tunnel_descriptor_is_always_sensitive() {
        let mut value = ConnectResult {
            relay_handle: 1,
            route_handle: 2,
            generation: 3,
            transfer: Descriptor {
                transfer_id: 2,
                mode: Mode::Byte,
                direction: Direction::BIDIRECTIONAL,
                receiver_send_credit: 4096,
                sender_send_credit: 4096,
                max_item_bytes: 0,
                max_chunk_bytes: 4096,
                content_family: crate::family::RELAY,
                content_kind: TUNNEL_CONTENT_KIND,
                content_version: VERSION,
                extensions: Extensions::default(),
            },
        };
        assert!(value.encode().is_err());
        value.transfer.extensions = Extensions(vec![Extension {
            tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
            required: true,
            value: Vec::new(),
        }]);
        let bytes = value.encode().unwrap();
        assert_eq!(ConnectResult::decode(&bytes).unwrap(), value);
    }

    #[test]
    fn family_limits_are_hard_bounded_and_typed() {
        let limits = Limits::HARD;
        let extensions = limits.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), limits);

        let mut invalid = limits;
        invalid.max_routes += 1;
        assert_eq!(
            invalid.validate(),
            Err(Error::Invalid("Relay family limit"))
        );
        let mut invalid = limits;
        invalid.max_pending_connects = invalid.max_links_per_session + 1;
        assert_eq!(
            invalid.validate(),
            Err(Error::Invalid("Relay family limit"))
        );

        let mut unknown = extensions;
        unknown.0.push(Extension {
            tag: 99,
            required: true,
            value: Vec::new(),
        });
        assert_eq!(
            Limits::from_extensions(&unknown),
            Err(Error::Invalid("unknown required Relay family limit"))
        );
    }
}
