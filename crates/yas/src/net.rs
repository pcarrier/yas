//! YAS raw network-endpoint family wire values.

use crate::prelude::*;

use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, put_bytes_u16, put_bytes_u32,
    put_string_u16, put_u16, put_u32, put_u64,
};
use crate::transfer::{Descriptor, Direction as TransferDirection, Mode as TransferMode};

pub const VERSION: u16 = crate::schema::net::VERSION;
pub const MAX_HOST_BYTES: usize = crate::schema::net::MAX_HOST_BYTES as usize;
pub const MAX_LOCAL_ADDRESS_BYTES: usize = crate::schema::net::MAX_LOCAL_ADDRESS_BYTES as usize;
pub const MAX_PIPE_NAME_BYTES: usize = crate::schema::net::MAX_PIPE_NAME_BYTES as usize;
pub const MAX_ALPN_PROTOCOLS: usize = crate::schema::net::MAX_ALPN_PROTOCOLS as usize;
pub const MAX_ALPN_BYTES: usize = crate::schema::net::MAX_ALPN_BYTES as usize;
pub const MAX_EARLY_DATA_BYTES: usize = crate::schema::net::MAX_EARLY_DATA_BYTES as usize;
pub const MAX_FLOWS_PER_SESSION: u32 = crate::schema::net::MAX_FLOWS_PER_SESSION as u32;
pub const MAX_PENDING_OPENS: u32 = crate::schema::net::MAX_PENDING_OPENS as u32;
pub const MAX_BUFFERED_PER_FLOW: u64 = crate::schema::net::MAX_BUFFERED_PER_FLOW;
pub const MAX_DATAGRAM_PAYLOAD: usize = crate::schema::net::MAX_DATAGRAM_PAYLOAD as usize;
pub const MAX_DATAGRAM_QUEUE: u32 = crate::schema::net::MAX_DATAGRAM_QUEUE as u32;
pub const CONNECT_TIMEOUT_NS: u64 = crate::schema::net::CONNECT_TIMEOUT_NS;

pub mod request_kind {
    pub use crate::schema::net::request::*;
}

pub mod event_kind {
    pub use crate::schema::net::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum UnixNameKind {
    Filesystem = crate::schema::net::UNIX_FILESYSTEM as u8,
    Abstract = crate::schema::net::UNIX_ABSTRACT as u8,
}

impl TryFrom<u8> for UnixNameKind {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Filesystem as u8 => Ok(Self::Filesystem),
            value if value == Self::Abstract as u8 => Ok(Self::Abstract),
            _ => Err(Error::Invalid("Net Unix address kind")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PipeMode {
    Auto = crate::schema::net::PIPE_MODE_AUTO as u8,
    Byte = crate::schema::net::PIPE_MODE_BYTE as u8,
    Message = crate::schema::net::PIPE_MODE_MESSAGE as u8,
}

impl TryFrom<u8> for PipeMode {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Auto as u8 => Ok(Self::Auto),
            value if value == Self::Byte as u8 => Ok(Self::Byte),
            value if value == Self::Message as u8 => Ok(Self::Message),
            _ => Err(Error::Invalid("Net pipe mode")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnixName {
    pub kind: UnixNameKind,
    pub name: Vec<u8>,
}

impl UnixName {
    fn validate(&self) -> Result<()> {
        if self.name.is_empty() {
            return Err(Error::Invalid("empty Net Unix address"));
        }
        if self.name.len() > MAX_LOCAL_ADDRESS_BYTES {
            return Err(limit(
                "Net Unix address bytes",
                self.name.len(),
                MAX_LOCAL_ADDRESS_BYTES,
            ));
        }
        if self.kind == UnixNameKind::Filesystem && self.name.contains(&0) {
            return Err(Error::Invalid("NUL in Net Unix filesystem address"));
        }
        Ok(())
    }

    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.kind as u8);
        out.extend_from_slice(&[0; 3]);
        put_bytes_u32(out, &self.name)
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let kind = UnixNameKind::try_from(decoder.u8()?)?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Net Unix address reserved bytes"));
        }
        let value = Self {
            kind,
            name: decoder.len_bytes_u32()?.to_vec(),
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Address {
    Tcp {
        host: String,
        port: u16,
    },
    Udp {
        host: String,
        port: u16,
    },
    UnixStream(UnixName),
    UnixDatagram(UnixName),
    UnixSeqpacket(UnixName),
    WindowsPipe {
        requested_mode: PipeMode,
        name: String,
    },
}

impl Address {
    pub fn is_datagram(&self) -> bool {
        matches!(self, Self::Udp { .. } | Self::UnixDatagram(_))
    }

    pub fn is_tcp(&self) -> bool {
        matches!(self, Self::Tcp { .. })
    }

    fn validate(&self) -> Result<()> {
        match self {
            Self::Tcp { host, port } | Self::Udp { host, port } => {
                if host.is_empty() || host.as_bytes().contains(&0) {
                    return Err(Error::Invalid("Net host"));
                }
                if host.len() > MAX_HOST_BYTES {
                    return Err(limit("Net host bytes", host.len(), MAX_HOST_BYTES));
                }
                if *port == 0 {
                    return Err(Error::Invalid("zero Net port"));
                }
                Ok(())
            }
            Self::UnixStream(name) | Self::UnixDatagram(name) | Self::UnixSeqpacket(name) => {
                name.validate()
            }
            Self::WindowsPipe { name, .. } => {
                if name.is_empty() || name.as_bytes().contains(&0) {
                    return Err(Error::Invalid("Net Windows pipe name"));
                }
                if name.len() > MAX_PIPE_NAME_BYTES {
                    return Err(limit(
                        "Net Windows pipe name bytes",
                        name.len(),
                        MAX_PIPE_NAME_BYTES,
                    ));
                }
                Ok(())
            }
        }
    }

    fn kind(&self) -> u8 {
        match self {
            Self::Tcp { .. } => crate::schema::net::ADDRESS_TCP as u8,
            Self::Udp { .. } => crate::schema::net::ADDRESS_UDP as u8,
            Self::UnixStream(_) => crate::schema::net::ADDRESS_UNIX_STREAM as u8,
            Self::UnixDatagram(_) => crate::schema::net::ADDRESS_UNIX_DATAGRAM as u8,
            Self::UnixSeqpacket(_) => crate::schema::net::ADDRESS_UNIX_SEQPACKET as u8,
            Self::WindowsPipe { .. } => crate::schema::net::ADDRESS_WINDOWS_PIPE as u8,
        }
    }
}

impl Encode for Address {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.kind());
        out.extend_from_slice(&[0; 3]);
        match self {
            Self::Tcp { host, port } | Self::Udp { host, port } => {
                put_string_u16(out, host)?;
                put_u16(out, *port);
                put_u16(out, 0);
            }
            Self::UnixStream(name) | Self::UnixDatagram(name) | Self::UnixSeqpacket(name) => {
                name.encode_to(out)?;
            }
            Self::WindowsPipe {
                requested_mode,
                name,
            } => {
                out.push(*requested_mode as u8);
                out.extend_from_slice(&[0; 3]);
                put_string_u16(out, name)?;
            }
        }
        Ok(())
    }
}

impl Decode for Address {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let kind = decoder.u8()?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Net address reserved bytes"));
        }
        let value = match kind {
            value if value == crate::schema::net::ADDRESS_TCP as u8 => {
                let host = decoder.string_u16()?;
                let port = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Net TCP address reserved field"));
                }
                Self::Tcp { host, port }
            }
            value if value == crate::schema::net::ADDRESS_UDP as u8 => {
                let host = decoder.string_u16()?;
                let port = decoder.u16()?;
                if decoder.u16()? != 0 {
                    return Err(Error::Invalid("Net UDP address reserved field"));
                }
                Self::Udp { host, port }
            }
            value if value == crate::schema::net::ADDRESS_UNIX_STREAM as u8 => {
                Self::UnixStream(UnixName::decode_from(&mut decoder)?)
            }
            value if value == crate::schema::net::ADDRESS_UNIX_DATAGRAM as u8 => {
                Self::UnixDatagram(UnixName::decode_from(&mut decoder)?)
            }
            value if value == crate::schema::net::ADDRESS_UNIX_SEQPACKET as u8 => {
                Self::UnixSeqpacket(UnixName::decode_from(&mut decoder)?)
            }
            value if value == crate::schema::net::ADDRESS_WINDOWS_PIPE as u8 => {
                let requested_mode = PipeMode::try_from(decoder.u8()?)?;
                if decoder.take(3)? != [0; 3] {
                    return Err(Error::Invalid("Net pipe address reserved bytes"));
                }
                Self::WindowsPipe {
                    requested_mode,
                    name: decoder.string_u16()?,
                }
            }
            _ => return Err(Error::Invalid("Net address kind")),
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TlsVerification {
    Strict = crate::schema::net::TLS_VERIFY_STRICT as u8,
    Insecure = crate::schema::net::TLS_VERIFY_INSECURE as u8,
}

impl TryFrom<u8> for TlsVerification {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Strict as u8 => Ok(Self::Strict),
            value if value == Self::Insecure as u8 => Ok(Self::Insecure),
            _ => Err(Error::Invalid("Net TLS verification mode")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsOptions {
    pub verification: TlsVerification,
    pub sni: String,
    pub alpn: Vec<Vec<u8>>,
    pub extensions: Extensions,
}

impl TlsOptions {
    fn validate(&self) -> Result<()> {
        if self.sni.len() > MAX_HOST_BYTES || self.sni.as_bytes().contains(&0) {
            return Err(Error::Invalid("Net TLS SNI"));
        }
        if self.alpn.len() > MAX_ALPN_PROTOCOLS {
            return Err(limit(
                "Net TLS ALPN protocol count",
                self.alpn.len(),
                MAX_ALPN_PROTOCOLS,
            ));
        }
        let mut unique = BTreeSet::new();
        for protocol in &self.alpn {
            if protocol.is_empty() || protocol.len() > MAX_ALPN_BYTES || !unique.insert(protocol) {
                return Err(Error::Invalid("Net TLS ALPN protocol"));
            }
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for TlsOptions {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(self.verification as u8);
        out.extend_from_slice(&[0; 3]);
        put_string_u16(out, &self.sni)?;
        put_u16(
            out,
            self.alpn
                .len()
                .try_into()
                .map_err(|_| Error::LengthOverflow)?,
        );
        put_u16(out, 0);
        for protocol in &self.alpn {
            put_bytes_u16(out, protocol)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for TlsOptions {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let verification = TlsVerification::try_from(decoder.u8()?)?;
        if decoder.take(3)? != [0; 3] {
            return Err(Error::Invalid("Net TLS reserved bytes"));
        }
        let sni = decoder.string_u16()?;
        let count = usize::from(decoder.u16()?);
        if decoder.u16()? != 0 || count > MAX_ALPN_PROTOCOLS || count > decoder.remaining() / 2 {
            return Err(Error::Invalid("Net TLS ALPN count or reserved field"));
        }
        let mut alpn = Vec::with_capacity(count);
        for _ in 0..count {
            alpn.push(decoder.len_bytes_u16()?.to_vec());
        }
        let value = Self {
            verification,
            sni,
            alpn,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DeliveryPreference {
    NativeRequired = crate::schema::net::DELIVERY_NATIVE_REQUIRED as u8,
    PreferNative = crate::schema::net::DELIVERY_PREFER_NATIVE as u8,
    ReliableTunnel = crate::schema::net::DELIVERY_REQUIRE_RELIABLE_TUNNEL as u8,
    NotApplicable = crate::schema::net::DELIVERY_PREFERENCE_NOT_APPLICABLE as u8,
}

impl TryFrom<u8> for DeliveryPreference {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::NativeRequired as u8 => Ok(Self::NativeRequired),
            value if value == Self::PreferNative as u8 => Ok(Self::PreferNative),
            value if value == Self::ReliableTunnel as u8 => Ok(Self::ReliableTunnel),
            value if value == Self::NotApplicable as u8 => Ok(Self::NotApplicable),
            _ => Err(Error::Invalid("Net delivery preference")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DropPolicy {
    Oldest = crate::schema::net::DROP_OLDEST as u8,
    Latest = crate::schema::net::DROP_LATEST as u8,
    NotApplicable = crate::schema::net::DROP_NOT_APPLICABLE as u8,
}

impl TryFrom<u8> for DropPolicy {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Oldest as u8 => Ok(Self::Oldest),
            value if value == Self::Latest as u8 => Ok(Self::Latest),
            value if value == Self::NotApplicable as u8 => Ok(Self::NotApplicable),
            _ => Err(Error::Invalid("Net datagram drop policy")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub operation_id: [u8; 16],
    pub address: Address,
    pub delivery_preference: DeliveryPreference,
    pub drop_policy: DropPolicy,
    pub initial_receive_credit: u64,
    pub early_data: Vec<u8>,
    pub tls_options: Option<TlsOptions>,
    pub extensions: Extensions,
}

impl Open {
    fn validate(&self) -> Result<()> {
        validate_operation_id(&self.operation_id)?;
        self.address.validate()?;
        if self.early_data.len() > MAX_EARLY_DATA_BYTES {
            return Err(limit(
                "Net early-data bytes",
                self.early_data.len(),
                MAX_EARLY_DATA_BYTES,
            ));
        }
        if self.address.is_datagram() {
            if self.delivery_preference == DeliveryPreference::NotApplicable
                || self.drop_policy == DropPolicy::NotApplicable
                || self.initial_receive_credit != 0
                || !self.early_data.is_empty()
                || self.tls_options.is_some()
            {
                return Err(Error::Invalid("Net datagram OPEN options"));
            }
        } else if self.delivery_preference != DeliveryPreference::NotApplicable
            || self.drop_policy != DropPolicy::NotApplicable
            || self.initial_receive_credit == 0
        {
            return Err(Error::Invalid("Net reliable OPEN options"));
        }
        if let Some(tls) = &self.tls_options {
            if !self.address.is_tcp() {
                return Err(Error::Invalid("Net TLS on non-TCP endpoint"));
            }
            tls.validate()?;
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for Open {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.extend_from_slice(&self.operation_id);
        put_bytes_u32(out, &self.address.encode()?)?;
        out.push(self.delivery_preference as u8);
        out.push(self.drop_policy as u8);
        put_u16(out, 0);
        put_u64(out, self.initial_receive_credit);
        put_bytes_u32(out, &self.early_data)?;
        match &self.tls_options {
            Some(options) => put_bytes_u32(out, &options.encode()?)?,
            None => put_bytes_u32(out, &[])?,
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Open {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let operation_id = decoder.array_16()?;
        let address = Address::decode(decoder.len_bytes_u32()?)?;
        let delivery_preference = DeliveryPreference::try_from(decoder.u8()?)?;
        let drop_policy = DropPolicy::try_from(decoder.u8()?)?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Net OPEN reserved field"));
        }
        let initial_receive_credit = decoder.u64()?;
        let early_data = decoder.len_bytes_u32()?.to_vec();
        let tls = decoder.len_bytes_u32()?;
        let tls_options = if tls.is_empty() {
            None
        } else {
            Some(TlsOptions::decode(tls)?)
        };
        let value = Self {
            operation_id,
            address,
            delivery_preference,
            drop_policy,
            initial_receive_credit,
            early_data,
            tls_options,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Close {
    pub flow_handle: u64,
    pub operation_id: [u8; 16],
    pub extensions: Extensions,
}

impl Encode for Close {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.flow_handle)?;
        validate_operation_id(&self.operation_id)?;
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.flow_handle);
        out.extend_from_slice(&self.operation_id);
        self.extensions.encode_tail(out)
    }
}

impl Decode for Close {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            flow_handle: decoder.u64()?,
            operation_id: decoder.array_16()?,
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
pub enum FlowMode {
    Byte = crate::schema::net::MODE_BYTE as u8,
    Message = crate::schema::net::MODE_MESSAGE as u8,
    Datagram = crate::schema::net::MODE_DATAGRAM as u8,
}

impl TryFrom<u8> for FlowMode {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::Byte as u8 => Ok(Self::Byte),
            value if value == Self::Message as u8 => Ok(Self::Message),
            value if value == Self::Datagram as u8 => Ok(Self::Datagram),
            _ => Err(Error::Invalid("Net flow mode")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlowDirection(u8);

impl FlowDirection {
    pub const CLIENT_TO_PEER: Self = Self(crate::schema::net::DIRECTION_CLIENT_TO_PEER as u8);
    pub const PEER_TO_CLIENT: Self = Self(crate::schema::net::DIRECTION_PEER_TO_CLIENT as u8);
    pub const DUPLEX: Self = Self(crate::schema::net::DIRECTION_DUPLEX as u8);

    pub fn bits(self) -> u8 {
        self.0
    }

    fn from_bits(bits: u8) -> Result<Self> {
        let known = crate::schema::net::DIRECTION_DUPLEX as u8;
        if bits == 0 || bits & !known != 0 {
            return Err(Error::Invalid("Net flow direction"));
        }
        Ok(Self(bits))
    }

    fn transfer_direction(self) -> TransferDirection {
        TransferDirection {
            receiver_to_sender: self.0 & crate::schema::net::DIRECTION_CLIENT_TO_PEER as u8 != 0,
            sender_to_receiver: self.0 & crate::schema::net::DIRECTION_PEER_TO_CLIENT as u8 != 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DatagramDelivery {
    NotApplicable = crate::schema::net::DELIVERY_NOT_APPLICABLE as u8,
    Native = crate::schema::net::DELIVERY_NATIVE_DATAGRAM as u8,
    ReliableTunnel = crate::schema::net::DELIVERY_RELIABLE_TUNNEL as u8,
}

impl TryFrom<u8> for DatagramDelivery {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == Self::NotApplicable as u8 => Ok(Self::NotApplicable),
            value if value == Self::Native as u8 => Ok(Self::Native),
            value if value == Self::ReliableTunnel as u8 => Ok(Self::ReliableTunnel),
            _ => Err(Error::Invalid("Net selected datagram delivery")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub flow_handle: u64,
    pub mode: FlowMode,
    pub direction: FlowDirection,
    pub selected_delivery: DatagramDelivery,
    pub max_datagram_payload: u32,
    pub server_instance_limit: u32,
    pub max_message_bytes: u64,
    pub local_address: Option<Address>,
    pub peer_address: Address,
    pub negotiated_alpn: Vec<u8>,
    pub descriptor: Option<Descriptor>,
    pub extensions: Extensions,
}

impl Endpoint {
    fn validate(&self) -> Result<()> {
        validate_handle(self.flow_handle)?;
        self.peer_address.validate()?;
        if let Some(address) = &self.local_address {
            address.validate()?;
        }
        if self.negotiated_alpn.len() > MAX_ALPN_BYTES {
            return Err(limit(
                "Net negotiated ALPN bytes",
                self.negotiated_alpn.len(),
                MAX_ALPN_BYTES,
            ));
        }
        match self.mode {
            FlowMode::Datagram => {
                if !self.peer_address.is_datagram()
                    || self.descriptor.is_some()
                    || self.selected_delivery == DatagramDelivery::NotApplicable
                    || self.max_datagram_payload == 0
                    || self.max_datagram_payload as usize > MAX_DATAGRAM_PAYLOAD
                    || self.server_instance_limit != 0
                    || self.max_message_bytes != 0
                    || !self.negotiated_alpn.is_empty()
                {
                    return Err(Error::Invalid("Net datagram endpoint"));
                }
            }
            FlowMode::Byte | FlowMode::Message => {
                if self.peer_address.is_datagram()
                    || self.selected_delivery != DatagramDelivery::NotApplicable
                    || self.max_datagram_payload != 0
                {
                    return Err(Error::Invalid("Net reliable endpoint"));
                }
                let descriptor = self
                    .descriptor
                    .as_ref()
                    .ok_or(Error::Invalid("missing Net Transfer descriptor"))?;
                validate_flow_descriptor(descriptor, self.mode, self.direction)?;
                if self.mode == FlowMode::Byte && self.max_message_bytes != 0 {
                    return Err(Error::Invalid("Net BYTE maximum message bytes"));
                }
                if self.mode == FlowMode::Message
                    && (self.max_message_bytes == 0
                        || descriptor.max_item_bytes != self.max_message_bytes)
                {
                    return Err(Error::Invalid("Net MESSAGE maximum message bytes"));
                }
                if !self.peer_address.is_tcp() && !self.negotiated_alpn.is_empty() {
                    return Err(Error::Invalid("Net ALPN on non-TCP endpoint"));
                }
            }
        }
        reject_unknown_required(&self.extensions, &[])
    }
}

impl Encode for Endpoint {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.flow_handle);
        out.push(self.mode as u8);
        out.push(self.direction.bits());
        out.push(self.selected_delivery as u8);
        out.push(0);
        put_u32(out, self.max_datagram_payload);
        put_u32(out, self.server_instance_limit);
        put_u64(out, self.max_message_bytes);
        match &self.local_address {
            Some(address) => put_bytes_u32(out, &address.encode()?)?,
            None => put_bytes_u32(out, &[])?,
        }
        put_bytes_u32(out, &self.peer_address.encode()?)?;
        put_bytes_u16(out, &self.negotiated_alpn)?;
        match &self.descriptor {
            Some(descriptor) => put_bytes_u32(out, &descriptor.encode()?)?,
            None => put_bytes_u32(out, &[])?,
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for Endpoint {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flow_handle = decoder.u64()?;
        let mode = FlowMode::try_from(decoder.u8()?)?;
        let direction = FlowDirection::from_bits(decoder.u8()?)?;
        let selected_delivery = DatagramDelivery::try_from(decoder.u8()?)?;
        if decoder.u8()? != 0 {
            return Err(Error::Invalid("Net endpoint reserved byte"));
        }
        let max_datagram_payload = decoder.u32()?;
        let server_instance_limit = decoder.u32()?;
        let max_message_bytes = decoder.u64()?;
        let local = decoder.len_bytes_u32()?;
        let local_address = if local.is_empty() {
            None
        } else {
            Some(Address::decode(local)?)
        };
        let peer_address = Address::decode(decoder.len_bytes_u32()?)?;
        let negotiated_alpn = decoder.len_bytes_u16()?.to_vec();
        let descriptor = decoder.len_bytes_u32()?;
        let descriptor = if descriptor.is_empty() {
            None
        } else {
            Some(Descriptor::decode(descriptor)?)
        };
        let value = Self {
            flow_handle,
            mode,
            direction,
            selected_delivery,
            max_datagram_payload,
            server_instance_limit,
            max_message_bytes,
            local_address,
            peer_address,
            negotiated_alpn,
            descriptor,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Datagram {
    pub flow_handle: u64,
    pub sequence: u64,
    pub payload: Vec<u8>,
}

impl Encode for Datagram {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.flow_handle)?;
        if self.payload.len() > MAX_DATAGRAM_PAYLOAD {
            return Err(limit(
                "Net datagram payload bytes",
                self.payload.len(),
                MAX_DATAGRAM_PAYLOAD,
            ));
        }
        put_u64(out, self.flow_handle);
        put_u64(out, self.sequence);
        out.extend_from_slice(&self.payload);
        Ok(())
    }
}

impl Decode for Datagram {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            flow_handle: decoder.u64()?,
            sequence: decoder.u64()?,
            payload: decoder.rest().to_vec(),
        };
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DatagramStats {
    pub flow_handle: u64,
    pub revision: u64,
    pub final_stats: bool,
    pub client_to_peer_delivered: u64,
    pub peer_to_client_delivered: u64,
    pub client_oversized_drops: u64,
    pub peer_oversized_drops: u64,
    pub client_congestive_drops: u64,
    pub peer_congestive_drops: u64,
    pub transport_errors: u64,
    pub extensions: Extensions,
}

impl Encode for DatagramStats {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_handle(self.flow_handle)?;
        if self.revision == 0 {
            return Err(Error::Invalid("zero Net datagram-stats revision"));
        }
        reject_unknown_required(&self.extensions, &[])?;
        put_u64(out, self.flow_handle);
        put_u64(out, self.revision);
        put_u16(
            out,
            if self.final_stats {
                crate::schema::net::DATAGRAM_STATS_FINAL as u16
            } else {
                0
            },
        );
        put_u16(out, 0);
        put_u64(out, self.client_to_peer_delivered);
        put_u64(out, self.peer_to_client_delivered);
        put_u64(out, self.client_oversized_drops);
        put_u64(out, self.peer_oversized_drops);
        put_u64(out, self.client_congestive_drops);
        put_u64(out, self.peer_congestive_drops);
        put_u64(out, self.transport_errors);
        self.extensions.encode_tail(out)
    }
}

impl Decode for DatagramStats {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let flow_handle = decoder.u64()?;
        let revision = decoder.u64()?;
        let flags = decoder.u16()?;
        if flags & !(crate::schema::net::DATAGRAM_STATS_FINAL as u16) != 0 || decoder.u16()? != 0 {
            return Err(Error::Invalid("Net datagram-stats flags or reserved field"));
        }
        let value = Self {
            flow_handle,
            revision,
            final_stats: flags & crate::schema::net::DATAGRAM_STATS_FINAL as u16 != 0,
            client_to_peer_delivered: decoder.u64()?,
            peer_to_client_delivered: decoder.u64()?,
            client_oversized_drops: decoder.u64()?,
            peer_oversized_drops: decoder.u64()?,
            client_congestive_drops: decoder.u64()?,
            peer_congestive_drops: decoder.u64()?,
            transport_errors: decoder.u64()?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        let mut ignored = Vec::new();
        value.encode_to(&mut ignored)?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_host_bytes: u32,
    pub max_local_address_bytes: u32,
    pub max_pipe_name_bytes: u32,
    pub max_alpn_protocols: u32,
    pub max_alpn_bytes: u32,
    pub max_early_data_bytes: u32,
    pub max_flows_per_session: u32,
    pub max_pending_opens: u32,
    pub max_buffered_per_flow: u64,
    pub max_datagram_payload: u32,
    pub max_datagram_queue: u32,
    pub connect_timeout_ns: u64,
    pub max_mutation_replays: u32,
}

impl Limits {
    pub const HARD: Self = Self {
        max_host_bytes: MAX_HOST_BYTES as u32,
        max_local_address_bytes: MAX_LOCAL_ADDRESS_BYTES as u32,
        max_pipe_name_bytes: MAX_PIPE_NAME_BYTES as u32,
        max_alpn_protocols: MAX_ALPN_PROTOCOLS as u32,
        max_alpn_bytes: MAX_ALPN_BYTES as u32,
        max_early_data_bytes: MAX_EARLY_DATA_BYTES as u32,
        max_flows_per_session: MAX_FLOWS_PER_SESSION,
        max_pending_opens: MAX_PENDING_OPENS,
        max_buffered_per_flow: MAX_BUFFERED_PER_FLOW,
        max_datagram_payload: MAX_DATAGRAM_PAYLOAD as u32,
        max_datagram_queue: MAX_DATAGRAM_QUEUE,
        connect_timeout_ns: CONNECT_TIMEOUT_NS,
        max_mutation_replays: crate::schema::net::MAX_MUTATION_REPLAYS as u32,
    };

    pub fn validate(self) -> Result<()> {
        let hard = Self::HARD;
        let valid_u32 = |value: u32, maximum: u32| value != 0 && value <= maximum;
        if !valid_u32(self.max_host_bytes, hard.max_host_bytes)
            || !valid_u32(self.max_local_address_bytes, hard.max_local_address_bytes)
            || !valid_u32(self.max_pipe_name_bytes, hard.max_pipe_name_bytes)
            || !valid_u32(self.max_alpn_protocols, hard.max_alpn_protocols)
            || !valid_u32(self.max_alpn_bytes, hard.max_alpn_bytes)
            || self.max_early_data_bytes > hard.max_early_data_bytes
            || !valid_u32(self.max_flows_per_session, hard.max_flows_per_session)
            || !valid_u32(self.max_pending_opens, hard.max_pending_opens)
            || self.max_buffered_per_flow == 0
            || self.max_buffered_per_flow > hard.max_buffered_per_flow
            || !valid_u32(self.max_datagram_payload, hard.max_datagram_payload)
            || !valid_u32(self.max_datagram_queue, hard.max_datagram_queue)
            || self.connect_timeout_ns == 0
            || self.connect_timeout_ns > hard.connect_timeout_ns
            || !valid_u32(self.max_mutation_replays, hard.max_mutation_replays)
        {
            return Err(Error::Invalid("Net family limit"));
        }
        Ok(())
    }

    pub fn to_extensions(self) -> Result<Extensions> {
        self.validate()?;
        Ok(Extensions(vec![
            limit_u32(
                crate::schema::net::LIMIT_MAX_HOST_BYTES,
                self.max_host_bytes,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_LOCAL_ADDRESS_BYTES,
                self.max_local_address_bytes,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_PIPE_NAME_BYTES,
                self.max_pipe_name_bytes,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_ALPN_PROTOCOLS,
                self.max_alpn_protocols,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_ALPN_BYTES,
                self.max_alpn_bytes,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_EARLY_DATA_BYTES,
                self.max_early_data_bytes,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_FLOWS_PER_SESSION,
                self.max_flows_per_session,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_PENDING_OPENS,
                self.max_pending_opens,
            ),
            limit_u64(
                crate::schema::net::LIMIT_MAX_BUFFERED_PER_FLOW,
                self.max_buffered_per_flow,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_DATAGRAM_PAYLOAD,
                self.max_datagram_payload,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_DATAGRAM_QUEUE,
                self.max_datagram_queue,
            ),
            limit_u64(
                crate::schema::net::LIMIT_CONNECT_TIMEOUT_NS,
                self.connect_timeout_ns,
            ),
            limit_u32(
                crate::schema::net::LIMIT_MAX_MUTATION_REPLAYS,
                self.max_mutation_replays,
            ),
        ]))
    }

    pub fn from_extensions(extensions: &Extensions) -> Result<Self> {
        let known = [
            crate::schema::net::LIMIT_MAX_HOST_BYTES as u16,
            crate::schema::net::LIMIT_MAX_LOCAL_ADDRESS_BYTES as u16,
            crate::schema::net::LIMIT_MAX_PIPE_NAME_BYTES as u16,
            crate::schema::net::LIMIT_MAX_ALPN_PROTOCOLS as u16,
            crate::schema::net::LIMIT_MAX_ALPN_BYTES as u16,
            crate::schema::net::LIMIT_MAX_EARLY_DATA_BYTES as u16,
            crate::schema::net::LIMIT_MAX_FLOWS_PER_SESSION as u16,
            crate::schema::net::LIMIT_MAX_PENDING_OPENS as u16,
            crate::schema::net::LIMIT_MAX_BUFFERED_PER_FLOW as u16,
            crate::schema::net::LIMIT_MAX_DATAGRAM_PAYLOAD as u16,
            crate::schema::net::LIMIT_MAX_DATAGRAM_QUEUE as u16,
            crate::schema::net::LIMIT_CONNECT_TIMEOUT_NS as u16,
            crate::schema::net::LIMIT_MAX_MUTATION_REPLAYS as u16,
        ];
        reject_unknown_required(extensions, &known)?;
        let value = Self {
            max_host_bytes: read_limit_u32(extensions, crate::schema::net::LIMIT_MAX_HOST_BYTES)?,
            max_local_address_bytes: read_limit_u32(
                extensions,
                crate::schema::net::LIMIT_MAX_LOCAL_ADDRESS_BYTES,
            )?,
            max_pipe_name_bytes: read_limit_u32(
                extensions,
                crate::schema::net::LIMIT_MAX_PIPE_NAME_BYTES,
            )?,
            max_alpn_protocols: read_limit_u32(
                extensions,
                crate::schema::net::LIMIT_MAX_ALPN_PROTOCOLS,
            )?,
            max_alpn_bytes: read_limit_u32(extensions, crate::schema::net::LIMIT_MAX_ALPN_BYTES)?,
            max_early_data_bytes: read_limit_u32(
                extensions,
                crate::schema::net::LIMIT_MAX_EARLY_DATA_BYTES,
            )?,
            max_flows_per_session: read_limit_u32(
                extensions,
                crate::schema::net::LIMIT_MAX_FLOWS_PER_SESSION,
            )?,
            max_pending_opens: read_limit_u32(
                extensions,
                crate::schema::net::LIMIT_MAX_PENDING_OPENS,
            )?,
            max_buffered_per_flow: read_limit_u64(
                extensions,
                crate::schema::net::LIMIT_MAX_BUFFERED_PER_FLOW,
            )?,
            max_datagram_payload: read_limit_u32(
                extensions,
                crate::schema::net::LIMIT_MAX_DATAGRAM_PAYLOAD,
            )?,
            max_datagram_queue: read_limit_u32(
                extensions,
                crate::schema::net::LIMIT_MAX_DATAGRAM_QUEUE,
            )?,
            connect_timeout_ns: read_limit_u64(
                extensions,
                crate::schema::net::LIMIT_CONNECT_TIMEOUT_NS,
            )?,
            max_mutation_replays: read_limit_u32(
                extensions,
                crate::schema::net::LIMIT_MAX_MUTATION_REPLAYS,
            )?,
        };
        value.validate()?;
        Ok(value)
    }
}

fn validate_flow_descriptor(
    descriptor: &Descriptor,
    mode: FlowMode,
    direction: FlowDirection,
) -> Result<()> {
    descriptor.validate()?;
    let transfer_mode = match mode {
        FlowMode::Byte => TransferMode::Byte,
        FlowMode::Message => TransferMode::Message,
        FlowMode::Datagram => return Err(Error::Invalid("Transfer for Net datagram endpoint")),
    };
    if descriptor.mode != transfer_mode
        || descriptor.direction != direction.transfer_direction()
        || descriptor.content_family != crate::family::NET
        || descriptor.content_kind != crate::schema::net::FLOW_CONTENT_KIND as u16
        || descriptor.content_version != VERSION
        || !descriptor.sensitive_content()?
        || descriptor.max_chunk_bytes as u64 > MAX_BUFFERED_PER_FLOW
    {
        return Err(Error::Invalid("Net flow Transfer descriptor"));
    }
    Ok(())
}

fn validate_handle(value: u64) -> Result<()> {
    if value == 0 {
        Err(Error::Invalid("zero Net flow handle"))
    } else {
        Ok(())
    }
}

fn validate_operation_id(value: &[u8; 16]) -> Result<()> {
    if *value == [0; 16] {
        Err(Error::Invalid("zero Net operation ID"))
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
        return Err(Error::Invalid("unknown required Net extension"));
    }
    Ok(())
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
        .ok_or(Error::Invalid("missing Net family limit"))?;
    Ok(u32::from_le_bytes(
        extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("Net family limit length"))?,
    ))
}

fn read_limit_u64(extensions: &Extensions, tag: u64) -> Result<u64> {
    let extension = extensions
        .0
        .iter()
        .find(|extension| extension.tag == tag as u16)
        .ok_or(Error::Invalid("missing Net family limit"))?;
    Ok(u64::from_le_bytes(
        extension
            .value
            .as_slice()
            .try_into()
            .map_err(|_| Error::Invalid("Net family limit length"))?,
    ))
}

fn limit(name: &'static str, actual: usize, maximum: usize) -> Error {
    Error::LimitExceeded {
        limit: name,
        actual: actual as u64,
        maximum: maximum as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sensitive_extensions() -> Extensions {
        Extensions(vec![Extension {
            tag: crate::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
            required: true,
            value: Vec::new(),
        }])
    }

    fn descriptor(mode: TransferMode, max_item_bytes: u64) -> Descriptor {
        Descriptor {
            transfer_id: 2,
            mode,
            direction: TransferDirection::BIDIRECTIONAL,
            receiver_send_credit: 65_536,
            sender_send_credit: 65_536,
            max_item_bytes,
            max_chunk_bytes: 16_384,
            content_family: crate::family::NET,
            content_kind: crate::schema::net::FLOW_CONTENT_KIND as u16,
            content_version: VERSION,
            extensions: sensitive_extensions(),
        }
    }

    fn round_trip<T>(value: T)
    where
        T: Encode + Decode + PartialEq + std::fmt::Debug,
    {
        let encoded = value.encode().unwrap();
        assert_eq!(T::decode(&encoded).unwrap(), value);
        for end in 0..encoded.len() {
            assert!(T::decode(&encoded[..end]).is_err(), "accepted prefix {end}");
        }
    }

    #[test]
    fn addresses_and_tls_round_trip() {
        for address in [
            Address::Tcp {
                host: "db.internal".into(),
                port: 5432,
            },
            Address::Udp {
                host: "2001:db8::1".into(),
                port: 53,
            },
            Address::UnixStream(UnixName {
                kind: UnixNameKind::Filesystem,
                name: b"/run/service.sock".to_vec(),
            }),
            Address::UnixDatagram(UnixName {
                kind: UnixNameKind::Abstract,
                name: b"yas-private".to_vec(),
            }),
            Address::UnixSeqpacket(UnixName {
                kind: UnixNameKind::Filesystem,
                name: b"/run/seq.sock".to_vec(),
            }),
            Address::WindowsPipe {
                requested_mode: PipeMode::Message,
                name: r"\\server\pipe\yas".into(),
            },
        ] {
            round_trip(address);
        }
        round_trip(TlsOptions {
            verification: TlsVerification::Strict,
            sni: "db.internal".into(),
            alpn: vec![b"h2".to_vec(), b"http/1.1".to_vec()],
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn hard_limits_round_trip_and_validate() {
        let extensions = Limits::HARD.to_extensions().unwrap();
        assert_eq!(Limits::from_extensions(&extensions).unwrap(), Limits::HARD);

        let mut early_data_disabled = Limits::HARD;
        early_data_disabled.max_early_data_bytes = 0;
        assert_eq!(
            Limits::from_extensions(&early_data_disabled.to_extensions().unwrap()).unwrap(),
            early_data_disabled
        );

        let mut invalid = Limits::HARD;
        invalid.max_flows_per_session = 0;
        assert!(invalid.validate().is_err());

        let mut missing = extensions.clone();
        missing.0.pop();
        assert!(Limits::from_extensions(&missing).is_err());
    }

    #[test]
    fn reliable_and_datagram_opens_round_trip() {
        round_trip(Open {
            operation_id: [1; 16],
            address: Address::Tcp {
                host: "localhost".into(),
                port: 443,
            },
            delivery_preference: DeliveryPreference::NotApplicable,
            drop_policy: DropPolicy::NotApplicable,
            initial_receive_credit: 1 << 20,
            early_data: b"GET / HTTP/1.0\r\n\r\n".to_vec(),
            tls_options: Some(TlsOptions {
                verification: TlsVerification::Strict,
                sni: String::new(),
                alpn: vec![b"http/1.1".to_vec()],
                extensions: Extensions::default(),
            }),
            extensions: Extensions::default(),
        });
        round_trip(Open {
            operation_id: [2; 16],
            address: Address::Udp {
                host: "127.0.0.1".into(),
                port: 5353,
            },
            delivery_preference: DeliveryPreference::PreferNative,
            drop_policy: DropPolicy::Oldest,
            initial_receive_credit: 0,
            early_data: Vec::new(),
            tls_options: None,
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn endpoints_validate_transfer_semantics() {
        round_trip(Endpoint {
            flow_handle: 7,
            mode: FlowMode::Byte,
            direction: FlowDirection::DUPLEX,
            selected_delivery: DatagramDelivery::NotApplicable,
            max_datagram_payload: 0,
            server_instance_limit: 0,
            max_message_bytes: 0,
            local_address: Some(Address::Tcp {
                host: "127.0.0.1".into(),
                port: 49152,
            }),
            peer_address: Address::Tcp {
                host: "127.0.0.1".into(),
                port: 80,
            },
            negotiated_alpn: b"http/1.1".to_vec(),
            descriptor: Some(descriptor(TransferMode::Byte, 0)),
            extensions: Extensions::default(),
        });
        round_trip(Endpoint {
            flow_handle: 8,
            mode: FlowMode::Datagram,
            direction: FlowDirection::DUPLEX,
            selected_delivery: DatagramDelivery::ReliableTunnel,
            max_datagram_payload: 1200,
            server_instance_limit: 0,
            max_message_bytes: 0,
            local_address: None,
            peer_address: Address::Udp {
                host: "127.0.0.1".into(),
                port: 53,
            },
            negotiated_alpn: Vec::new(),
            descriptor: None,
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn datagrams_and_stats_are_bounded() {
        let datagram = Datagram {
            flow_handle: 9,
            sequence: 42,
            payload: vec![0, 1, 2, 3],
        };
        assert_eq!(
            Datagram::decode(&datagram.encode().unwrap()).unwrap(),
            datagram
        );
        assert!(
            Datagram {
                payload: vec![0; MAX_DATAGRAM_PAYLOAD + 1],
                ..datagram
            }
            .encode()
            .is_err()
        );
        round_trip(DatagramStats {
            flow_handle: 9,
            revision: 2,
            final_stats: true,
            client_to_peer_delivered: 10,
            peer_to_client_delivered: 11,
            client_oversized_drops: 1,
            peer_oversized_drops: 2,
            client_congestive_drops: 3,
            peer_congestive_drops: 4,
            transport_errors: 5,
            extensions: Extensions::default(),
        });
    }

    #[test]
    fn invalid_cross_mode_options_are_rejected() {
        let datagram = Open {
            operation_id: [3; 16],
            address: Address::Udp {
                host: "localhost".into(),
                port: 53,
            },
            delivery_preference: DeliveryPreference::PreferNative,
            drop_policy: DropPolicy::Latest,
            initial_receive_credit: 1,
            early_data: Vec::new(),
            tls_options: None,
            extensions: Extensions::default(),
        };
        assert!(datagram.encode().is_err());
        let mut bad_descriptor = descriptor(TransferMode::Byte, 0);
        bad_descriptor.content_family = crate::family::PROCESS;
        let endpoint = Endpoint {
            flow_handle: 1,
            mode: FlowMode::Byte,
            direction: FlowDirection::DUPLEX,
            selected_delivery: DatagramDelivery::NotApplicable,
            max_datagram_payload: 0,
            server_instance_limit: 0,
            max_message_bytes: 0,
            local_address: None,
            peer_address: Address::Tcp {
                host: "localhost".into(),
                port: 80,
            },
            negotiated_alpn: Vec::new(),
            descriptor: Some(bad_descriptor),
            extensions: Extensions::default(),
        };
        assert!(endpoint.encode().is_err());
    }
}
