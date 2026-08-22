use crate::codec::{
    Decode, Decoder, Encode, Error, Extension, Extensions, Result, put_len_u32, put_string_u16,
    put_string_u32, put_u16, put_u32, put_u64,
};
use crate::frame::{
    Class, HARD_MAX_BUFFERED, HARD_MAX_DATAGRAM, HARD_MAX_DECODED_FRAME, HARD_MAX_WIRE_FRAME,
};
use crate::prelude::*;

macro_rules! fixed_u64_codec {
    ($type:ident, $field:ident) => {
        impl Encode for $type {
            fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
                put_u64(out, self.$field);
                Ok(())
            }
        }

        impl Decode for $type {
            fn decode(input: &[u8]) -> Result<Self> {
                let mut decoder = Decoder::new(input);
                let value = Self {
                    $field: decoder.u64()?,
                };
                decoder.finish()?;
                Ok(value)
            }
        }
    };
}

fn bounded_count(
    count: usize,
    remaining: usize,
    minimum_item_bytes: usize,
    context: &'static str,
) -> Result<usize> {
    if count > remaining / minimum_item_bytes {
        return Err(Error::Invalid(context));
    }
    Ok(count)
}

fn validate_known_family_limits(family_id: u16, version: u16, limits: &Extensions) -> Result<()> {
    macro_rules! validate_limits {
        ($family:ident, $module:ident) => {
            if family_id == crate::family::$family && version == crate::$module::VERSION {
                crate::$module::Limits::from_extensions(limits)?;
                return Ok(());
            }
        };
    }

    validate_limits!(RELAY, relay);
    validate_limits!(FONT, font);
    validate_limits!(TERMINAL, terminal);
    validate_limits!(CLIENT, client);
    validate_limits!(SURFACE, surface);
    validate_limits!(SELECTION, selection);
    validate_limits!(DESKTOP, desktop);
    validate_limits!(MEDIA, media);
    validate_limits!(FS, fs);
    validate_limits!(GIT, git);
    validate_limits!(LSP, lsp);
    validate_limits!(KV, kv);
    validate_limits!(PROCESS, process);
    validate_limits!(NET, net);
    validate_limits!(CHANNEL, channel);
    validate_limits!(EXTENSION, extension);
    validate_limits!(EVENTS, events);
    validate_limits!(ENV, env);
    Ok(())
}

pub const VERSION: u16 = crate::schema::core::VERSION;

pub mod request_kind {
    pub use crate::schema::core::request::*;
}

pub mod event_kind {
    pub use crate::schema::core::event::*;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReceiveLimits {
    pub max_frame: u32,
    pub max_decoded: u32,
    pub max_datagram: u32,
    pub max_buffered: u64,
}

impl ReceiveLimits {
    pub const fn recommended(max_datagram: u32) -> Self {
        Self {
            max_frame: crate::schema::transport::RECOMMENDED_WIRE_FRAME,
            max_decoded: crate::schema::transport::RECOMMENDED_DECODED_FRAME,
            max_datagram,
            max_buffered: crate::schema::transport::RECOMMENDED_BUFFERED,
        }
    }

    pub fn validate(&self) -> Result<()> {
        if self.max_frame < crate::schema::transport::CORRELATED_HEADER_BYTES as u32
            || self.max_frame > HARD_MAX_WIRE_FRAME
        {
            return Err(Error::Invalid("HELLO receive_max_frame"));
        }
        if self.max_decoded < self.max_frame || self.max_decoded > HARD_MAX_DECODED_FRAME {
            return Err(Error::Invalid("HELLO receive_max_decoded"));
        }
        if (self.max_datagram != 0
            && self.max_datagram < crate::schema::transport::EVENT_HEADER_BYTES as u32)
            || self.max_datagram > HARD_MAX_DATAGRAM
        {
            return Err(Error::Invalid("HELLO receive_max_datagram"));
        }
        if self.max_buffered == 0 || self.max_buffered > HARD_MAX_BUFFERED {
            return Err(Error::Invalid("HELLO receive_max_buffered"));
        }
        Ok(())
    }

    /// Validate a server-to-client SESSION_UPDATE replacement. YAS v1 has no
    /// acknowledgement barrier for frames already in flight, so wire and
    /// decoded frame limits cannot shrink within a session. Datagram and
    /// aggregate-buffer limits have explicit grandfathering/drop semantics.
    pub fn validate_update_from(&self, previous: &Self) -> Result<()> {
        self.validate()?;
        if self.max_frame < previous.max_frame || self.max_decoded < previous.max_decoded {
            return Err(Error::Invalid("SESSION_UPDATE frame limit reduction"));
        }
        Ok(())
    }

    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.max_frame);
        put_u32(out, self.max_decoded);
        put_u32(out, self.max_datagram);
        put_u64(out, self.max_buffered);
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let value = Self {
            max_frame: decoder.u32()?,
            max_decoded: decoder.u32()?,
            max_datagram: decoder.u32()?,
            max_buffered: decoder.u64()?,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyOffer {
    pub family_id: u16,
    pub versions: Vec<u16>,
    pub required: bool,
}

impl FamilyOffer {
    fn validate(&self) -> Result<()> {
        if self.family_id == crate::family::CORE
            || self.versions.is_empty()
            || self.versions.contains(&0)
        {
            return Err(Error::Invalid("HELLO family offer"));
        }
        if self.versions.len() > usize::from(u8::MAX)
            || self.versions.windows(2).any(|pair| pair[0] <= pair[1])
        {
            return Err(Error::Invalid("HELLO family version order"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientHello {
    pub min_minor: u16,
    pub max_minor: u16,
    pub receive: ReceiveLimits,
    pub client_instance: [u8; 16],
    pub client_name: String,
    pub client_release: String,
    pub families: Vec<FamilyOffer>,
    pub codecs: Vec<u16>,
    pub extensions: Extensions,
}

impl ClientHello {
    pub fn validate(&self) -> Result<()> {
        if self.min_minor > self.max_minor {
            return Err(Error::Invalid("HELLO Core minor range"));
        }
        self.receive.validate()?;
        if self.families.len() > usize::from(u16::MAX)
            || self
                .families
                .windows(2)
                .any(|pair| pair[0].family_id >= pair[1].family_id)
        {
            return Err(Error::Invalid("HELLO family order"));
        }
        for family in &self.families {
            family.validate()?;
        }
        if self.codecs.len() > usize::from(u8::MAX) {
            return Err(Error::LengthOverflow);
        }
        if self.codecs.contains(&0) || self.codecs.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(Error::Invalid("HELLO codec order"));
        }
        self.extensions.validate()?;
        for extension in &self.extensions.0 {
            match u64::from(extension.tag) {
                crate::schema::core::CLIENT_HELLO_IDLE_TIMEOUT_EXTENSION => {
                    if extension.value.len() != 8 {
                        return Err(Error::Invalid("HELLO idle timeout extension"));
                    }
                }
                crate::schema::core::CLIENT_HELLO_INITIAL_WATCHES_EXTENSION => {
                    InitialWatches::decode(&extension.value)?;
                }
                crate::schema::core::CLIENT_HELLO_READ_ONLY_SESSION_EXTENSION
                    if !extension.required || !extension.value.is_empty() =>
                {
                    return Err(Error::Invalid("HELLO read-only session extension"));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl Encode for ClientHello {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.min_minor);
        put_u16(out, self.max_minor);
        self.receive.encode_to(out)?;
        out.extend_from_slice(&self.client_instance);
        put_string_u16(out, &self.client_name)?;
        put_string_u16(out, &self.client_release)?;
        put_u16(
            out,
            u16::try_from(self.families.len()).map_err(|_| Error::LengthOverflow)?,
        );
        for family in &self.families {
            put_u16(out, family.family_id);
            out.push(u8::try_from(family.versions.len()).map_err(|_| Error::LengthOverflow)?);
            out.push(u8::from(family.required));
            for version in &family.versions {
                put_u16(out, *version);
            }
        }
        out.push(u8::try_from(self.codecs.len()).map_err(|_| Error::LengthOverflow)?);
        for codec in &self.codecs {
            put_u16(out, *codec);
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for ClientHello {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let min_minor = decoder.u16()?;
        let max_minor = decoder.u16()?;
        let receive = ReceiveLimits::decode_from(&mut decoder)?;
        let client_instance = decoder.array_16()?;
        let client_name = decoder.string_u16()?;
        let client_release = decoder.string_u16()?;
        let family_count = decoder.u16()?;
        let family_count = bounded_count(
            usize::from(family_count),
            decoder.remaining(),
            4,
            "HELLO family count",
        )?;
        let mut families = Vec::with_capacity(family_count);
        for _ in 0..family_count {
            let family_id = decoder.u16()?;
            let version_count = decoder.u8()?;
            let offer_flags = decoder.u8()?;
            if offer_flags & !1 != 0 {
                return Err(Error::Invalid("HELLO family offer flags"));
            }
            let version_count = bounded_count(
                usize::from(version_count),
                decoder.remaining(),
                2,
                "HELLO family version count",
            )?;
            let mut versions = Vec::with_capacity(version_count);
            for _ in 0..version_count {
                versions.push(decoder.u16()?);
            }
            families.push(FamilyOffer {
                family_id,
                versions,
                required: offer_flags & 1 != 0,
            });
        }
        let codec_count = decoder.u8()?;
        let codec_count = bounded_count(
            usize::from(codec_count),
            decoder.remaining(),
            2,
            "HELLO codec count",
        )?;
        let mut codecs = Vec::with_capacity(codec_count);
        for _ in 0..codec_count {
            codecs.push(decoder.u16()?);
        }
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let value = Self {
            min_minor,
            max_minor,
            receive,
            client_instance,
            client_name,
            client_release,
            families,
            codecs,
            extensions,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InitialWatch {
    pub family_id: u16,
    pub family_version: u16,
    pub watch_payload: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InitialWatches(pub Vec<InitialWatch>);

impl InitialWatches {
    pub fn extension(&self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::core::CLIENT_HELLO_INITIAL_WATCHES_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for InitialWatches {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        put_u16(
            out,
            u16::try_from(self.0.len()).map_err(|_| Error::LengthOverflow)?,
        );
        for watch in &self.0 {
            if watch.family_id == crate::family::CORE || watch.family_version == 0 {
                return Err(Error::Invalid("initial WATCH family"));
            }
            put_u16(out, watch.family_id);
            put_u16(out, watch.family_version);
            put_len_u32(out, watch.watch_payload.len())?;
            out.extend_from_slice(&watch.watch_payload);
        }
        Ok(())
    }
}

impl Decode for InitialWatches {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = decoder.u16()?;
        let count = bounded_count(
            usize::from(count),
            decoder.remaining(),
            8,
            "initial WATCH count",
        )?;
        let mut watches = Vec::with_capacity(count);
        for _ in 0..count {
            let watch = InitialWatch {
                family_id: decoder.u16()?,
                family_version: decoder.u16()?,
                watch_payload: decoder.len_bytes_u32()?.to_vec(),
            };
            if watch.family_id == crate::family::CORE || watch.family_version == 0 {
                return Err(Error::Invalid("initial WATCH family"));
            }
            watches.push(watch);
        }
        decoder.finish()?;
        Ok(Self(watches))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InitialWatchResults(pub Vec<ResultPrefix>);

impl InitialWatchResults {
    pub fn extension(&self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::core::SERVER_HELLO_INITIAL_WATCH_RESULTS_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for InitialWatchResults {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        put_u16(
            out,
            u16::try_from(self.0.len()).map_err(|_| Error::LengthOverflow)?,
        );
        for result in &self.0 {
            let encoded = result.encode()?;
            put_len_u32(out, encoded.len())?;
            out.extend_from_slice(&encoded);
        }
        Ok(())
    }
}

impl Decode for InitialWatchResults {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = decoder.u16()?;
        let count = bounded_count(
            usize::from(count),
            decoder.remaining(),
            4,
            "initial WATCH Result count",
        )?;
        let mut results = Vec::with_capacity(count);
        for _ in 0..count {
            results.push(ResultPrefix::decode(decoder.len_bytes_u32()?)?);
        }
        decoder.finish()?;
        Ok(Self(results))
    }
}

/// Generic compression codecs accepted by the server for both directions.
/// This is HELLO Result extension tag 2; absence means no codec was selected.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NegotiatedCodecs(pub Vec<u16>);

impl NegotiatedCodecs {
    pub fn validate(&self) -> Result<()> {
        if self.0.len() > usize::from(u8::MAX) {
            return Err(Error::LengthOverflow);
        }
        if self.0.contains(&0) || self.0.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(Error::Invalid("negotiated codec order"));
        }
        Ok(())
    }

    pub fn validate_offered_by(&self, hello: &ClientHello) -> Result<()> {
        self.validate()?;
        hello.validate()?;
        if self
            .0
            .iter()
            .any(|codec| hello.codecs.binary_search(codec).is_err())
        {
            return Err(Error::Invalid("server selected unoffered codec"));
        }
        Ok(())
    }

    pub fn extension(&self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::core::SERVER_HELLO_NEGOTIATED_CODECS_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for NegotiatedCodecs {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(u8::try_from(self.0.len()).map_err(|_| Error::LengthOverflow)?);
        for codec in &self.0 {
            put_u16(out, *codec);
        }
        Ok(())
    }
}

impl Decode for NegotiatedCodecs {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let count = decoder.u8()?;
        let count = bounded_count(
            usize::from(count),
            decoder.remaining(),
            2,
            "negotiated codec count",
        )?;
        let mut codecs = Vec::with_capacity(count);
        for _ in 0..count {
            codecs.push(decoder.u16()?);
        }
        decoder.finish()?;
        let value = Self(codecs);
        value.validate()?;
        Ok(value)
    }
}

/// What a peer is running on: the operating system, the CPU architecture, and
/// the platform flavour that distinguishes two builds for the same pair.
///
/// Rust's own names, because they are the names every artifact in this project
/// is already labelled with: `linux`/`macos`/`windows`, `x86_64`/`aarch64`,
/// and an environment of `musl`, `gnu` or `msvc` (empty where the target does
/// not have one). A client picking an extension build, or a person reading a
/// client list, wants exactly this triple and no more.
///
/// Rides both HELLOs as an optional extension, so a peer that does not send it
/// is simply a peer that did not say — never a failed handshake.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Platform {
    pub os: String,
    pub arch: String,
    pub env: String,
}

impl Platform {
    /// This build's own triple, from the compiler's view of its target.
    pub fn current() -> Self {
        Self {
            os: current_os().to_owned(),
            arch: current_arch().to_owned(),
            env: current_env().to_owned(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        // A name, not a sentence: these are identifiers a build system prints,
        // and anything longer is someone using the field for something else.
        for field in [&self.os, &self.arch, &self.env] {
            if field.len() > MAX_PLATFORM_FIELD_BYTES {
                return Err(Error::Invalid("HELLO platform field length"));
            }
        }
        Ok(())
    }

    pub fn extension(&self, tag: u16) -> Result<Extension> {
        Ok(Extension {
            tag,
            required: false,
            value: self.encode()?,
        })
    }

    /// The platform a peer declared, or `None` when it declared none.
    pub fn from_extensions(extensions: &Extensions, tag: u16) -> Option<Self> {
        extensions
            .0
            .iter()
            .find(|extension| extension.tag == tag)
            .and_then(|extension| Self::decode(&extension.value).ok())
    }
}

impl core::fmt::Display for Platform {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}/{}", self.os, self.arch)?;
        if !self.env.is_empty() {
            write!(f, "/{}", self.env)?;
        }
        Ok(())
    }
}

const MAX_PLATFORM_FIELD_BYTES: usize = 64;

/// Rust's `target_os`, spelled out rather than read from `std::env::consts`,
/// which this `no_std` crate cannot reach. An unlisted target says nothing
/// rather than guessing.
const fn current_os() -> &'static str {
    if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "freebsd") {
        "freebsd"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_family = "wasm") {
        "wasm"
    } else {
        ""
    }
}

const fn current_arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "arm") {
        "arm"
    } else if cfg!(target_arch = "riscv64") {
        "riscv64"
    } else if cfg!(target_arch = "wasm32") {
        "wasm32"
    } else if cfg!(target_arch = "x86") {
        "x86"
    } else {
        ""
    }
}

const fn current_env() -> &'static str {
    if cfg!(target_env = "musl") {
        "musl"
    } else if cfg!(target_env = "gnu") {
        "gnu"
    } else if cfg!(target_env = "msvc") {
        "msvc"
    } else {
        ""
    }
}

impl Encode for Platform {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        for field in [&self.os, &self.arch, &self.env] {
            put_string_u16(out, field)?;
        }
        Ok(())
    }
}

impl Decode for Platform {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let mut field = || -> Result<String> {
            let value = decoder.string_u16()?;
            if value.len() > MAX_PLATFORM_FIELD_BYTES {
                return Err(Error::Invalid("HELLO platform field length"));
            }
            Ok(value)
        };
        let value = Self {
            os: field()?,
            arch: field()?,
            env: field()?,
        };
        decoder.finish()?;
        Ok(value)
    }
}

fn validate_result_or_event_extensions(
    extensions: &Extensions,
    context: &'static str,
) -> Result<()> {
    extensions.validate()?;
    if extensions.0.iter().any(|extension| extension.required) {
        return Err(Error::Invalid(context));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RuntimeState {
    Available = crate::schema::core::RUNTIME_AVAILABLE as u8,
    Degraded = crate::schema::core::RUNTIME_DEGRADED as u8,
    Unavailable = crate::schema::core::RUNTIME_UNAVAILABLE as u8,
}

impl TryFrom<u8> for RuntimeState {
    type Error = Error;

    fn try_from(value: u8) -> Result<Self> {
        match value {
            value if value == crate::schema::core::RUNTIME_AVAILABLE as u8 => Ok(Self::Available),
            value if value == crate::schema::core::RUNTIME_DEGRADED as u8 => Ok(Self::Degraded),
            value if value == crate::schema::core::RUNTIME_UNAVAILABLE as u8 => {
                Ok(Self::Unavailable)
            }
            _ => Err(Error::Invalid("family runtime state")),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Operation {
    pub server_accepts: bool,
    pub server_sends: bool,
    pub class: Class,
    pub kind: u16,
}

impl Operation {
    pub fn from_schema(metadata: &crate::schema::OperationMetadata) -> Result<Self> {
        let class = match metadata.class {
            value if value == crate::schema::transport::class::EVENT => Class::Event,
            value if value == crate::schema::transport::class::REQUEST => Class::Request,
            _ => return Err(Error::Invalid("schema operation class")),
        };
        let (server_accepts, server_sends) = match metadata.direction {
            value if value == crate::schema::transport::direction::CLIENT_TO_SERVER => {
                (true, false)
            }
            value if value == crate::schema::transport::direction::SERVER_TO_CLIENT => {
                (false, true)
            }
            value if value == crate::schema::transport::direction::BIDIRECTIONAL => (true, true),
            _ => return Err(Error::Invalid("schema operation direction")),
        };
        Ok(Self {
            server_accepts,
            server_sends,
            class,
            kind: metadata.kind,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if (!self.server_accepts && !self.server_sends) || self.class == Class::Result {
            return Err(Error::Invalid("family operation class or direction"));
        }
        Ok(())
    }

    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.push(
            (u8::from(self.server_accepts) * crate::schema::core::DIRECTION_ACCEPTS as u8)
                | (u8::from(self.server_sends) * crate::schema::core::DIRECTION_SENDS as u8),
        );
        out.push(self.class as u8);
        put_u16(out, self.kind);
        Ok(())
    }

    fn decode_from(decoder: &mut Decoder<'_>) -> Result<Self> {
        let direction = decoder.u8()?;
        let known =
            (crate::schema::core::DIRECTION_ACCEPTS | crate::schema::core::DIRECTION_SENDS) as u8;
        if direction == 0 || direction & !known != 0 {
            return Err(Error::Invalid("family operation direction"));
        }
        let class = match decoder.u8()? {
            0 => Class::Event,
            1 => Class::Request,
            _ => return Err(Error::Invalid("family operation class")),
        };
        let value = Self {
            server_accepts: direction & crate::schema::core::DIRECTION_ACCEPTS as u8 != 0,
            server_sends: direction & crate::schema::core::DIRECTION_SENDS as u8 != 0,
            class,
            kind: decoder.u16()?,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyDescriptor {
    pub family_id: u16,
    pub version: u16,
    pub runtime_state: RuntimeState,
    pub operations: Vec<Operation>,
    pub limits: Extensions,
}

impl FamilyDescriptor {
    pub fn validate(&self) -> Result<()> {
        if self.version == 0 || self.operations.len() > usize::from(u16::MAX) {
            return Err(Error::Invalid(
                "family descriptor version or operation count",
            ));
        }
        for (index, operation) in self.operations.iter().enumerate() {
            if self.operations[..index].iter().any(|previous| {
                previous.class == operation.class && previous.kind == operation.kind
            }) {
                return Err(Error::Invalid("duplicate family operation"));
            }
            operation.validate()?;
            if let Some(family) = crate::schema::FAMILIES
                .iter()
                .find(|family| family.id == self.family_id && family.version == self.version)
                && let Some(metadata) = family.operations.iter().find(|metadata| {
                    metadata.class == operation.class as u8 && metadata.kind == operation.kind
                })
            {
                let allowed = Operation::from_schema(metadata)?;
                if operation.server_accepts && !allowed.server_accepts
                    || operation.server_sends && !allowed.server_sends
                {
                    return Err(Error::Invalid("family operation direction exceeds schema"));
                }
            }
        }
        self.limits.validate()?;
        if self.limits.0.iter().any(|extension| extension.required) {
            return Err(Error::Invalid("required family limit extension"));
        }
        if let Some(family) = crate::schema::family_metadata(self.family_id, self.version) {
            for policy in family.limits {
                let extension = self
                    .limits
                    .0
                    .iter()
                    .find(|extension| extension.tag == policy.tag);
                let Some(extension) = extension else {
                    if policy.required {
                        return Err(Error::Invalid("missing required family limit"));
                    }
                    continue;
                };
                if extension.value.len() != policy.value_type.width() {
                    return Err(Error::Invalid("family limit length"));
                }
                let value = match policy.value_type {
                    crate::schema::LimitValueType::U32 => u64::from(u32::from_le_bytes(
                        extension
                            .value
                            .as_slice()
                            .try_into()
                            .map_err(|_| Error::Invalid("family limit length"))?,
                    )),
                    crate::schema::LimitValueType::U64 => u64::from_le_bytes(
                        extension
                            .value
                            .as_slice()
                            .try_into()
                            .map_err(|_| Error::Invalid("family limit length"))?,
                    ),
                };
                if value < policy.hard_min {
                    return Err(Error::Invalid("family limit below hard minimum"));
                }
                if value > policy.hard_max {
                    return Err(Error::LimitExceeded {
                        limit: "family limit",
                        actual: value,
                        maximum: policy.hard_max,
                    });
                }
            }
            validate_known_family_limits(self.family_id, self.version, &self.limits)?;
        }
        Ok(())
    }

    pub fn operation(&self, class: Class, kind: u16) -> Option<&Operation> {
        self.operations
            .iter()
            .find(|operation| operation.class == class && operation.kind == kind)
    }

    fn encode_record(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        let mut body = Vec::new();
        put_u16(&mut body, self.family_id);
        put_u16(&mut body, self.version);
        body.push(self.runtime_state as u8);
        body.push(0);
        put_u16(&mut body, self.operations.len() as u16);
        for operation in &self.operations {
            operation.encode_to(&mut body)?;
        }
        let mut limits = Vec::new();
        self.limits.encode_entries(&mut limits)?;
        put_len_u32(&mut body, limits.len())?;
        body.extend_from_slice(&limits);
        put_len_u32(out, body.len())?;
        out.extend_from_slice(&body);
        Ok(())
    }

    fn decode_record(decoder: &mut Decoder<'_>) -> Result<Self> {
        let mut record = Decoder::new(decoder.len_bytes_u32()?);
        let family_id = record.u16()?;
        let version = record.u16()?;
        let runtime_state = RuntimeState::try_from(record.u8()?)?;
        if record.u8()? != 0 {
            return Err(Error::Invalid("family descriptor reserved byte"));
        }
        let operation_count = record.u16()?;
        let operation_count = bounded_count(
            usize::from(operation_count),
            record.remaining(),
            4,
            "family operation count",
        )?;
        let mut operations = Vec::with_capacity(operation_count);
        for _ in 0..operation_count {
            operations.push(Operation::decode_from(&mut record)?);
        }
        let limits = Extensions::decode_entries(record.len_bytes_u32()?)?;
        record.finish()?;
        let value = Self {
            family_id,
            version,
            runtime_state,
            operations,
            limits,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerHello {
    pub minor: u16,
    pub boot_id: [u8; 16],
    pub session_id: [u8; 16],
    pub receive: ReceiveLimits,
    pub server_monotonic_ns: u64,
    pub catalog_revision: u64,
    pub server_name: String,
    pub server_release: String,
    pub families: Vec<FamilyDescriptor>,
    pub extensions: Extensions,
}

impl ServerHello {
    pub fn validate(&self) -> Result<()> {
        self.receive.validate()?;
        if self.families.is_empty()
            || self.families[0].family_id != crate::family::CORE
            || self.families[0].version != VERSION
            || self.families.len() > usize::from(u16::MAX)
            || self
                .families
                .windows(2)
                .any(|pair| pair[0].family_id >= pair[1].family_id)
        {
            return Err(Error::Invalid("HELLO selected family order"));
        }
        for family in &self.families {
            family.validate()?;
        }
        validate_family_dependencies(&self.families, "HELLO missing family dependency")?;
        validate_result_or_event_extensions(&self.extensions, "required HELLO Result extension")?;
        for extension in &self.extensions.0 {
            match u64::from(extension.tag) {
                crate::schema::core::SERVER_HELLO_INITIAL_WATCH_RESULTS_EXTENSION => {
                    InitialWatchResults::decode(&extension.value)?;
                }
                crate::schema::core::SERVER_HELLO_NEGOTIATED_CODECS_EXTENSION => {
                    NegotiatedCodecs::decode(&extension.value)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    pub fn negotiated_codecs(&self) -> Result<NegotiatedCodecs> {
        match self.extensions.0.iter().find(|extension| {
            u64::from(extension.tag)
                == crate::schema::core::SERVER_HELLO_NEGOTIATED_CODECS_EXTENSION
        }) {
            Some(extension) => NegotiatedCodecs::decode(&extension.value),
            None => Ok(NegotiatedCodecs::default()),
        }
    }

    pub fn validate_for_client(&self, client: &ClientHello) -> Result<()> {
        self.validate()?;
        if self.minor < client.min_minor || self.minor > client.max_minor {
            return Err(Error::Invalid("server selected unoffered Core minor"));
        }
        for descriptor in self.families.iter().skip(1) {
            let Some(offer) = client
                .families
                .iter()
                .find(|offer| offer.family_id == descriptor.family_id)
            else {
                return Err(Error::Invalid("server selected unoffered family"));
            };
            if !offer.versions.contains(&descriptor.version) {
                return Err(Error::Invalid("server selected unoffered family version"));
            }
        }
        if client.families.iter().any(|offer| {
            offer.required
                && !self
                    .families
                    .iter()
                    .any(|family| family.family_id == offer.family_id)
        }) {
            return Err(Error::Invalid("server omitted required family"));
        }
        self.negotiated_codecs()?.validate_offered_by(client)
    }
}

impl Encode for ServerHello {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u16(out, self.minor);
        put_u16(out, 0);
        out.extend_from_slice(&self.boot_id);
        out.extend_from_slice(&self.session_id);
        self.receive.encode_to(out)?;
        put_u64(out, self.server_monotonic_ns);
        put_u64(out, self.catalog_revision);
        put_string_u16(out, &self.server_name)?;
        put_string_u16(out, &self.server_release)?;
        put_u16(out, self.families.len() as u16);
        for family in &self.families {
            family.encode_record(out)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for ServerHello {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let minor = decoder.u16()?;
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("HELLO result reserved field"));
        }
        let boot_id = decoder.array_16()?;
        let session_id = decoder.array_16()?;
        let receive = ReceiveLimits::decode_from(&mut decoder)?;
        let server_monotonic_ns = decoder.u64()?;
        let catalog_revision = decoder.u64()?;
        let server_name = decoder.string_u16()?;
        let server_release = decoder.string_u16()?;
        let family_count = decoder.u16()?;
        let family_count = bounded_count(
            usize::from(family_count),
            decoder.remaining(),
            16,
            "HELLO selected family count",
        )?;
        let mut families = Vec::with_capacity(family_count);
        for _ in 0..family_count {
            families.push(FamilyDescriptor::decode_record(&mut decoder)?);
        }
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let value = Self {
            minor,
            boot_id,
            session_id,
            receive,
            server_monotonic_ns,
            catalog_revision,
            server_name,
            server_release,
            families,
            extensions,
        };
        value.validate()?;
        Ok(value)
    }
}

/// Process-wide resource use returned by Core SESSION_INFO extension tag 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerDiagnostics {
    pub active_sessions: u32,
    pub relay_active: u32,
    pub relay_pending: u32,
    pub aggregate_receive_limit: u64,
    pub aggregate_receive_buffered: u64,
}

impl ServerDiagnostics {
    pub fn validate(&self) -> Result<()> {
        if self.aggregate_receive_buffered > self.aggregate_receive_limit {
            return Err(Error::Invalid(
                "server diagnostics aggregate receive budget",
            ));
        }
        Ok(())
    }

    pub fn extension(&self) -> Result<Extension> {
        Ok(Extension {
            tag: crate::schema::core::SESSION_INFO_SERVER_DIAGNOSTICS_EXTENSION as u16,
            required: false,
            value: self.encode()?,
        })
    }
}

impl Encode for ServerDiagnostics {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u32(out, self.active_sessions);
        put_u32(out, self.relay_active);
        put_u32(out, self.relay_pending);
        put_u32(out, 0);
        put_u64(out, self.aggregate_receive_limit);
        put_u64(out, self.aggregate_receive_buffered);
        Ok(())
    }
}

impl Decode for ServerDiagnostics {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            active_sessions: decoder.u32()?,
            relay_active: decoder.u32()?,
            relay_pending: decoder.u32()?,
            aggregate_receive_limit: {
                if decoder.u32()? != 0 {
                    return Err(Error::Invalid("server diagnostics reserved field"));
                }
                decoder.u64()?
            },
            aggregate_receive_buffered: decoder.u64()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub session_id: [u8; 16],
    pub catalog_revision: u64,
    pub receive: ReceiveLimits,
    pub server_monotonic_ns: u64,
    pub families: Vec<FamilyDescriptor>,
    pub extensions: Extensions,
}

impl SessionInfo {
    pub fn validate(&self) -> Result<()> {
        self.receive.validate()?;
        if self.families.is_empty()
            || self.families[0].family_id != crate::family::CORE
            || self.families[0].version != VERSION
            || self.families.len() > usize::from(u16::MAX)
            || self
                .families
                .windows(2)
                .any(|pair| pair[0].family_id >= pair[1].family_id)
        {
            return Err(Error::Invalid("SESSION_INFO family order"));
        }
        for family in &self.families {
            family.validate()?;
        }
        validate_family_dependencies(&self.families, "SESSION_INFO missing family dependency")?;
        validate_result_or_event_extensions(
            &self.extensions,
            "required SESSION_INFO Result extension",
        )?;
        if let Some(extension) = self.extensions.0.iter().find(|extension| {
            u64::from(extension.tag)
                == crate::schema::core::SESSION_INFO_SERVER_DIAGNOSTICS_EXTENSION
        }) {
            ServerDiagnostics::decode(&extension.value)?;
        }
        Ok(())
    }

    pub fn server_diagnostics(&self) -> Result<Option<ServerDiagnostics>> {
        self.extensions
            .0
            .iter()
            .find(|extension| {
                u64::from(extension.tag)
                    == crate::schema::core::SESSION_INFO_SERVER_DIAGNOSTICS_EXTENSION
            })
            .map(|extension| ServerDiagnostics::decode(&extension.value))
            .transpose()
    }

    pub fn validate_after(
        &self,
        session_id: &[u8; 16],
        current_revision: u64,
        previous_receive: &ReceiveLimits,
        previous_families: &[FamilyDescriptor],
    ) -> Result<()> {
        self.validate()?;
        if &self.session_id != session_id {
            return Err(Error::Invalid("SESSION_INFO session ID change"));
        }
        if self.catalog_revision < current_revision {
            return Err(Error::Invalid("SESSION_INFO catalog revision regression"));
        }
        self.receive.validate_update_from(previous_receive)?;
        if self.families.len() != previous_families.len()
            || self
                .families
                .iter()
                .zip(previous_families)
                .any(|(next, previous)| {
                    next.family_id != previous.family_id || next.version != previous.version
                })
        {
            return Err(Error::Invalid("SESSION_INFO family or version change"));
        }
        Ok(())
    }
}

fn validate_family_dependencies(
    families: &[FamilyDescriptor],
    context: &'static str,
) -> Result<()> {
    for family in families {
        let Some(metadata) = crate::schema::family_metadata(family.family_id, family.version)
        else {
            continue;
        };
        if metadata.dependencies.iter().any(|dependency| {
            !families
                .iter()
                .any(|candidate| candidate.family_id == *dependency)
        }) {
            return Err(Error::Invalid(context));
        }
    }
    Ok(())
}

impl Encode for SessionInfo {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.extend_from_slice(&self.session_id);
        put_u64(out, self.catalog_revision);
        self.receive.encode_to(out)?;
        put_u64(out, self.server_monotonic_ns);
        put_u16(out, self.families.len() as u16);
        for family in &self.families {
            family.encode_record(out)?;
        }
        self.extensions.encode_tail(out)
    }
}

impl Decode for SessionInfo {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let session_id = decoder.array_16()?;
        let catalog_revision = decoder.u64()?;
        let receive = ReceiveLimits::decode_from(&mut decoder)?;
        let server_monotonic_ns = decoder.u64()?;
        let family_count = decoder.u16()?;
        let family_count = bounded_count(
            usize::from(family_count),
            decoder.remaining(),
            16,
            "SESSION_INFO family count",
        )?;
        let mut families = Vec::with_capacity(family_count);
        for _ in 0..family_count {
            families.push(FamilyDescriptor::decode_record(&mut decoder)?);
        }
        let extensions = decoder.extensions()?;
        decoder.finish()?;
        let value = Self {
            session_id,
            catalog_revision,
            receive,
            server_monotonic_ns,
            families,
            extensions,
        };
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Invalid,
    Unsupported,
    NotFound,
    Conflict,
    Busy,
    Unavailable,
    ResourceExhausted,
    RateLimited,
    Timeout,
    Cancelled,
    Stale,
    Io,
    Internal,
    Unknown(u16),
}

impl Status {
    pub const fn code(self) -> u16 {
        match self {
            Self::Ok => crate::schema::core::status::OK,
            Self::Invalid => crate::schema::core::status::INVALID,
            Self::Unsupported => crate::schema::core::status::UNSUPPORTED,
            Self::NotFound => crate::schema::core::status::NOT_FOUND,
            Self::Conflict => crate::schema::core::status::CONFLICT,
            Self::Busy => crate::schema::core::status::BUSY,
            Self::Unavailable => crate::schema::core::status::UNAVAILABLE,
            Self::ResourceExhausted => crate::schema::core::status::RESOURCE_EXHAUSTED,
            Self::RateLimited => crate::schema::core::status::RATE_LIMITED,
            Self::Timeout => crate::schema::core::status::TIMEOUT,
            Self::Cancelled => crate::schema::core::status::CANCELLED,
            Self::Stale => crate::schema::core::status::STALE,
            Self::Io => crate::schema::core::status::IO,
            Self::Internal => crate::schema::core::status::INTERNAL,
            Self::Unknown(code) => code,
        }
    }

    pub const fn from_code(code: u16) -> Self {
        match code {
            crate::schema::core::status::OK => Self::Ok,
            crate::schema::core::status::INVALID => Self::Invalid,
            crate::schema::core::status::UNSUPPORTED => Self::Unsupported,
            crate::schema::core::status::NOT_FOUND => Self::NotFound,
            crate::schema::core::status::CONFLICT => Self::Conflict,
            crate::schema::core::status::BUSY => Self::Busy,
            crate::schema::core::status::UNAVAILABLE => Self::Unavailable,
            crate::schema::core::status::RESOURCE_EXHAUSTED => Self::ResourceExhausted,
            crate::schema::core::status::RATE_LIMITED => Self::RateLimited,
            crate::schema::core::status::TIMEOUT => Self::Timeout,
            crate::schema::core::status::CANCELLED => Self::Cancelled,
            crate::schema::core::status::STALE => Self::Stale,
            crate::schema::core::status::IO => Self::Io,
            crate::schema::core::status::INTERNAL => Self::Internal,
            value => Self::Unknown(value),
        }
    }

    pub const fn is_ok(self) -> bool {
        matches!(self, Self::Ok)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResultPrefix {
    pub status: Status,
    pub detail: Extensions,
    pub body: Vec<u8>,
}

impl Encode for ResultPrefix {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if !self.status.is_ok() && !self.body.is_empty() {
            return Err(Error::Invalid("failed Result operation body"));
        }
        validate_result_or_event_extensions(&self.detail, "required Result detail extension")?;
        put_u16(out, self.status.code());
        put_u16(out, 0);
        let mut detail = Vec::new();
        self.detail.encode_entries(&mut detail)?;
        put_len_u32(out, detail.len())?;
        out.extend_from_slice(&detail);
        out.extend_from_slice(&self.body);
        Ok(())
    }
}

impl Decode for ResultPrefix {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let status = Status::from_code(decoder.u16()?);
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("Result flags"));
        }
        let detail = Extensions::decode_entries(decoder.len_bytes_u32()?)?;
        let body = decoder.rest().to_vec();
        decoder.finish()?;
        if !status.is_ok() && !body.is_empty() {
            return Err(Error::Invalid("failed Result operation body"));
        }
        let value = Self {
            status,
            detail,
            body,
        };
        validate_result_or_event_extensions(&value.detail, "required Result detail extension")?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ping {
    pub sender_monotonic_ns: u64,
}

fixed_u64_codec!(Ping, sender_monotonic_ns);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PingResult {
    pub receiver_receive_ns: u64,
    pub receiver_send_ns: u64,
}

impl PingResult {
    pub fn validate(&self) -> Result<()> {
        if self.receiver_send_ns < self.receiver_receive_ns {
            return Err(Error::Invalid("PING Result timestamp order"));
        }
        Ok(())
    }
}

impl Encode for PingResult {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        put_u64(out, self.receiver_receive_ns);
        put_u64(out, self.receiver_send_ns);
        Ok(())
    }
}

impl Decode for PingResult {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            receiver_receive_ns: decoder.u64()?,
            receiver_send_ns: decoder.u64()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cancel {
    pub target_request_id: u32,
}

impl Encode for Cancel {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.target_request_id == 0 {
            return Err(Error::Invalid("zero CANCEL target request ID"));
        }
        put_u32(out, self.target_request_id);
        Ok(())
    }
}

impl Decode for Cancel {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            target_request_id: decoder.u32()?,
        };
        decoder.finish()?;
        if value.target_request_id == 0 {
            return Err(Error::Invalid("zero CANCEL target request ID"));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shutdown {
    pub operation_id: [u8; 16],
    pub grace_ns: u64,
    pub reason: String,
}

impl Shutdown {
    pub fn validate(&self) -> Result<()> {
        if self.operation_id == [0; 16] {
            return Err(Error::Invalid("zero SHUTDOWN operation ID"));
        }
        Ok(())
    }
}

impl Encode for Shutdown {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        self.validate()?;
        out.extend_from_slice(&self.operation_id);
        put_u64(out, self.grace_ns);
        put_string_u32(out, &self.reason)
    }
}

impl Decode for Shutdown {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            operation_id: decoder.array_16()?,
            grace_ns: decoder.u64()?,
            reason: decoder.string_u32()?,
        };
        decoder.finish()?;
        value.validate()?;
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GoAway {
    pub status: Status,
    pub close_deadline_server_ns: u64,
    /// Encoded detail extension entries.
    pub detail: Extensions,
}

impl Encode for GoAway {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        validate_result_or_event_extensions(&self.detail, "required GOAWAY detail extension")?;
        put_u16(out, self.status.code());
        put_u16(out, 0);
        put_u64(out, self.close_deadline_server_ns);
        let mut detail = Vec::new();
        self.detail.encode_entries(&mut detail)?;
        put_len_u32(out, detail.len())?;
        out.extend_from_slice(&detail);
        Ok(())
    }
}

impl Decode for GoAway {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let status = Status::from_code(decoder.u16()?);
        if decoder.u16()? != 0 {
            return Err(Error::Invalid("GOAWAY reserved field"));
        }
        let close_deadline_server_ns = decoder.u64()?;
        let detail = Extensions::decode_entries(decoder.len_bytes_u32()?)?;
        decoder.finish()?;
        let value = Self {
            status,
            close_deadline_server_ns,
            detail,
        };
        validate_result_or_event_extensions(&value.detail, "required GOAWAY detail extension")?;
        Ok(value)
    }
}

/// How an incoming catalogue revision relates to the currently applied one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogStep {
    Contiguous,
    Gap,
}

pub fn catalog_step(current: u64, incoming: u64) -> Result<CatalogStep> {
    if incoming <= current {
        return Err(Error::Invalid("non-increasing catalog revision"));
    }
    Ok(if current.checked_add(1) == Some(incoming) {
        CatalogStep::Contiguous
    } else {
        CatalogStep::Gap
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionUpdate {
    pub catalog_revision: u64,
    pub receive: ReceiveLimits,
    pub extensions: Extensions,
}

impl Encode for SessionUpdate {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.catalog_revision == 0 {
            return Err(Error::Invalid("zero SESSION_UPDATE catalog revision"));
        }
        self.receive.validate()?;
        validate_result_or_event_extensions(&self.extensions, "required SESSION_UPDATE extension")?;
        put_u64(out, self.catalog_revision);
        self.receive.encode_to(out)?;
        self.extensions.encode_tail(out)
    }
}

impl Decode for SessionUpdate {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            catalog_revision: decoder.u64()?,
            receive: ReceiveLimits::decode_from(&mut decoder)?,
            extensions: decoder.extensions()?,
        };
        decoder.finish()?;
        if value.catalog_revision == 0 {
            return Err(Error::Invalid("zero SESSION_UPDATE catalog revision"));
        }
        validate_result_or_event_extensions(
            &value.extensions,
            "required SESSION_UPDATE extension",
        )?;
        Ok(value)
    }
}

impl SessionUpdate {
    pub fn validate_after(
        &self,
        current_revision: u64,
        previous_receive: &ReceiveLimits,
    ) -> Result<CatalogStep> {
        self.receive.validate_update_from(previous_receive)?;
        validate_result_or_event_extensions(&self.extensions, "required SESSION_UPDATE extension")?;
        catalog_step(current_revision, self.catalog_revision)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FamilyUpdate {
    pub catalog_revision: u64,
    pub family: FamilyDescriptor,
}

impl Encode for FamilyUpdate {
    fn encode_to(&self, out: &mut Vec<u8>) -> Result<()> {
        if self.catalog_revision == 0 {
            return Err(Error::Invalid("zero FAMILY_UPDATE catalog revision"));
        }
        self.family.validate()?;
        put_u64(out, self.catalog_revision);
        self.family.encode_record(out)
    }
}

impl FamilyUpdate {
    pub fn validate_after(
        &self,
        current_revision: u64,
        previous: &FamilyDescriptor,
    ) -> Result<CatalogStep> {
        self.family.validate()?;
        if self.family.family_id != previous.family_id || self.family.version != previous.version {
            return Err(Error::Invalid("FAMILY_UPDATE family or version change"));
        }
        catalog_step(current_revision, self.catalog_revision)
    }
}

impl Decode for FamilyUpdate {
    fn decode(input: &[u8]) -> Result<Self> {
        let mut decoder = Decoder::new(input);
        let value = Self {
            catalog_revision: decoder.u64()?,
            family: FamilyDescriptor::decode_record(&mut decoder)?,
        };
        decoder.finish()?;
        if value.catalog_revision == 0 {
            return Err(Error::Invalid("zero FAMILY_UPDATE catalog revision"));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits() -> ReceiveLimits {
        ReceiveLimits {
            max_frame: 1024,
            max_decoded: 2048,
            max_datagram: 0,
            max_buffered: 4096,
        }
    }

    #[test]
    fn client_hello_golden_and_all_truncations() {
        let hello = ClientHello {
            min_minor: 0,
            max_minor: 1,
            receive: limits(),
            client_instance: [0xaa; 16],
            client_name: "web".into(),
            client_release: "1".into(),
            families: vec![FamilyOffer {
                family_id: crate::family::RELAY,
                versions: vec![2, 1],
                required: true,
            }],
            codecs: vec![1],
            extensions: Extensions::default(),
        };
        let bytes = hello.encode().unwrap();
        assert_eq!(&bytes[..8], &[0, 0, 1, 0, 0, 4, 0, 0]);
        assert_eq!(ClientHello::decode(&bytes).unwrap(), hello);
        for end in 0..bytes.len() {
            assert!(ClientHello::decode(&bytes[..end]).is_err(), "prefix {end}");
        }
    }

    #[test]
    fn server_hello_and_descriptors_round_trip() {
        let hello = ServerHello {
            minor: 0,
            boot_id: [1; 16],
            session_id: [2; 16],
            receive: limits(),
            server_monotonic_ns: 3,
            catalog_revision: 4,
            server_name: "home".into(),
            server_release: "dev".into(),
            families: vec![FamilyDescriptor {
                family_id: crate::family::CORE,
                version: 1,
                runtime_state: RuntimeState::Available,
                operations: vec![Operation {
                    server_accepts: true,
                    server_sends: false,
                    class: Class::Request,
                    kind: request_kind::HELLO,
                }],
                limits: Extensions::default(),
            }],
            extensions: Extensions::default(),
        };
        let bytes = hello.encode().unwrap();
        assert_eq!(ServerHello::decode(&bytes).unwrap(), hello);
    }

    #[test]
    fn session_info_server_diagnostics_round_trip_and_validate() {
        let diagnostics = ServerDiagnostics {
            active_sessions: 7,
            relay_active: 3,
            relay_pending: 2,
            aggregate_receive_limit: 64 * 1024 * 1024,
            aggregate_receive_buffered: 8192,
        };
        assert_eq!(
            ServerDiagnostics::decode(&diagnostics.encode().unwrap()).unwrap(),
            diagnostics
        );

        let info = SessionInfo {
            session_id: [2; 16],
            catalog_revision: 4,
            receive: limits(),
            server_monotonic_ns: 3,
            families: vec![FamilyDescriptor {
                family_id: crate::family::CORE,
                version: VERSION,
                runtime_state: RuntimeState::Available,
                operations: vec![],
                limits: Extensions::default(),
            }],
            extensions: Extensions(vec![diagnostics.extension().unwrap()]),
        };
        let decoded = SessionInfo::decode(&info.encode().unwrap()).unwrap();
        assert_eq!(decoded.server_diagnostics().unwrap(), Some(diagnostics));

        let mut reserved = diagnostics.encode().unwrap();
        reserved[12] = 1;
        assert_eq!(
            ServerDiagnostics::decode(&reserved),
            Err(Error::Invalid("server diagnostics reserved field"))
        );

        let invalid_budget = ServerDiagnostics {
            aggregate_receive_limit: 1,
            aggregate_receive_buffered: 2,
            ..diagnostics
        };
        assert_eq!(
            invalid_budget.encode(),
            Err(Error::Invalid(
                "server diagnostics aggregate receive budget"
            ))
        );
    }

    #[test]
    fn catalogues_require_generated_family_dependencies() {
        let core = FamilyDescriptor {
            family_id: crate::family::CORE,
            version: VERSION,
            runtime_state: RuntimeState::Available,
            operations: vec![],
            limits: Extensions::default(),
        };
        let relay = FamilyDescriptor {
            family_id: crate::family::RELAY,
            version: crate::relay::VERSION,
            runtime_state: RuntimeState::Available,
            operations: vec![],
            limits: crate::relay::Limits::HARD.to_extensions().unwrap(),
        };
        let hello = ServerHello {
            minor: 0,
            boot_id: [1; 16],
            session_id: [2; 16],
            receive: limits(),
            server_monotonic_ns: 3,
            catalog_revision: 4,
            server_name: "home".into(),
            server_release: "dev".into(),
            families: vec![core.clone(), relay.clone()],
            extensions: Extensions::default(),
        };
        assert_eq!(
            hello.validate(),
            Err(Error::Invalid("HELLO missing family dependency"))
        );

        let info = SessionInfo {
            session_id: [2; 16],
            catalog_revision: 4,
            receive: limits(),
            server_monotonic_ns: 3,
            families: vec![core.clone(), relay.clone()],
            extensions: Extensions::default(),
        };
        assert_eq!(
            info.validate(),
            Err(Error::Invalid("SESSION_INFO missing family dependency"))
        );

        let transfer = FamilyDescriptor {
            family_id: crate::family::TRANSFER,
            version: crate::transfer::VERSION,
            runtime_state: RuntimeState::Available,
            operations: vec![],
            limits: Extensions::default(),
        };
        ServerHello {
            families: vec![core, transfer, relay],
            ..hello
        }
        .validate()
        .unwrap();
    }

    #[test]
    fn family_descriptor_enforces_generated_limit_policies() {
        let descriptor = FamilyDescriptor {
            family_id: crate::family::RELAY,
            version: crate::relay::VERSION,
            runtime_state: RuntimeState::Available,
            operations: vec![],
            limits: crate::relay::Limits::HARD.to_extensions().unwrap(),
        };
        descriptor.validate().unwrap();

        let mut missing = descriptor.clone();
        missing.limits.0.remove(0);
        assert_eq!(
            missing.validate(),
            Err(Error::Invalid("missing required family limit"))
        );

        let mut wrong_width = descriptor.clone();
        wrong_width.limits.0[0].value.pop();
        assert_eq!(
            wrong_width.validate(),
            Err(Error::Invalid("family limit length"))
        );

        let mut below_minimum = descriptor.clone();
        below_minimum.limits.0[0].value = 0u32.to_le_bytes().to_vec();
        assert_eq!(
            below_minimum.validate(),
            Err(Error::Invalid("family limit below hard minimum"))
        );

        let mut oversized = descriptor.clone();
        oversized.limits.0[0].value = (crate::relay::Limits::HARD.max_routes + 1)
            .to_le_bytes()
            .to_vec();
        assert_eq!(
            oversized.validate(),
            Err(Error::LimitExceeded {
                limit: "family limit",
                actual: u64::from(crate::relay::Limits::HARD.max_routes) + 1,
                maximum: u64::from(crate::relay::Limits::HARD.max_routes),
            })
        );

        let mut zero_allowed = descriptor;
        zero_allowed.limits.0[3].value = 0u32.to_le_bytes().to_vec();
        zero_allowed.validate().unwrap();

        let mut invalid_relationship = zero_allowed;
        invalid_relationship.limits.0[1].value = 1u32.to_le_bytes().to_vec();
        invalid_relationship.limits.0[2].value = 2u32.to_le_bytes().to_vec();
        assert_eq!(
            invalid_relationship.validate(),
            Err(Error::Invalid("Relay family limit"))
        );
    }

    #[test]
    fn result_failure_must_not_have_body() {
        let value = ResultPrefix {
            status: Status::Invalid,
            detail: Extensions::default(),
            body: vec![1],
        };
        assert_eq!(
            value.encode(),
            Err(Error::Invalid("failed Result operation body"))
        );
    }

    #[test]
    fn core_control_round_trips() {
        let ping = Ping {
            sender_monotonic_ns: 7,
        };
        assert_eq!(Ping::decode(&ping.encode().unwrap()).unwrap(), ping);
        let cancel = Cancel {
            target_request_id: 8,
        };
        assert_eq!(Cancel::decode(&cancel.encode().unwrap()).unwrap(), cancel);
        assert_eq!(
            PingResult {
                receiver_receive_ns: 2,
                receiver_send_ns: 1,
            }
            .encode(),
            Err(Error::Invalid("PING Result timestamp order"))
        );
        let goodbye = GoAway {
            status: Status::Ok,
            close_deadline_server_ns: 9,
            detail: Extensions::default(),
        };
        assert_eq!(GoAway::decode(&goodbye.encode().unwrap()).unwrap(), goodbye);
    }

    #[test]
    fn receive_limits_and_session_replacements_are_bounded() {
        let previous = limits();
        let mut invalid = previous;
        invalid.max_frame = crate::schema::transport::CORRELATED_HEADER_BYTES as u32 - 1;
        assert_eq!(
            invalid.validate(),
            Err(Error::Invalid("HELLO receive_max_frame"))
        );
        invalid = previous;
        invalid.max_datagram = crate::schema::transport::EVENT_HEADER_BYTES as u32 - 1;
        assert_eq!(
            invalid.validate(),
            Err(Error::Invalid("HELLO receive_max_datagram"))
        );

        let mut replacement = previous;
        replacement.max_buffered /= 2;
        replacement.max_datagram = 1200;
        replacement.validate_update_from(&previous).unwrap();
        replacement.max_frame -= 1;
        assert_eq!(
            replacement.validate_update_from(&previous),
            Err(Error::Invalid("SESSION_UPDATE frame limit reduction"))
        );
    }

    #[test]
    fn hello_codec_negotiation_is_canonical_and_a_subset() {
        let mut client = ClientHello {
            min_minor: 0,
            max_minor: 0,
            receive: limits(),
            client_instance: [1; 16],
            client_name: "client".into(),
            client_release: "v1".into(),
            families: vec![],
            codecs: vec![1, 3],
            extensions: Extensions::default(),
        };
        client.validate().unwrap();
        NegotiatedCodecs(vec![1])
            .validate_offered_by(&client)
            .unwrap();
        assert_eq!(
            NegotiatedCodecs(vec![2]).validate_offered_by(&client),
            Err(Error::Invalid("server selected unoffered codec"))
        );
        client.codecs = vec![3, 1];
        assert_eq!(client.validate(), Err(Error::Invalid("HELLO codec order")));
        client.codecs = vec![0];
        assert_eq!(client.validate(), Err(Error::Invalid("HELLO codec order")));
    }

    #[test]
    fn descriptors_advertise_requests_and_events_but_not_results() {
        let ping = crate::schema::operation(
            crate::family::CORE,
            crate::schema::transport::class::REQUEST,
            request_kind::PING,
        )
        .unwrap();
        let operation = Operation::from_schema(ping).unwrap();
        assert_eq!(operation.class, Class::Request);
        assert!(operation.server_accepts && operation.server_sends);

        let descriptor = FamilyDescriptor {
            family_id: crate::family::CORE,
            version: VERSION,
            runtime_state: RuntimeState::Available,
            operations: vec![Operation {
                class: Class::Result,
                ..operation
            }],
            limits: Extensions::default(),
        };
        assert_eq!(
            descriptor.validate(),
            Err(Error::Invalid("family operation class or direction"))
        );

        let reversed = FamilyDescriptor {
            family_id: crate::family::CORE,
            version: VERSION,
            runtime_state: RuntimeState::Available,
            operations: vec![Operation {
                server_accepts: false,
                server_sends: true,
                class: Class::Request,
                kind: request_kind::HELLO,
            }],
            limits: Extensions::default(),
        };
        assert_eq!(
            reversed.validate(),
            Err(Error::Invalid("family operation direction exceeds schema"))
        );
    }

    #[test]
    fn catalogue_updates_are_contiguous_or_require_resync() {
        assert_eq!(catalog_step(4, 5), Ok(CatalogStep::Contiguous));
        assert_eq!(catalog_step(4, 6), Ok(CatalogStep::Gap));
        assert_eq!(
            catalog_step(4, 4),
            Err(Error::Invalid("non-increasing catalog revision"))
        );

        let mut receive = limits();
        receive.max_frame *= 2;
        receive.max_decoded *= 2;
        let update = SessionUpdate {
            catalog_revision: 5,
            receive,
            extensions: Extensions::default(),
        };
        assert_eq!(
            update.validate_after(4, &limits()),
            Ok(CatalogStep::Contiguous)
        );

        let previous = FamilyDescriptor {
            family_id: crate::family::CORE,
            version: VERSION,
            runtime_state: RuntimeState::Available,
            operations: vec![],
            limits: Extensions::default(),
        };
        let family_update = FamilyUpdate {
            catalog_revision: 7,
            family: FamilyDescriptor {
                version: VERSION + 1,
                ..previous.clone()
            },
        };
        assert_eq!(
            family_update.validate_after(6, &previous),
            Err(Error::Invalid("FAMILY_UPDATE family or version change"))
        );

        assert_eq!(
            SessionUpdate {
                catalog_revision: 0,
                receive: limits(),
                extensions: Extensions::default(),
            }
            .encode(),
            Err(Error::Invalid("zero SESSION_UPDATE catalog revision"))
        );
        assert_eq!(
            FamilyUpdate {
                catalog_revision: 0,
                family: previous,
            }
            .encode(),
            Err(Error::Invalid("zero FAMILY_UPDATE catalog revision"))
        );
    }

    #[test]
    fn shutdown_and_result_event_extensions_enforce_invariants() {
        let shutdown = Shutdown {
            operation_id: [0; 16],
            grace_ns: 0,
            reason: String::new(),
        };
        assert_eq!(
            shutdown.encode(),
            Err(Error::Invalid("zero SHUTDOWN operation ID"))
        );
        let detail = Extensions(vec![Extension {
            tag: 1,
            required: true,
            value: vec![],
        }]);
        assert_eq!(
            ResultPrefix {
                status: Status::Ok,
                detail: detail.clone(),
                body: vec![],
            }
            .encode(),
            Err(Error::Invalid("required Result detail extension"))
        );
        assert_eq!(
            GoAway {
                status: Status::Ok,
                close_deadline_server_ns: 0,
                detail,
            }
            .encode(),
            Err(Error::Invalid("required GOAWAY detail extension"))
        );
    }

    #[test]
    fn malicious_counts_fail_before_capacity_allocation() {
        let hello = ClientHello {
            min_minor: 0,
            max_minor: 0,
            receive: limits(),
            client_instance: [1; 16],
            client_name: String::new(),
            client_release: String::new(),
            families: vec![],
            codecs: vec![],
            extensions: Extensions::default(),
        };
        let mut bytes = hello.encode().unwrap();
        bytes[44..46].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(
            ClientHello::decode(&bytes),
            Err(Error::Invalid("HELLO family count"))
        );
        assert_eq!(
            NegotiatedCodecs::decode(&[u8::MAX]),
            Err(Error::Invalid("negotiated codec count"))
        );

        let mut update = Vec::new();
        put_u64(&mut update, 1);
        put_u32(&mut update, 12);
        put_u16(&mut update, crate::family::CORE);
        put_u16(&mut update, VERSION);
        update.push(RuntimeState::Available as u8);
        update.push(0);
        put_u16(&mut update, u16::MAX);
        put_u32(&mut update, 0);
        assert_eq!(
            FamilyUpdate::decode(&update),
            Err(Error::Invalid("family operation count"))
        );
    }
}
