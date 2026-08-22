use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    schema: u16,
    family: Vec<RegistryFamily>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RegistryFamily {
    name: String,
    const_name: String,
    id: u16,
    version: u16,
    #[serde(default)]
    dependencies: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FamilySpec {
    schema: u16,
    family: String,
    #[serde(default)]
    limits: Vec<LimitMetadata>,
    #[serde(default)]
    request: Vec<Operation>,
    #[serde(default)]
    event: Vec<Operation>,
    #[serde(default)]
    status: Vec<Status>,
    #[serde(default, rename = "type")]
    types: Vec<TypeMetadata>,
    #[serde(default, rename = "constant")]
    constants: Vec<Constant>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Constant {
    name: String,
    value: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum LimitValueType {
    U32,
    U64,
}

impl LimitValueType {
    const fn width(self) -> u8 {
        match self {
            Self::U32 => 4,
            Self::U64 => 8,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LimitMetadata {
    name: String,
    tag: u16,
    #[serde(rename = "type")]
    value_type: LimitValueType,
    required: bool,
    hard_min: u64,
    hard_max: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransportSpec {
    schema: u16,
    preface_hex: String,
    protocol_major: u16,
    websocket_subprotocol: String,
    stream_length_bits: u8,
    event_header_bytes: u8,
    correlated_header_bytes: u8,
    recommended: TransportRecommended,
    header: TransportHeader,
    class: TransportClass,
    meta: TransportMeta,
    datagram_predicate: TransportDatagramPredicate,
    limits: TransportLimits,
    codec: Vec<TransportCodec>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransportRecommended {
    wire_frame: u32,
    decoded_frame: u32,
    buffered: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransportHeader {
    family_bits: u8,
    kind_bits: u8,
    meta_bits: u8,
    request_id_bits: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransportClass {
    event: u8,
    request: u8,
    result: u8,
    mask: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransportMeta {
    compressed: u8,
    sensitive: u8,
    reserved_mask: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransportDatagramPredicate {
    forbidden: u8,
    net_native_flow: u8,
    surface_frame: u8,
    media_frame: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransportLimits {
    pre_hello_frame: u32,
    wire_frame: u32,
    decoded_frame: u32,
    datagram: u32,
    bulk_chunk: u32,
    buffered: u64,
    extension_entries: u32,
    typed_records: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TransportCodec {
    name: String,
    id: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StateSpec {
    schema: u16,
    #[serde(default, rename = "constant")]
    constants: Vec<Constant>,
    #[serde(default, rename = "type")]
    types: Vec<TypeMetadata>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Operation {
    name: String,
    kind: u16,
    direction: Direction,
    sensitive: FlagPolicy,
    compression: FlagPolicy,
    datagram: DatagramPredicate,
    layout: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Direction {
    ClientToServer,
    ServerToClient,
    Bidirectional,
}

impl Direction {
    const fn wire(self) -> u8 {
        match self {
            Self::ClientToServer => 0,
            Self::ServerToClient => 1,
            Self::Bidirectional => 2,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::ClientToServer => "client_to_server",
            Self::ServerToClient => "server_to_client",
            Self::Bidirectional => "bidirectional",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum FlagPolicy {
    Required,
    Forbidden,
    Allowed,
}

impl FlagPolicy {
    const fn wire(self) -> u8 {
        match self {
            Self::Allowed => 0,
            Self::Required => 1,
            Self::Forbidden => 2,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Required => "required",
            Self::Forbidden => "forbidden",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DatagramPredicate {
    Forbidden,
    NetNativeFlow,
    SurfaceFrame,
    MediaFrame,
}

impl DatagramPredicate {
    const fn wire(self, values: &TransportDatagramPredicate) -> u8 {
        match self {
            Self::Forbidden => values.forbidden,
            Self::NetNativeFlow => values.net_native_flow,
            Self::SurfaceFrame => values.surface_frame,
            Self::MediaFrame => values.media_frame,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Forbidden => "forbidden",
            Self::NetNativeFlow => "net_native_flow",
            Self::SurfaceFrame => "surface_frame",
            Self::MediaFrame => "media_frame",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Status {
    name: String,
    code: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct TypeMetadata {
    name: String,
    layout: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CodecSpec {
    schema: u16,
    name: String,
    const_name: String,
    family: String,
    id: u16,
    version: u16,
    direction: Direction,
    layout: String,
    golden_hex: String,
    #[serde(default, rename = "constant")]
    constants: Vec<Constant>,
}

#[derive(Debug, Serialize)]
struct Artifact {
    schema: u16,
    transport: TransportSpec,
    state: StateSpec,
    families: Vec<FamilyArtifact>,
    codecs: Vec<CodecSpec>,
    statuses: Vec<Status>,
}

#[derive(Debug, Serialize)]
struct FamilyArtifact {
    name: String,
    const_name: String,
    id: u16,
    version: u16,
    dependencies: Vec<u16>,
    limits: Vec<LimitMetadata>,
    requests: Vec<Operation>,
    events: Vec<Operation>,
    types: Vec<TypeMetadata>,
    constants: Vec<Constant>,
}

#[derive(Debug, Serialize)]
struct VectorArtifact {
    schema: u16,
    vectors: Vec<GoldenVector>,
}

#[derive(Debug, Serialize)]
struct GoldenVector {
    name: String,
    hex: String,
}

#[derive(Debug)]
pub struct Generated {
    pub rust: String,
    pub json: String,
    pub vectors: String,
    pub typescript: String,
    pub markdown: String,
    pub inspection: String,
}

pub fn generate(schema_dir: &Path) -> Result<Generated, String> {
    let registry: Registry = load(&schema_dir.join("registry.toml"))?;
    if registry.schema != 1 {
        return Err("registry schema must be 1".into());
    }
    validate_registry(&registry.family)?;
    let transport: TransportSpec = load(&schema_dir.join("transport.toml"))?;
    validate_transport(&transport, registry.schema)?;
    let state: StateSpec = load(&schema_dir.join("state.toml"))?;
    validate_state(&state, registry.schema)?;

    let mut specs = BTreeMap::new();
    let mut family_paths = fs::read_dir(schema_dir.join("families"))
        .map_err(|error| format!("families directory: {error}"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| format!("families directory: {error}"))?;
    family_paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "toml")
    });
    family_paths.sort();
    for path in family_paths {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("unknown");
        let spec: FamilySpec = load(&path)?;
        if spec.schema != registry.schema {
            return Err(format!("{stem}.toml schema version differs from registry"));
        }
        validate_spec(&spec)?;
        if specs.insert(spec.family.clone(), spec).is_some() {
            return Err(format!("duplicate family specification for {stem}"));
        }
    }

    let mut families = Vec::new();
    let mut statuses = Vec::new();
    let family_ids = registry
        .family
        .iter()
        .map(|family| (family.name.as_str(), family.id))
        .collect::<BTreeMap<_, _>>();
    for family in &registry.family {
        let spec = specs
            .remove(&family.name)
            .ok_or_else(|| format!("missing specification for {}", family.name))?;
        if family.name == "yas.core" {
            statuses = spec.status;
        } else if !spec.status.is_empty() {
            return Err(format!(
                "statuses are only legal in yas.core, found {}",
                family.name
            ));
        }
        families.push(FamilyArtifact {
            name: family.name.clone(),
            const_name: family.const_name.clone(),
            id: family.id,
            version: family.version,
            dependencies: family
                .dependencies
                .iter()
                .map(|dependency| family_ids[dependency.as_str()])
                .collect(),
            limits: spec.limits,
            requests: spec.request,
            events: spec.event,
            types: spec.types,
            constants: spec.constants,
        });
    }
    if !specs.is_empty() {
        return Err(format!(
            "unregistered family specifications: {:?}",
            specs.keys()
        ));
    }

    let mut codecs = Vec::new();
    let codec_dir = schema_dir.join("codecs");
    let mut codec_paths = fs::read_dir(&codec_dir)
        .map_err(|error| format!("codecs directory: {error}"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| format!("codecs directory: {error}"))?;
    codec_paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "toml")
    });
    codec_paths.sort();
    let registered = registry
        .family
        .iter()
        .map(|family| family.name.as_str())
        .collect::<BTreeSet<_>>();
    let mut codec_names = BTreeSet::new();
    let mut codec_constants = BTreeSet::new();
    let mut codec_ids = BTreeSet::new();
    for path in codec_paths {
        let codec: CodecSpec = load(&path)?;
        validate_const(&codec.const_name)?;
        if codec.schema != registry.schema
            || codec.name.is_empty()
            || codec.layout.is_empty()
            || codec.id == 0
            || codec.version == 0
            || codec.golden_hex.is_empty()
            || !codec.golden_hex.len().is_multiple_of(2)
            || !codec
                .golden_hex
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            || !registered.contains(codec.family.as_str())
            || !codec_names.insert(codec.name.clone())
            || !codec_constants.insert(codec.const_name.clone())
            || !codec_ids.insert((codec.family.clone(), codec.id))
        {
            return Err(format!(
                "invalid or duplicate packed codec {}",
                path.display()
            ));
        }
        validate_constants(&codec.name, &codec.constants)?;
        codecs.push(codec);
    }

    let artifact = Artifact {
        schema: registry.schema,
        transport,
        state,
        families,
        codecs,
        statuses,
    };
    let json = format!("{}\n", serde_json::to_string_pretty(&artifact).unwrap());
    let vector_artifact = vectors(&artifact);
    let vectors = format!(
        "{}\n",
        serde_json::to_string_pretty(&vector_artifact).unwrap()
    );
    let rust = generate_rust(&artifact, &vector_artifact);
    let typescript = generate_typescript(&artifact, &vector_artifact, &json, &vectors);
    let markdown = generate_markdown(&artifact);
    let inspection = generate_inspection(&artifact);
    Ok(Generated {
        rust,
        json,
        vectors,
        typescript,
        markdown,
        inspection,
    })
}

fn load<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    toml::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))
}

fn validate_registry(families: &[RegistryFamily]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut const_names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut previous = None;
    let family_ids = families
        .iter()
        .map(|family| (family.name.as_str(), family.id))
        .collect::<BTreeMap<_, _>>();
    for family in families {
        if !names.insert(&family.name)
            || !const_names.insert(&family.const_name)
            || !ids.insert(family.id)
        {
            return Err(format!("duplicate registry family {}", family.name));
        }
        validate_const(&family.const_name)?;
        if previous.is_some_and(|id| id >= family.id) {
            return Err("registry families must be ordered by increasing ID".into());
        }
        if family.version == 0 {
            return Err(format!("zero version for {}", family.name));
        }
        let mut previous_dependency = None;
        for dependency in &family.dependencies {
            let Some(&dependency_id) = family_ids.get(dependency.as_str()) else {
                return Err(format!(
                    "unknown dependency {dependency} for {}",
                    family.name
                ));
            };
            if dependency_id >= family.id
                || previous_dependency.is_some_and(|previous| previous >= dependency_id)
            {
                return Err(format!(
                    "dependencies for {} must be unique, ordered, and precede the family",
                    family.name
                ));
            }
            previous_dependency = Some(dependency_id);
        }
        previous = Some(family.id);
    }
    Ok(())
}

fn validate_transport(transport: &TransportSpec, schema: u16) -> Result<(), String> {
    if transport.schema != schema
        || transport.preface_hex.len() != 16
        || !transport
            .preface_hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || transport.protocol_major == 0
        || transport.websocket_subprotocol.is_empty()
        || transport.stream_length_bits != 32
        || transport.event_header_bytes != 5
        || transport.correlated_header_bytes != 9
        || transport.header.family_bits != 16
        || transport.header.kind_bits != 16
        || transport.header.meta_bits != 8
        || transport.header.request_id_bits != 32
        || transport.class.event != 0
        || transport.class.request != 1
        || transport.class.result != 2
        || transport.class.mask != 3
        || transport.meta.compressed & transport.class.mask != 0
        || transport.meta.sensitive & transport.class.mask != 0
        || transport.meta.compressed.count_ones() != 1
        || transport.meta.sensitive.count_ones() != 1
        || transport.meta.compressed & transport.meta.sensitive != 0
        || transport.meta.reserved_mask
            & (transport.class.mask | transport.meta.compressed | transport.meta.sensitive)
            != 0
        || transport.class.mask
            | transport.meta.compressed
            | transport.meta.sensitive
            | transport.meta.reserved_mask
            != u8::MAX
        || transport.limits.pre_hello_frame < transport.correlated_header_bytes as u32
        || transport.limits.pre_hello_frame > transport.limits.wire_frame
        || transport.recommended.wire_frame < transport.correlated_header_bytes as u32
        || transport.recommended.wire_frame > transport.limits.wire_frame
        || transport.recommended.decoded_frame < transport.recommended.wire_frame
        || transport.recommended.decoded_frame > transport.limits.decoded_frame
        || transport.recommended.buffered == 0
        || transport.recommended.buffered > transport.limits.buffered
        || transport.limits.wire_frame < transport.correlated_header_bytes as u32
        || transport.limits.wire_frame > transport.limits.decoded_frame
        || transport.limits.datagram == 0
        || transport.limits.datagram < transport.event_header_bytes as u32
        || transport.limits.datagram > transport.limits.wire_frame
        || transport.limits.bulk_chunk == 0
        || transport.limits.bulk_chunk > transport.limits.wire_frame
        || transport.limits.buffered == 0
        || transport.limits.extension_entries == 0
        || transport.limits.extension_entries > u32::from(u16::MAX)
        || transport.limits.typed_records == 0
        || transport.limits.typed_records > u32::from(u16::MAX)
    {
        return Err("invalid transport schema".into());
    }
    let preface = decode_hex(&transport.preface_hex);
    if preface[..4] != *b"YAS\0"
        || preface[4..6] != transport.protocol_major.to_le_bytes()
        || preface[6..] != [0x0d, 0x0a]
        || transport.websocket_subprotocol != format!("yas.v{}", transport.protocol_major)
    {
        return Err("transport selector or preface differs from protocol major".into());
    }
    let predicates = [
        transport.datagram_predicate.forbidden,
        transport.datagram_predicate.net_native_flow,
        transport.datagram_predicate.surface_frame,
        transport.datagram_predicate.media_frame,
    ];
    if transport.datagram_predicate.forbidden != 0
        || predicates.iter().copied().collect::<BTreeSet<_>>().len() != predicates.len()
    {
        return Err("invalid transport datagram predicates".into());
    }
    let mut names = BTreeSet::new();
    let mut ids = BTreeSet::new();
    for codec in &transport.codec {
        validate_const(&codec.name)?;
        if codec.id == 0 || !names.insert(&codec.name) || !ids.insert(codec.id) {
            return Err("invalid or duplicate transport codec".into());
        }
    }
    Ok(())
}

fn validate_state(state: &StateSpec, schema: u16) -> Result<(), String> {
    if state.schema != schema {
        return Err("state schema version differs from registry".into());
    }
    validate_constants("state", &state.constants)?;
    let mut names = BTreeSet::new();
    for value in &state.types {
        if value.name.is_empty() || value.layout.is_empty() || !names.insert(&value.name) {
            return Err("invalid or duplicate state type metadata".into());
        }
    }
    Ok(())
}

fn validate_spec(spec: &FamilySpec) -> Result<(), String> {
    validate_operations(&spec.family, "request", &spec.request)?;
    validate_operations(&spec.family, "event", &spec.event)?;
    let mut names = BTreeSet::new();
    let mut codes = BTreeSet::new();
    for status in &spec.status {
        validate_const(&status.name)?;
        if !names.insert(&status.name) || !codes.insert(status.code) {
            return Err(format!("duplicate status in {}", spec.family));
        }
    }
    let mut type_names = BTreeSet::new();
    for value in &spec.types {
        if value.name.is_empty() || value.layout.is_empty() || !type_names.insert(&value.name) {
            return Err(format!(
                "invalid or duplicate type metadata in {}",
                spec.family
            ));
        }
    }
    validate_constants(&spec.family, &spec.constants)?;
    validate_limits(&spec.family, &spec.limits, &spec.constants)?;
    Ok(())
}

fn validate_limits(
    family: &str,
    limits: &[LimitMetadata],
    constants: &[Constant],
) -> Result<(), String> {
    let limit_constants = constants
        .iter()
        .filter_map(|constant| {
            constant
                .name
                .strip_prefix("LIMIT_")
                .map(|name| (name, constant.value))
        })
        .collect::<BTreeMap<_, _>>();
    if limit_constants.len() != limits.len() {
        return Err(format!(
            "every LIMIT_ constant in {family} must have exactly one limit policy"
        ));
    }

    let mut names = BTreeSet::new();
    let mut tags = BTreeSet::new();
    let mut previous_tag = None;
    for limit in limits {
        validate_const(&limit.name)?;
        let Some(&constant_tag) = limit_constants.get(limit.name.as_str()) else {
            return Err(format!(
                "limit policy {} in {family} has no matching LIMIT_ constant",
                limit.name
            ));
        };
        if limit.tag == 0
            || constant_tag != u64::from(limit.tag)
            || !names.insert(limit.name.as_str())
            || !tags.insert(limit.tag)
            || previous_tag.is_some_and(|previous| previous >= limit.tag)
            || limit.hard_min > limit.hard_max
            || limit.hard_max == 0
            || matches!(limit.value_type, LimitValueType::U32)
                && limit.hard_max > u64::from(u32::MAX)
        {
            return Err(format!("invalid limit policy {} in {family}", limit.name));
        }
        previous_tag = Some(limit.tag);
    }
    Ok(())
}

fn validate_constants(scope: &str, constants: &[Constant]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    for constant in constants {
        validate_const(&constant.name)?;
        if !names.insert(&constant.name) {
            return Err(format!("duplicate constant in {scope}"));
        }
    }
    Ok(())
}

fn validate_operations(family: &str, class: &str, operations: &[Operation]) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    let mut previous = None;
    for operation in operations {
        validate_const(&operation.name)?;
        if operation.layout.is_empty()
            || !names.insert(&operation.name)
            || !kinds.insert(operation.kind)
        {
            return Err(format!("invalid or duplicate {class} in {family}"));
        }
        if previous.is_some_and(|kind| kind >= operation.kind) {
            return Err(format!(
                "{family} {class}s must be ordered by increasing kind"
            ));
        }
        if class == "request" && operation.datagram != DatagramPredicate::Forbidden {
            return Err(format!("datagram predicate on {family} request"));
        }
        let valid_datagram = match operation.datagram {
            DatagramPredicate::Forbidden => true,
            DatagramPredicate::NetNativeFlow => {
                family == "yas.net" && class == "event" && operation.name == "DATAGRAM"
            }
            DatagramPredicate::SurfaceFrame => {
                family == "yas.surface" && class == "event" && operation.name == "FRAME"
            }
            DatagramPredicate::MediaFrame => {
                family == "yas.media" && class == "event" && operation.name == "FRAME"
            }
        };
        if !valid_datagram {
            return Err(format!(
                "invalid datagram predicate on {family} {}",
                operation.name
            ));
        }
        previous = Some(operation.kind);
    }
    Ok(())
}

fn validate_const(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name
            .bytes()
            .any(|byte| !(byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
        || name.as_bytes()[0].is_ascii_digit()
    {
        return Err(format!("invalid generated constant name {name:?}"));
    }
    Ok(())
}

fn vectors(artifact: &Artifact) -> VectorArtifact {
    let mut vectors = vec![GoldenVector {
        name: "preface".into(),
        hex: artifact.transport.preface_hex.clone(),
    }];
    for family in &artifact.families {
        for operation in &family.requests {
            let request_id = 1u32;
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&family.id.to_le_bytes());
            bytes.extend_from_slice(&operation.kind.to_le_bytes());
            bytes.push(
                artifact.transport.class.request
                    | if matches!(operation.sensitive, FlagPolicy::Required) {
                        artifact.transport.meta.sensitive
                    } else {
                        0
                    }
                    | if matches!(operation.compression, FlagPolicy::Required) {
                        artifact.transport.meta.compressed
                    } else {
                        0
                    },
            );
            bytes.extend_from_slice(&request_id.to_le_bytes());
            vectors.push(GoldenVector {
                name: format!(
                    "{}.request.{}.header",
                    family.name,
                    operation.name.to_ascii_lowercase()
                ),
                hex: hex(&bytes),
            });
        }
        for operation in &family.events {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&family.id.to_le_bytes());
            bytes.extend_from_slice(&operation.kind.to_le_bytes());
            bytes.push(
                artifact.transport.class.event
                    | if matches!(operation.sensitive, FlagPolicy::Required) {
                        artifact.transport.meta.sensitive
                    } else {
                        0
                    }
                    | if matches!(operation.compression, FlagPolicy::Required) {
                        artifact.transport.meta.compressed
                    } else {
                        0
                    },
            );
            vectors.push(GoldenVector {
                name: format!(
                    "{}.event.{}.header",
                    family.name,
                    operation.name.to_ascii_lowercase()
                ),
                hex: hex(&bytes),
            });
        }
    }

    // Full operation payloads shared by Rust and TypeScript conformance tests.
    let mut hello = Vec::new();
    push_u16(&mut hello, 0);
    push_u16(&mut hello, 0);
    push_u32(&mut hello, 1024 * 1024);
    push_u32(&mut hello, 4 * 1024 * 1024);
    push_u32(&mut hello, 0);
    push_u64(&mut hello, 16 * 1024 * 1024);
    hello.extend_from_slice(&[0; 16]);
    push_bytes_u16(&mut hello, b"web");
    push_bytes_u16(&mut hello, b"1");
    push_u16(&mut hello, 1);
    push_u16(&mut hello, family_id(artifact, "yas.relay"));
    hello.push(1);
    hello.push(1);
    push_u16(&mut hello, family_version(artifact, "yas.relay"));
    hello.push(1);
    push_u16(&mut hello, transport_codec(artifact, "LZ4"));
    push_u32(&mut hello, 0);
    push_vector(&mut vectors, "core.client_hello.payload", &hello);

    let mut result = Vec::new();
    push_u16(&mut result, status_code(artifact, "OK"));
    push_u16(&mut result, 0);
    push_u32(&mut result, 0);
    push_vector(&mut vectors, "core.result.ok_empty.payload", &result);

    let lz4 = transport_codec(artifact, "LZ4");
    let negotiated_codecs = [1, lz4 as u8, (lz4 >> 8) as u8];
    push_vector(
        &mut vectors,
        "core.negotiated_codecs.payload",
        &negotiated_codecs,
    );

    let accepts = family_constant(artifact, "yas.core", "DIRECTION_ACCEPTS") as u8;
    let sends = family_constant(artifact, "yas.core", "DIRECTION_SENDS") as u8;
    let core_descriptor = family_descriptor(
        artifact,
        "yas.core",
        family_constant(artifact, "yas.core", "RUNTIME_AVAILABLE") as u8,
        &[
            (
                accepts,
                artifact.transport.class.request,
                request_kind(artifact, "yas.core", "HELLO"),
            ),
            (
                accepts | sends,
                artifact.transport.class.request,
                request_kind(artifact, "yas.core", "PING"),
            ),
            (
                accepts | sends,
                artifact.transport.class.request,
                request_kind(artifact, "yas.core", "CANCEL"),
            ),
            (
                sends,
                artifact.transport.class.event,
                event_kind(artifact, "yas.core", "GOAWAY"),
            ),
        ],
        &[],
    );

    let mut server_hello = Vec::new();
    push_u16(&mut server_hello, 0);
    push_u16(&mut server_hello, 0);
    server_hello.extend_from_slice(&[0x11; 16]);
    server_hello.extend_from_slice(&[0x22; 16]);
    push_u32(&mut server_hello, artifact.transport.recommended.wire_frame);
    push_u32(
        &mut server_hello,
        artifact.transport.recommended.decoded_frame,
    );
    push_u32(&mut server_hello, 0);
    push_u64(&mut server_hello, artifact.transport.recommended.buffered);
    push_u64(&mut server_hello, 0x0102_0304_0506_0708);
    push_u64(&mut server_hello, 4);
    push_bytes_u16(&mut server_hello, b"home");
    push_bytes_u16(&mut server_hello, b"v1");
    push_u16(&mut server_hello, 1);
    server_hello.extend_from_slice(&core_descriptor);
    let mut server_hello_extensions = Vec::new();
    push_u16(
        &mut server_hello_extensions,
        family_constant(
            artifact,
            "yas.core",
            "SERVER_HELLO_NEGOTIATED_CODECS_EXTENSION",
        ) as u16,
    );
    push_u16(&mut server_hello_extensions, 0);
    push_bytes_u32(&mut server_hello_extensions, &negotiated_codecs);
    push_bytes_u32(&mut server_hello, &server_hello_extensions);
    push_vector(&mut vectors, "core.server_hello.payload", &server_hello);

    let mut ping = Vec::new();
    push_u64(&mut ping, 0x0102_0304_0506_0708);
    push_vector(&mut vectors, "core.ping.payload", &ping);
    let mut ping_result = Vec::new();
    push_u64(&mut ping_result, 0x1112_1314_1516_1718);
    push_u64(&mut ping_result, 0x2122_2324_2526_2728);
    push_vector(&mut vectors, "core.ping_result.payload", &ping_result);

    let cancel = 0x1122_3344u32.to_le_bytes();
    push_vector(&mut vectors, "core.cancel.payload", &cancel);

    let mut shutdown = vec![0x33; 16];
    push_u64(&mut shutdown, 5_000_000_000);
    push_bytes_u32(&mut shutdown, b"maintenance");
    push_vector(&mut vectors, "core.shutdown.payload", &shutdown);

    let mut goaway_detail = Vec::new();
    push_u16(&mut goaway_detail, 1);
    push_u16(&mut goaway_detail, 0);
    push_bytes_u32(&mut goaway_detail, b"draining");
    let mut goaway = Vec::new();
    push_u16(&mut goaway, status_code(artifact, "OK"));
    push_u16(&mut goaway, 0);
    push_u64(&mut goaway, 0x3132_3334_3536_3738);
    push_bytes_u32(&mut goaway, &goaway_detail);
    push_vector(&mut vectors, "core.goaway.payload", &goaway);

    let mut session_update = Vec::new();
    push_u64(&mut session_update, 5);
    push_u32(
        &mut session_update,
        artifact.transport.recommended.wire_frame * 2,
    );
    push_u32(
        &mut session_update,
        artifact.transport.recommended.decoded_frame * 2,
    );
    push_u32(&mut session_update, 1200);
    push_u64(
        &mut session_update,
        artifact.transport.recommended.buffered / 2,
    );
    let mut session_extensions = Vec::new();
    push_u16(&mut session_extensions, 9);
    push_u16(&mut session_extensions, 0);
    push_bytes_u32(&mut session_extensions, &[0xaa]);
    push_bytes_u32(&mut session_update, &session_extensions);
    push_vector(&mut vectors, "core.session_update.payload", &session_update);

    let terminal_limits = family_limit_extensions(artifact, "yas.terminal");
    let transfer_descriptor = family_descriptor(
        artifact,
        "yas.transfer",
        family_constant(artifact, "yas.core", "RUNTIME_AVAILABLE") as u8,
        &[
            (
                accepts | sends,
                artifact.transport.class.event,
                event_kind(artifact, "yas.transfer", "BYTE_DATA"),
            ),
            (
                accepts | sends,
                artifact.transport.class.event,
                event_kind(artifact, "yas.transfer", "MESSAGE_DATA"),
            ),
            (
                accepts | sends,
                artifact.transport.class.event,
                event_kind(artifact, "yas.transfer", "CREDIT"),
            ),
            (
                accepts | sends,
                artifact.transport.class.event,
                event_kind(artifact, "yas.transfer", "CLOSE"),
            ),
            (
                accepts | sends,
                artifact.transport.class.event,
                event_kind(artifact, "yas.transfer", "RESET"),
            ),
        ],
        &[],
    );
    let terminal_descriptor = family_descriptor(
        artifact,
        "yas.terminal",
        family_constant(artifact, "yas.core", "RUNTIME_DEGRADED") as u8,
        &[
            (
                accepts,
                artifact.transport.class.request,
                request_kind(artifact, "yas.terminal", "OPEN_VIEW"),
            ),
            (
                sends,
                artifact.transport.class.event,
                event_kind(artifact, "yas.terminal", "STATE"),
            ),
        ],
        &terminal_limits,
    );
    let mut family_update = Vec::new();
    push_u64(&mut family_update, 6);
    family_update.extend_from_slice(&terminal_descriptor);
    push_vector(&mut vectors, "core.family_update.payload", &family_update);

    let mut session_info = Vec::new();
    session_info.extend_from_slice(&[0x22; 16]);
    push_u64(&mut session_info, 6);
    push_u32(
        &mut session_info,
        artifact.transport.recommended.wire_frame * 2,
    );
    push_u32(
        &mut session_info,
        artifact.transport.recommended.decoded_frame * 2,
    );
    push_u32(&mut session_info, 1200);
    push_u64(
        &mut session_info,
        artifact.transport.recommended.buffered / 2,
    );
    push_u64(&mut session_info, 0x4142_4344_4546_4748);
    push_u16(&mut session_info, 3);
    session_info.extend_from_slice(&core_descriptor);
    session_info.extend_from_slice(&transfer_descriptor);
    session_info.extend_from_slice(&terminal_descriptor);
    push_u32(&mut session_info, 0);
    push_vector(&mut vectors, "core.session_info.payload", &session_info);

    let mut ping_frame = Vec::new();
    push_u16(&mut ping_frame, family_id(artifact, "yas.core"));
    push_u16(&mut ping_frame, request_kind(artifact, "yas.core", "PING"));
    ping_frame.push(artifact.transport.class.request);
    push_u32(&mut ping_frame, 7);
    ping_frame.extend_from_slice(&ping);
    push_vector(&mut vectors, "transport.ping.frame", &ping_frame);
    let mut ping_stream = Vec::new();
    push_bytes_u32(&mut ping_stream, &ping_frame);
    push_vector(&mut vectors, "transport.ping.stream", &ping_stream);

    let mut shutdown_frame = Vec::new();
    push_u16(&mut shutdown_frame, family_id(artifact, "yas.core"));
    push_u16(
        &mut shutdown_frame,
        request_kind(artifact, "yas.core", "SHUTDOWN"),
    );
    shutdown_frame.push(artifact.transport.class.request | artifact.transport.meta.sensitive);
    push_u32(&mut shutdown_frame, 8);
    shutdown_frame.extend_from_slice(&shutdown);
    push_vector(&mut vectors, "transport.shutdown.frame", &shutdown_frame);

    let mut goaway_frame = Vec::new();
    push_u16(&mut goaway_frame, family_id(artifact, "yas.core"));
    push_u16(
        &mut goaway_frame,
        event_kind(artifact, "yas.core", "GOAWAY"),
    );
    goaway_frame.push(artifact.transport.class.event);
    goaway_frame.extend_from_slice(&goaway);
    push_vector(&mut vectors, "transport.goaway.frame", &goaway_frame);

    let mut descriptor = Vec::new();
    push_u32(&mut descriptor, 2);
    descriptor.push(family_constant(artifact, "yas.transfer", "MODE_BYTE") as u8);
    descriptor
        .push(family_constant(artifact, "yas.transfer", "DIRECTION_SENDER_TO_RECEIVER") as u8);
    push_u16(&mut descriptor, 0);
    push_u64(&mut descriptor, 0);
    push_u64(&mut descriptor, 4096);
    push_u64(&mut descriptor, 0);
    push_u32(&mut descriptor, artifact.transport.limits.bulk_chunk);
    push_u16(&mut descriptor, family_id(artifact, "yas.font"));
    push_u16(
        &mut descriptor,
        family_constant(artifact, "yas.font", "FACE_BYTES_CONTENT_KIND") as u16,
    );
    push_u16(&mut descriptor, family_version(artifact, "yas.font"));
    push_u32(&mut descriptor, 0);
    push_vector(&mut vectors, "transfer.descriptor.payload", &descriptor);

    let mut byte_data = Vec::new();
    push_u32(&mut byte_data, 2);
    push_u64(&mut byte_data, 0);
    byte_data.extend_from_slice(b"YAS");
    push_vector(&mut vectors, "transfer.byte_data.payload", &byte_data);

    let mut message_data = Vec::new();
    push_u32(&mut message_data, 3);
    push_u64(&mut message_data, 7);
    push_u64(&mut message_data, 0);
    message_data.push(
        (family_constant(artifact, "yas.transfer", "MESSAGE_START")
            | family_constant(artifact, "yas.transfer", "MESSAGE_END")) as u8,
    );
    message_data.extend_from_slice(&[0; 3]);
    message_data.extend_from_slice(b"msg");
    push_vector(&mut vectors, "transfer.message_data.payload", &message_data);

    let mut credit = Vec::new();
    push_u32(&mut credit, 2);
    push_u64(&mut credit, 8192);
    push_vector(&mut vectors, "transfer.credit.payload", &credit);

    let mut close = Vec::new();
    push_u32(&mut close, 2);
    push_u64(&mut close, 3);
    push_u16(&mut close, status_code(artifact, "OK"));
    push_u16(&mut close, 0);
    push_bytes_u32(&mut close, b"done");
    push_vector(&mut vectors, "transfer.close.payload", &close);

    let mut reset = Vec::new();
    push_u32(&mut reset, 3);
    push_u16(&mut reset, status_code(artifact, "CANCELLED"));
    push_u16(&mut reset, 0);
    push_bytes_u32(&mut reset, b"stop");
    push_vector(&mut vectors, "transfer.reset.payload", &reset);

    let mut watch = Vec::new();
    push_u16(&mut watch, 0);
    push_u16(&mut watch, 0);
    push_u64(&mut watch, 4096);
    push_u32(&mut watch, 0);
    push_vector(&mut vectors, "state.watch.payload", &watch);

    push_vector(&mut vectors, "state.unwatch.payload", &1u32.to_le_bytes());

    let mut state_ack = Vec::new();
    push_u32(&mut state_ack, 1);
    push_u64(&mut state_ack, 2);
    push_u64(&mut state_ack, 8192);
    push_vector(&mut vectors, "state.ack.payload", &state_ack);

    let mut state = Vec::new();
    push_u32(&mut state, 1);
    state.push(state_constant(artifact, "PHASE_DELTA") as u8);
    state.push(0);
    push_u16(&mut state, 0);
    push_u64(&mut state, 1);
    push_u64(&mut state, 2);
    push_u16(&mut state, 1);
    push_u32(&mut state, 20);
    push_u16(&mut state, state_constant(artifact, "RECORD_REMOVE") as u16);
    push_u16(&mut state, 0);
    push_u64(&mut state, 7);
    push_u64(&mut state, 3);
    push_vector(&mut vectors, "state.delta_remove.payload", &state);

    let mut connect = Vec::new();
    push_u64(&mut connect, 7);
    push_u64(&mut connect, 3);
    push_u64(&mut connect, 4096);
    push_u16(&mut connect, 0);
    push_u16(&mut connect, 0);
    push_u32(&mut connect, 0);
    push_vector(&mut vectors, "relay.connect.payload", &connect);

    let mut fetch = Vec::new();
    push_u64(&mut fetch, 9);
    fetch.extend_from_slice(&[0xaa; 32]);
    push_u64(&mut fetch, 4096);
    push_u32(&mut fetch, 0);
    push_vector(&mut vectors, "font.fetch.payload", &fetch);

    // Terminal CREATE with an exact launch record (ARGV, cwd PATH, explicit
    // empty-base environment), plus the normative codec-1 byte-budget frame.
    let terminal_create = decode_hex(
        "18005000000000001111111111111111111111111111111137000000010101000200020000007368020000002d6c040000002f746d70020004004c414e4700010000004304005445524d01000000000000000000000000",
    );
    push_vector(&mut vectors, "terminal.create.payload", &terminal_create);
    let terminal_frame =
        decode_hex("0100000002000000080017004f000100fe0e01000800000000000078000000");
    push_vector(
        &mut vectors,
        "terminal.frame.byte_budget.payload",
        &terminal_frame,
    );
    let terminal_close_view = 7u32.to_le_bytes();
    push_vector(
        &mut vectors,
        "terminal.close_view.payload",
        &terminal_close_view,
    );
    let mut terminal_query_inline = vec![
        family_constant(artifact, "yas.terminal", "QUERY_INLINE") as u8,
        family_constant(artifact, "yas.terminal", "CONTENT_TEXT") as u8,
        family_constant(artifact, "yas.terminal", "QUERY_ENCODING_UTF8") as u8,
        0,
    ];
    push_u16(
        &mut terminal_query_inline,
        family_constant(artifact, "yas.terminal", "QUERY_TRUNCATED") as u16,
    );
    push_u16(&mut terminal_query_inline, 0);
    let mut terminal_query_extensions = Vec::new();
    push_u16(
        &mut terminal_query_extensions,
        family_constant(artifact, "yas.terminal", "QUERY_EXTENSION_INLINE_BYTES") as u16,
    );
    push_u16(&mut terminal_query_extensions, 1);
    push_bytes_u32(&mut terminal_query_extensions, b"hello");
    let mut terminal_query_cursor = Vec::new();
    terminal_query_cursor
        .push(family_constant(artifact, "yas.terminal", "READ_CURSOR_ABSOLUTE") as u8);
    push_u64(&mut terminal_query_cursor, 9);
    push_u32(&mut terminal_query_cursor, 2);
    push_u16(
        &mut terminal_query_extensions,
        family_constant(artifact, "yas.terminal", "QUERY_EXTENSION_NEXT_CURSOR") as u16,
    );
    push_u16(&mut terminal_query_extensions, 0);
    push_bytes_u32(&mut terminal_query_extensions, &terminal_query_cursor);
    push_u16(
        &mut terminal_query_extensions,
        family_constant(artifact, "yas.terminal", "QUERY_EXTENSION_TOTAL_LINES") as u16,
    );
    push_u16(&mut terminal_query_extensions, 0);
    push_u32(&mut terminal_query_extensions, 8);
    push_u64(&mut terminal_query_extensions, 12);
    push_bytes_u32(&mut terminal_query_inline, &terminal_query_extensions);
    push_vector(
        &mut vectors,
        "terminal.query_inline.payload",
        &terminal_query_inline,
    );

    let mut terminal_read = Vec::new();
    push_u64(&mut terminal_read, 1);
    push_u32(&mut terminal_read, 2);
    terminal_read.push(family_constant(artifact, "yas.terminal", "READ_CURSOR_ABSOLUTE") as u8);
    terminal_read
        .push(family_constant(artifact, "yas.terminal", "QUERY_REPRESENTATION_BOTH") as u8);
    push_u16(&mut terminal_read, 0);
    push_u64(&mut terminal_read, 3);
    push_u32(&mut terminal_read, 20);
    push_u32(&mut terminal_read, 4096);
    push_u64(&mut terminal_read, 8192);
    push_u32(&mut terminal_read, 0);
    push_vector(&mut vectors, "terminal.read.payload", &terminal_read);

    let mut terminal_search = Vec::new();
    push_u64(&mut terminal_search, 1);
    push_u32(&mut terminal_search, 2);
    push_u16(
        &mut terminal_search,
        (family_constant(artifact, "yas.terminal", "SEARCH_REGEX")
            | family_constant(artifact, "yas.terminal", "SEARCH_CASE_SENSITIVE")) as u16,
    );
    push_u16(&mut terminal_search, 0);
    terminal_search.push(family_constant(artifact, "yas.terminal", "SEARCH_CURSOR_POSITION") as u8);
    push_u64(&mut terminal_search, 3);
    push_u32(&mut terminal_search, 4);
    push_u32(&mut terminal_search, 10);
    push_bytes_u32(&mut terminal_search, b"foo");
    push_u64(&mut terminal_search, 8192);
    push_u32(&mut terminal_search, 0);
    push_vector(&mut vectors, "terminal.search.payload", &terminal_search);

    let mut terminal_cwd = Vec::new();
    push_u64(&mut terminal_cwd, 1);
    push_u32(&mut terminal_cwd, 2);
    push_u32(&mut terminal_cwd, 0);
    push_u64(&mut terminal_cwd, 8192);
    push_u32(&mut terminal_cwd, 0);
    push_vector(&mut vectors, "terminal.cwd.payload", &terminal_cwd);

    let mut terminal_journal = Vec::new();
    push_u64(&mut terminal_journal, 1);
    push_u32(&mut terminal_journal, 2);
    push_u16(
        &mut terminal_journal,
        family_constant(artifact, "yas.terminal", "JOURNAL_TAIL") as u16,
    );
    push_u16(&mut terminal_journal, 10);
    push_u64(&mut terminal_journal, 2);
    push_u64(&mut terminal_journal, 8192);
    push_u32(&mut terminal_journal, 0);
    push_vector(&mut vectors, "terminal.journal.payload", &terminal_journal);

    let mut terminal_output = Vec::new();
    push_u64(&mut terminal_output, 1);
    push_u32(&mut terminal_output, 2);
    terminal_output.push(family_constant(artifact, "yas.terminal", "OUTPUT_CURSOR_SEQUENCE") as u8);
    terminal_output.push(0);
    push_u16(&mut terminal_output, 0);
    push_u64(&mut terminal_output, 7);
    push_u32(&mut terminal_output, 3);
    push_u32(&mut terminal_output, 4096);
    push_u64(&mut terminal_output, 8192);
    push_u32(&mut terminal_output, 0);
    push_vector(&mut vectors, "terminal.output.payload", &terminal_output);

    let mut terminal_wait = Vec::new();
    push_u64(&mut terminal_wait, 1);
    push_u32(&mut terminal_wait, 2);
    terminal_wait.push(family_constant(artifact, "yas.terminal", "WAIT_OUTPUT") as u8);
    terminal_wait.push(0);
    push_u16(&mut terminal_wait, 0);
    push_u64(&mut terminal_wait, 7);
    push_u32(&mut terminal_wait, 3);
    push_u32(&mut terminal_wait, 4096);
    push_u64(&mut terminal_wait, 1_000_000_000);
    push_bytes_u32(&mut terminal_wait, b"ready");
    push_u64(&mut terminal_wait, 8192);
    push_u32(&mut terminal_wait, 0);
    push_vector(&mut vectors, "terminal.wait.payload", &terminal_wait);

    let mut terminal_copy_range = Vec::new();
    push_u64(&mut terminal_copy_range, 1);
    push_u32(&mut terminal_copy_range, 2);
    terminal_copy_range.push(family_constant(
        artifact,
        "yas.terminal",
        "QUERY_REPRESENTATION_STYLED",
    ) as u8);
    terminal_copy_range.extend_from_slice(&[0; 3]);
    push_i64(&mut terminal_copy_range, -2);
    push_u32(&mut terminal_copy_range, 4);
    push_i64(&mut terminal_copy_range, -1);
    push_u32(&mut terminal_copy_range, 8);
    push_u32(&mut terminal_copy_range, 4096);
    push_u64(&mut terminal_copy_range, 8192);
    push_u32(&mut terminal_copy_range, 0);
    push_vector(
        &mut vectors,
        "terminal.copy_range.payload",
        &terminal_copy_range,
    );

    let mut terminal_search_results = Vec::new();
    push_u32(&mut terminal_search_results, 1);
    push_u64(&mut terminal_search_results, 3);
    push_u32(&mut terminal_search_results, 4);
    push_u64(&mut terminal_search_results, 3);
    push_u32(&mut terminal_search_results, 7);
    push_bytes_u32(&mut terminal_search_results, b"foo");
    push_vector(
        &mut vectors,
        "terminal.search_results.payload",
        &terminal_search_results,
    );

    let mut terminal_journal_result = Vec::new();
    push_u64(&mut terminal_journal_result, 4);
    push_u64(&mut terminal_journal_result, 5);
    push_u32(&mut terminal_journal_result, 1);
    push_u64(&mut terminal_journal_result, 4);
    push_u32(&mut terminal_journal_result, 1);
    push_u16(
        &mut terminal_journal_result,
        family_constant(artifact, "yas.terminal", "JOURNAL_NO_COMMAND") as u16,
    );
    push_u16(&mut terminal_journal_result, 0);
    push_u32(&mut terminal_journal_result, 0);
    push_u64(&mut terminal_journal_result, 0);
    push_u64(&mut terminal_journal_result, 0);
    push_u64(&mut terminal_journal_result, 0);
    push_u64(&mut terminal_journal_result, 0);
    push_u32(&mut terminal_journal_result, 0);
    push_vector(
        &mut vectors,
        "terminal.journal_result.payload",
        &terminal_journal_result,
    );

    let mut terminal_output_result = Vec::new();
    push_u32(&mut terminal_output_result, 2);
    push_u16(
        &mut terminal_output_result,
        family_constant(artifact, "yas.terminal", "OUTPUT_MATCHED") as u16,
    );
    push_u16(&mut terminal_output_result, 0);
    push_u64(&mut terminal_output_result, 7);
    push_u32(&mut terminal_output_result, 3);
    push_u64(&mut terminal_output_result, 8);
    push_u32(&mut terminal_output_result, 1);
    push_bytes_u32(&mut terminal_output_result, b"ready");
    push_vector(
        &mut vectors,
        "terminal.output_result.payload",
        &terminal_output_result,
    );

    let mut terminal_styled_lines = Vec::new();
    push_u32(&mut terminal_styled_lines, 1);
    push_i64(&mut terminal_styled_lines, -1);
    push_u32(&mut terminal_styled_lines, 5);
    push_u32(&mut terminal_styled_lines, 1);
    terminal_styled_lines.extend_from_slice(&[0; 12]);
    push_u32(&mut terminal_styled_lines, 0);
    push_u32(&mut terminal_styled_lines, 0);
    push_vector(
        &mut vectors,
        "terminal.styled_lines.payload",
        &terminal_styled_lines,
    );

    let mut terminal_text_and_styled = Vec::new();
    push_bytes_u32(&mut terminal_text_and_styled, b"x");
    push_bytes_u32(&mut terminal_text_and_styled, &terminal_styled_lines);
    push_vector(
        &mut vectors,
        "terminal.text_and_styled.payload",
        &terminal_text_and_styled,
    );

    let mut client_disconnect = Vec::new();
    client_disconnect.extend_from_slice(&[1; 16]);
    client_disconnect.extend_from_slice(&[2; 16]);
    push_u32(&mut client_disconnect, 3);
    client_disconnect.extend_from_slice(b"bye");
    push_vector(
        &mut vectors,
        "client.disconnect.payload",
        &client_disconnect,
    );

    let mut client_bandwidth_rates = Vec::new();
    push_u64(&mut client_bandwidth_rates, 1_000);
    push_u64(&mut client_bandwidth_rates, 2_000);
    push_u64(&mut client_bandwidth_rates, 500_000_000);
    push_vector(
        &mut vectors,
        "client.bandwidth_rates.payload",
        &client_bandwidth_rates,
    );

    let mut surface_create_app_endpoint = vec![1; 16];
    push_bytes_u16(&mut surface_create_app_endpoint, b"demo.app");
    push_u32(&mut surface_create_app_endpoint, 0);
    push_vector(
        &mut vectors,
        "surface.create_app_endpoint.payload",
        &surface_create_app_endpoint,
    );
    let mut surface_create_app_endpoint_result = Vec::new();
    push_u64(&mut surface_create_app_endpoint_result, 7);
    push_u64(
        &mut surface_create_app_endpoint_result,
        0x0102_0304_0506_0708,
    );
    push_u16(&mut surface_create_app_endpoint_result, 1);
    push_bytes_u16(&mut surface_create_app_endpoint_result, b"WAYLAND_DISPLAY");
    push_bytes_u32(&mut surface_create_app_endpoint_result, b"wayland-yas");
    push_u32(&mut surface_create_app_endpoint_result, 0);
    push_vector(
        &mut vectors,
        "surface.create_app_endpoint_result.payload",
        &surface_create_app_endpoint_result,
    );
    let mut surface_release_app_endpoint = Vec::new();
    push_u64(&mut surface_release_app_endpoint, 7);
    surface_release_app_endpoint.extend_from_slice(&[2; 16]);
    push_u32(&mut surface_release_app_endpoint, 0);
    push_vector(
        &mut vectors,
        "surface.release_app_endpoint.payload",
        &surface_release_app_endpoint,
    );

    let mut surface_open_view = Vec::new();
    push_u64(&mut surface_open_view, 1);
    push_u32(&mut surface_open_view, 1920);
    push_u32(&mut surface_open_view, 1080);
    push_u16(&mut surface_open_view, 60);
    surface_open_view.push(3);
    surface_open_view.push(2);
    push_u16(&mut surface_open_view, 1);
    push_u16(&mut surface_open_view, 2);
    push_u32(&mut surface_open_view, 0);
    push_vector(
        &mut vectors,
        "surface.open_view.payload",
        &surface_open_view,
    );
    let mut surface_remote_input = Vec::new();
    push_u64(&mut surface_remote_input, 1);
    push_u64(&mut surface_remote_input, 2);
    push_u64(&mut surface_remote_input, 3);
    surface_remote_input
        .push(family_constant(artifact, "yas.surface", "REMOTE_INPUT_POINTER") as u8);
    surface_remote_input.push(0);
    push_u16(&mut surface_remote_input, 1);
    push_u32(&mut surface_remote_input, 0);
    push_i64(&mut surface_remote_input, 4);
    push_i64(&mut surface_remote_input, -5);
    push_vector(
        &mut vectors,
        "surface.remote_input.payload",
        &surface_remote_input,
    );

    let mut selection_drag_get = Vec::new();
    selection_drag_get.push(family_constant(artifact, "yas.selection", "GET_TARGET_DRAG") as u8);
    selection_drag_get.extend_from_slice(&[0; 3]);
    push_u64(&mut selection_drag_get, 7);
    push_u64(&mut selection_drag_get, 3);
    push_u16(&mut selection_drag_get, 2);
    push_u16(&mut selection_drag_get, 0);
    push_u64(&mut selection_drag_get, 4096);
    push_bytes_u16(&mut selection_drag_get, b"text/plain");
    push_u32(&mut selection_drag_get, 0);
    push_vector(
        &mut vectors,
        "selection.drag_get.payload",
        &selection_drag_get,
    );
    let mut selection_drag_drop = Vec::new();
    push_u64(&mut selection_drag_drop, 7);
    push_u64(&mut selection_drag_drop, 3);
    selection_drag_drop.extend_from_slice(&[4; 16]);
    push_u16(
        &mut selection_drag_drop,
        family_constant(artifact, "yas.selection", "ACTION_COPY") as u16,
    );
    push_u16(&mut selection_drag_drop, 0);
    push_u32(&mut selection_drag_drop, 16);
    push_u16(
        &mut selection_drag_drop,
        family_constant(artifact, "yas.selection", "DRAG_DROP_ITEMS_EXTENSION") as u16,
    );
    push_u16(&mut selection_drag_drop, 1);
    push_u32(&mut selection_drag_drop, 8);
    push_u16(&mut selection_drag_drop, 1);
    push_bytes_u16(&mut selection_drag_drop, b"a");
    push_bytes_u16(&mut selection_drag_drop, b"b");
    push_vector(
        &mut vectors,
        "selection.drag_drop.payload",
        &selection_drag_drop,
    );

    let mut desktop_fetch_asset = Vec::new();
    desktop_fetch_asset.extend_from_slice(&[0xaa; 32]);
    push_u64(&mut desktop_fetch_asset, 4096);
    push_u32(&mut desktop_fetch_asset, 0);
    push_vector(
        &mut vectors,
        "desktop.fetch_asset.payload",
        &desktop_fetch_asset,
    );

    let mut desktop_tray_action = Vec::new();
    push_u64(&mut desktop_tray_action, 1);
    push_u64(&mut desktop_tray_action, 2);
    push_u64(&mut desktop_tray_action, 3);
    desktop_tray_action.extend_from_slice(&[4; 16]);
    desktop_tray_action
        .push(family_constant(artifact, "yas.desktop", "TRAY_ACTION_MENU_ITEM") as u8);
    desktop_tray_action.push(0);
    push_u16(&mut desktop_tray_action, 0);
    push_i32(&mut desktop_tray_action, 0);
    push_u64(&mut desktop_tray_action, 5);
    push_u32(&mut desktop_tray_action, 0);
    push_vector(
        &mut vectors,
        "desktop.tray_action.payload",
        &desktop_tray_action,
    );

    let mut desktop_notification_action = Vec::new();
    push_u64(&mut desktop_notification_action, 6);
    push_u64(&mut desktop_notification_action, 7);
    desktop_notification_action.push(family_constant(
        artifact,
        "yas.desktop",
        "NOTIFICATION_ACTION_ACTION",
    ) as u8);
    desktop_notification_action.extend_from_slice(&[0; 3]);
    push_u64(&mut desktop_notification_action, 8);
    desktop_notification_action.extend_from_slice(&[9; 16]);
    push_bytes_u32(&mut desktop_notification_action, b"approved");
    push_u32(&mut desktop_notification_action, 0);
    push_vector(
        &mut vectors,
        "desktop.notification_action.payload",
        &desktop_notification_action,
    );

    let mut desktop_notification_extensions = Vec::new();
    push_u16(
        &mut desktop_notification_extensions,
        family_constant(artifact, "yas.desktop", "NOTIFICATION_IMAGE_HASH_EXTENSION") as u16,
    );
    push_u16(&mut desktop_notification_extensions, 0);
    push_u32(&mut desktop_notification_extensions, 32);
    desktop_notification_extensions.extend_from_slice(&[1; 32]);
    push_u16(
        &mut desktop_notification_extensions,
        family_constant(
            artifact,
            "yas.desktop",
            "NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION",
        ) as u16,
    );
    push_u16(&mut desktop_notification_extensions, 0);
    push_u32(&mut desktop_notification_extensions, 32);
    desktop_notification_extensions.extend_from_slice(&[2; 32]);
    push_u16(
        &mut desktop_notification_extensions,
        family_constant(artifact, "yas.desktop", "NOTIFICATION_PROGRESS_EXTENSION") as u16,
    );
    push_u16(&mut desktop_notification_extensions, 0);
    push_u32(&mut desktop_notification_extensions, 8);
    push_u32(&mut desktop_notification_extensions, 7);
    push_u32(&mut desktop_notification_extensions, 10);
    let mut desktop_reply = Vec::new();
    push_bytes_u16(&mut desktop_reply, b"Reply");
    push_u16(
        &mut desktop_notification_extensions,
        family_constant(artifact, "yas.desktop", "NOTIFICATION_REPLY_EXTENSION") as u16,
    );
    push_u16(&mut desktop_notification_extensions, 0);
    push_bytes_u32(&mut desktop_notification_extensions, &desktop_reply);

    let mut desktop_notification_record = Vec::new();
    push_u64(&mut desktop_notification_record, 5);
    push_u64(&mut desktop_notification_record, 6);
    push_u16(
        &mut desktop_notification_record,
        (family_constant(artifact, "yas.desktop", "NOTIFICATION_RESIDENT")
            | family_constant(artifact, "yas.desktop", "NOTIFICATION_HAS_REPLY")
            | family_constant(artifact, "yas.desktop", "NOTIFICATION_HAS_PROGRESS")) as u16,
    );
    desktop_notification_record.push(family_constant(
        artifact,
        "yas.desktop",
        "NOTIFICATION_URGENCY_NORMAL",
    ) as u8);
    desktop_notification_record.push(0);
    push_u64(&mut desktop_notification_record, 99);
    push_bytes_u16(&mut desktop_notification_record, b"sync");
    push_bytes_u16(&mut desktop_notification_record, b"Uploading");
    push_bytes_u32(&mut desktop_notification_record, b"payload");
    push_u16(&mut desktop_notification_record, 1);
    push_u64(&mut desktop_notification_record, 8);
    push_bytes_u16(&mut desktop_notification_record, b"Cancel");
    push_bytes_u32(
        &mut desktop_notification_record,
        &desktop_notification_extensions,
    );
    push_vector(
        &mut vectors,
        "desktop.notification_record.payload",
        &desktop_notification_record,
    );

    let mut desktop_patch_extensions = Vec::new();
    push_u16(
        &mut desktop_patch_extensions,
        family_constant(artifact, "yas.desktop", "NOTIFICATION_IMAGE_HASH_EXTENSION") as u16,
    );
    push_u16(&mut desktop_patch_extensions, 0);
    push_u32(&mut desktop_patch_extensions, 0);
    push_u16(
        &mut desktop_patch_extensions,
        family_constant(
            artifact,
            "yas.desktop",
            "NOTIFICATION_APPLICATION_ICON_HASH_EXTENSION",
        ) as u16,
    );
    push_u16(&mut desktop_patch_extensions, 0);
    push_u32(&mut desktop_patch_extensions, 32);
    desktop_patch_extensions.extend_from_slice(&[3; 32]);
    push_u16(
        &mut desktop_patch_extensions,
        family_constant(artifact, "yas.desktop", "NOTIFICATION_PROGRESS_EXTENSION") as u16,
    );
    push_u16(&mut desktop_patch_extensions, 0);
    push_u32(&mut desktop_patch_extensions, 8);
    push_u32(&mut desktop_patch_extensions, 9);
    push_u32(&mut desktop_patch_extensions, 10);
    push_u16(
        &mut desktop_patch_extensions,
        family_constant(artifact, "yas.desktop", "NOTIFICATION_REPLY_EXTENSION") as u16,
    );
    push_u16(&mut desktop_patch_extensions, 0);
    push_u32(&mut desktop_patch_extensions, 0);
    let mut desktop_notification_patch = Vec::new();
    push_u16(
        &mut desktop_notification_patch,
        family_constant(artifact, "yas.desktop", "RECORD_NOTIFICATION") as u16,
    );
    push_u16(&mut desktop_notification_patch, 0);
    push_u64(&mut desktop_notification_patch, 5);
    push_u64(&mut desktop_notification_patch, 7);
    push_bytes_u32(&mut desktop_notification_patch, &desktop_patch_extensions);
    push_vector(
        &mut vectors,
        "desktop.notification_patch.payload",
        &desktop_notification_patch,
    );

    let mut desktop_notification_remove = Vec::new();
    push_u16(
        &mut desktop_notification_remove,
        family_constant(artifact, "yas.desktop", "RECORD_NOTIFICATION") as u16,
    );
    push_u16(&mut desktop_notification_remove, 0);
    push_u64(&mut desktop_notification_remove, 3);
    push_u64(&mut desktop_notification_remove, 4);
    desktop_notification_remove.push(family_constant(
        artifact,
        "yas.desktop",
        "NOTIFICATION_CLOSED_DISMISSED",
    ) as u8);
    desktop_notification_remove.extend_from_slice(&[0; 3]);
    push_vector(
        &mut vectors,
        "desktop.notification_remove.payload",
        &desktop_notification_remove,
    );

    let mut media_fetch_asset = Vec::new();
    media_fetch_asset.extend_from_slice(&[0xbb; 32]);
    push_u64(&mut media_fetch_asset, 8192);
    push_u32(&mut media_fetch_asset, 0);
    push_vector(
        &mut vectors,
        "media.fetch_asset.payload",
        &media_fetch_asset,
    );

    let mut media_choice_option = Vec::new();
    push_bytes_u16(&mut media_choice_option, b"yes");
    push_bytes_u16(&mut media_choice_option, b"Yes");
    let mut media_choice = Vec::new();
    push_bytes_u16(&mut media_choice, b"remember");
    push_bytes_u16(&mut media_choice, b"Remember");
    push_bytes_u16(&mut media_choice, b"yes");
    push_u16(&mut media_choice, 1);
    push_u16(&mut media_choice, 0);
    push_bytes_u32(&mut media_choice, &media_choice_option);
    let mut media_access_metadata = Vec::new();
    push_u64(&mut media_access_metadata, 10);
    push_u64(&mut media_access_metadata, 9);
    push_bytes_u16(&mut media_access_metadata, b"app");
    push_bytes_u16(&mut media_access_metadata, b"Permission");
    push_bytes_u16(&mut media_access_metadata, b"");
    push_bytes_u32(&mut media_access_metadata, b"Allow?");
    push_bytes_u16(&mut media_access_metadata, b"Deny");
    push_bytes_u16(&mut media_access_metadata, b"Allow");
    push_bytes_u16(&mut media_access_metadata, b"app");
    push_u16(&mut media_access_metadata, 1);
    push_u16(&mut media_access_metadata, 0);
    push_bytes_u32(&mut media_access_metadata, &media_choice);
    let mut media_access_request = Vec::new();
    push_u64(&mut media_access_request, 5);
    push_u64(&mut media_access_request, 6);
    push_u16(
        &mut media_access_request,
        family_constant(artifact, "yas.media", "PORTAL_KIND_ACCESS") as u16,
    );
    push_u16(&mut media_access_request, 0);
    push_u64(&mut media_access_request, 8);
    push_bytes_u32(&mut media_access_request, &media_access_metadata);
    push_u32(&mut media_access_request, 0);
    push_vector(
        &mut vectors,
        "media.portal_access_request.payload",
        &media_access_request,
    );

    let mut media_access_grant = Vec::new();
    push_u16(&mut media_access_grant, 1);
    push_u16(&mut media_access_grant, 0);
    let mut media_chosen = Vec::new();
    push_bytes_u16(&mut media_chosen, b"remember");
    push_bytes_u16(&mut media_chosen, b"yes");
    push_bytes_u32(&mut media_access_grant, &media_chosen);
    let mut media_access_reply = Vec::new();
    push_u64(&mut media_access_reply, 5);
    push_u64(&mut media_access_reply, 6);
    media_access_reply.extend_from_slice(&[7; 16]);
    push_u16(
        &mut media_access_reply,
        family_constant(artifact, "yas.media", "PORTAL_KIND_ACCESS") as u16,
    );
    media_access_reply.push(family_constant(artifact, "yas.media", "PORTAL_DECISION_GRANT") as u8);
    media_access_reply.push(0);
    push_bytes_u32(&mut media_access_reply, &media_access_grant);
    push_u32(&mut media_access_reply, 0);
    push_vector(
        &mut vectors,
        "media.portal_access_reply.payload",
        &media_access_reply,
    );

    let mut media_candidate = Vec::new();
    push_u64(&mut media_candidate, 11);
    push_u32(&mut media_candidate, 800);
    push_u32(&mut media_candidate, 600);
    push_bytes_u16(&mut media_candidate, b"Window");
    push_bytes_u16(&mut media_candidate, b"browser");
    media_candidate.push(1);
    media_candidate.extend_from_slice(&[0; 3]);
    media_candidate.extend_from_slice(&[3; 32]);
    let mut media_screencast_metadata = Vec::new();
    push_u64(&mut media_screencast_metadata, 20);
    push_u64(&mut media_screencast_metadata, 0);
    push_bytes_u16(&mut media_screencast_metadata, b"meet");
    media_screencast_metadata.push(1);
    media_screencast_metadata.extend_from_slice(&[0; 3]);
    push_u16(&mut media_screencast_metadata, 1);
    push_u16(&mut media_screencast_metadata, 0);
    push_bytes_u32(&mut media_screencast_metadata, &media_candidate);
    let mut media_screencast_request = Vec::new();
    push_u64(&mut media_screencast_request, 10);
    push_u64(&mut media_screencast_request, 1);
    push_u16(
        &mut media_screencast_request,
        family_constant(artifact, "yas.media", "PORTAL_KIND_SCREENCAST") as u16,
    );
    push_u16(&mut media_screencast_request, 0);
    push_u64(&mut media_screencast_request, 8);
    push_bytes_u32(&mut media_screencast_request, &media_screencast_metadata);
    push_u32(&mut media_screencast_request, 0);
    push_vector(
        &mut vectors,
        "media.portal_screencast_request.payload",
        &media_screencast_request,
    );

    let mut media_screencast_grant = Vec::new();
    push_u16(&mut media_screencast_grant, 1);
    push_u16(&mut media_screencast_grant, 0);
    push_u64(&mut media_screencast_grant, 11);
    let mut media_screencast_reply = Vec::new();
    push_u64(&mut media_screencast_reply, 10);
    push_u64(&mut media_screencast_reply, 1);
    media_screencast_reply.extend_from_slice(&[8; 16]);
    push_u16(
        &mut media_screencast_reply,
        family_constant(artifact, "yas.media", "PORTAL_KIND_SCREENCAST") as u16,
    );
    media_screencast_reply
        .push(family_constant(artifact, "yas.media", "PORTAL_DECISION_GRANT") as u8);
    media_screencast_reply.push(0);
    push_bytes_u32(&mut media_screencast_reply, &media_screencast_grant);
    push_u32(&mut media_screencast_reply, 0);
    push_vector(
        &mut vectors,
        "media.portal_screencast_reply.payload",
        &media_screencast_reply,
    );

    let mut media_portal_close = Vec::new();
    push_u64(&mut media_portal_close, 10);
    push_u64(&mut media_portal_close, 2);
    media_portal_close.extend_from_slice(&[10; 16]);
    push_u32(&mut media_portal_close, 0);
    push_vector(
        &mut vectors,
        "media.portal_close.payload",
        &media_portal_close,
    );

    let mut media_granted_metadata = Vec::new();
    push_u16(&mut media_granted_metadata, 1);
    push_u16(&mut media_granted_metadata, 0);
    push_u64(&mut media_granted_metadata, 11);
    push_u64(&mut media_granted_metadata, 12);
    let mut media_portal_granted = Vec::new();
    push_u64(&mut media_portal_granted, 10);
    push_u64(&mut media_portal_granted, 2);
    push_u16(
        &mut media_portal_granted,
        family_constant(artifact, "yas.media", "PORTAL_KIND_SCREENCAST") as u16,
    );
    push_u16(
        &mut media_portal_granted,
        family_constant(artifact, "yas.media", "PORTAL_GRANTED") as u16,
    );
    media_portal_granted.extend_from_slice(&[11; 16]);
    push_bytes_u32(&mut media_portal_granted, &media_granted_metadata);
    push_u32(&mut media_portal_granted, 0);
    push_vector(
        &mut vectors,
        "media.portal_granted.payload",
        &media_portal_granted,
    );

    let mut env_get = Vec::new();
    push_u64(&mut env_get, 64 * 1024);
    push_u32(&mut env_get, 0);
    push_vector(&mut vectors, "env.get.payload", &env_get);

    // Environment keys and values are raw bytes, not UTF-8 strings. Keep a
    // non-UTF-8 entry in the shared vector so language implementations cannot
    // accidentally normalize the snapshot.
    let mut env_entries = Vec::new();
    push_bytes_u16(&mut env_entries, b"A");
    push_bytes_u32(&mut env_entries, &[0xfe, b'=']);
    push_bytes_u16(&mut env_entries, &[0xff]);
    push_bytes_u32(&mut env_entries, &[]);

    let mut env_inline = Vec::new();
    env_inline.push(family_constant(artifact, "yas.env", "DELIVERY_INLINE") as u8);
    env_inline.extend_from_slice(&[0; 3]);
    push_u32(&mut env_inline, 2);
    push_u64(&mut env_inline, 4);
    env_inline.extend_from_slice(&env_entries);
    push_u32(&mut env_inline, 0);
    push_vector(&mut vectors, "env.inline.payload", &env_inline);

    let mut env_descriptor = Vec::new();
    push_u32(&mut env_descriptor, 2);
    env_descriptor.push(family_constant(artifact, "yas.transfer", "MODE_MESSAGE") as u8);
    env_descriptor
        .push(family_constant(artifact, "yas.transfer", "DIRECTION_SENDER_TO_RECEIVER") as u8);
    push_u16(&mut env_descriptor, 0);
    push_u64(&mut env_descriptor, 0);
    push_u64(&mut env_descriptor, 64 * 1024);
    push_u64(
        &mut env_descriptor,
        family_constant(artifact, "yas.env", "MAX_BATCH_BYTES"),
    );
    push_u32(&mut env_descriptor, artifact.transport.limits.bulk_chunk);
    push_u16(&mut env_descriptor, family_id(artifact, "yas.env"));
    push_u16(
        &mut env_descriptor,
        family_constant(artifact, "yas.env", "SNAPSHOT_CONTENT_KIND") as u16,
    );
    push_u16(&mut env_descriptor, family_version(artifact, "yas.env"));
    // Required, zero-length SENSITIVE_CONTENT Transfer extension.
    push_u32(&mut env_descriptor, 8);
    push_u16(
        &mut env_descriptor,
        family_constant(artifact, "yas.transfer", "SENSITIVE_CONTENT_EXTENSION") as u16,
    );
    push_u16(&mut env_descriptor, 1);
    push_u32(&mut env_descriptor, 0);

    let mut env_transfer = Vec::new();
    env_transfer.push(family_constant(artifact, "yas.env", "DELIVERY_TRANSFER") as u8);
    env_transfer.extend_from_slice(&[0; 3]);
    push_u32(&mut env_transfer, 2);
    push_u64(&mut env_transfer, 4);
    push_u32(&mut env_transfer, env_descriptor.len() as u32);
    env_transfer.extend_from_slice(&env_descriptor);
    push_u32(&mut env_transfer, 0);
    push_vector(&mut vectors, "env.transfer.payload", &env_transfer);

    let mut env_batch = Vec::new();
    push_u32(&mut env_batch, 0);
    push_u16(&mut env_batch, 2);
    push_u16(&mut env_batch, 0);
    env_batch.extend_from_slice(&env_entries);
    push_vector(&mut vectors, "env.batch.payload", &env_batch);

    let mut kv_open = Vec::new();
    push_bytes_u16(&mut kv_open, b"app/");
    push_u32(&mut kv_open, 0);
    push_vector(&mut vectors, "kv.open.payload", &kv_open);

    let mut kv_watch = Vec::new();
    push_u64(&mut kv_watch, 1);
    push_u32(&mut kv_watch, 1024);
    push_u32(&mut kv_watch, 0);
    let mut kv_state_watch = Vec::new();
    push_u16(&mut kv_state_watch, 0);
    push_u16(&mut kv_state_watch, 0);
    push_u64(&mut kv_state_watch, 4096);
    push_u32(&mut kv_state_watch, 0);
    push_u32(&mut kv_watch, kv_state_watch.len() as u32);
    kv_watch.extend_from_slice(&kv_state_watch);
    push_vector(&mut vectors, "kv.watch.payload", &kv_watch);

    let mut kv_entry = Vec::new();
    push_bytes_u16(&mut kv_entry, b"k");
    kv_entry.extend_from_slice(&[0x11; 32]);
    push_u64(&mut kv_entry, 2);
    push_u64(&mut kv_entry, 3);
    push_i64(&mut kv_entry, 1_700_000_000_000_000_003);
    kv_entry.push(family_constant(artifact, "yas.kv", "CONTENT_INLINE") as u8);
    kv_entry.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut kv_entry, &[0xff, 0]);
    push_u32(&mut kv_entry, 0);
    push_vector(&mut vectors, "kv.entry.inline.payload", &kv_entry);

    let mut kv_download_descriptor = Vec::new();
    push_u32(&mut kv_download_descriptor, 2);
    kv_download_descriptor.push(family_constant(artifact, "yas.transfer", "MODE_BYTE") as u8);
    kv_download_descriptor.push(family_constant(
        artifact,
        "yas.transfer",
        "DIRECTION_SENDER_TO_RECEIVER",
    ) as u8);
    push_u16(&mut kv_download_descriptor, 0);
    push_u64(&mut kv_download_descriptor, 0);
    push_u64(&mut kv_download_descriptor, 64 * 1024);
    push_u64(&mut kv_download_descriptor, 0);
    push_u32(
        &mut kv_download_descriptor,
        artifact.transport.limits.bulk_chunk,
    );
    push_u16(&mut kv_download_descriptor, family_id(artifact, "yas.kv"));
    push_u16(
        &mut kv_download_descriptor,
        family_constant(artifact, "yas.kv", "VALUE_CONTENT_KIND") as u16,
    );
    push_u16(
        &mut kv_download_descriptor,
        family_version(artifact, "yas.kv"),
    );
    push_u32(&mut kv_download_descriptor, 8);
    push_u16(
        &mut kv_download_descriptor,
        family_constant(artifact, "yas.transfer", "SENSITIVE_CONTENT_EXTENSION") as u16,
    );
    push_u16(&mut kv_download_descriptor, 1);
    push_u32(&mut kv_download_descriptor, 0);

    let mut kv_get_transfer = Vec::new();
    push_u64(&mut kv_get_transfer, 3);
    kv_get_transfer.push(family_constant(artifact, "yas.transfer", "DELIVERY_TRANSFER") as u8);
    kv_get_transfer.extend_from_slice(&[0; 3]);
    push_u64(&mut kv_get_transfer, 2);
    kv_get_transfer.extend_from_slice(&[0x11; 32]);
    kv_get_transfer.extend_from_slice(&kv_download_descriptor);
    push_vector(&mut vectors, "kv.get.transfer.payload", &kv_get_transfer);

    let mut kv_upload_descriptor = Vec::new();
    push_u32(&mut kv_upload_descriptor, 4);
    kv_upload_descriptor.push(family_constant(artifact, "yas.transfer", "MODE_BYTE") as u8);
    kv_upload_descriptor.push(family_constant(
        artifact,
        "yas.transfer",
        "DIRECTION_RECEIVER_TO_SENDER",
    ) as u8);
    push_u16(&mut kv_upload_descriptor, 0);
    push_u64(&mut kv_upload_descriptor, 64 * 1024);
    push_u64(&mut kv_upload_descriptor, 0);
    push_u64(&mut kv_upload_descriptor, 0);
    push_u32(
        &mut kv_upload_descriptor,
        artifact.transport.limits.bulk_chunk,
    );
    push_u16(&mut kv_upload_descriptor, family_id(artifact, "yas.kv"));
    push_u16(
        &mut kv_upload_descriptor,
        family_constant(artifact, "yas.kv", "VALUE_CONTENT_KIND") as u16,
    );
    push_u16(
        &mut kv_upload_descriptor,
        family_version(artifact, "yas.kv"),
    );
    push_u32(&mut kv_upload_descriptor, 32);
    push_u16(
        &mut kv_upload_descriptor,
        family_constant(artifact, "yas.transfer", "SENSITIVE_CONTENT_EXTENSION") as u16,
    );
    push_u16(&mut kv_upload_descriptor, 1);
    push_u32(&mut kv_upload_descriptor, 0);
    push_u16(
        &mut kv_upload_descriptor,
        family_constant(artifact, "yas.transfer", "UPLOAD_STAGE_EXTENSION") as u16,
    );
    push_u16(&mut kv_upload_descriptor, 1);
    push_u32(&mut kv_upload_descriptor, 16);
    push_u64(&mut kv_upload_descriptor, 5);
    push_u64(&mut kv_upload_descriptor, 1);

    let mut kv_stage_result = Vec::new();
    push_u64(&mut kv_stage_result, 5);
    push_u64(&mut kv_stage_result, 2);
    kv_stage_result.extend_from_slice(&[0x11; 32]);
    kv_stage_result.extend_from_slice(&kv_upload_descriptor);
    push_vector(
        &mut vectors,
        "kv.stage_value.result.payload",
        &kv_stage_result,
    );

    let mut kv_put = Vec::new();
    push_u64(&mut kv_put, 1);
    kv_put.extend_from_slice(&[1; 16]);
    push_u16(
        &mut kv_put,
        family_constant(artifact, "yas.kv", "MUTATION_DURABLE") as u16,
    );
    push_u16(&mut kv_put, 0);
    push_bytes_u16(&mut kv_put, b"k");
    kv_put.push(family_constant(artifact, "yas.kv", "PRECONDITION_ABSENT") as u8);
    kv_put.extend_from_slice(&[0; 3]);
    kv_put.push(family_constant(artifact, "yas.kv", "VALUE_INLINE") as u8);
    kv_put.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut kv_put, &[0xff, 0]);
    push_u32(&mut kv_put, 0);
    push_vector(&mut vectors, "kv.put.inline.payload", &kv_put);

    let mut kv_mutation_result = Vec::new();
    push_u16(&mut kv_mutation_result, status_code(artifact, "OK"));
    push_u16(&mut kv_mutation_result, 0);
    push_u64(&mut kv_mutation_result, 9);
    push_i64(&mut kv_mutation_result, 1_700_000_000_000_000_009);
    kv_mutation_result.extend_from_slice(&[3; 32]);
    push_u64(&mut kv_mutation_result, 3);
    push_u32(&mut kv_mutation_result, 0);
    push_vector(
        &mut vectors,
        "kv.mutation_result.payload",
        &kv_mutation_result,
    );

    let mut kv_mutation = Vec::new();
    kv_mutation.push(family_constant(artifact, "yas.kv", "MUTATION_PUT") as u8);
    kv_mutation.extend_from_slice(&[0; 3]);
    push_bytes_u16(&mut kv_mutation, b"k");
    kv_mutation.push(family_constant(artifact, "yas.kv", "PRECONDITION_ANY") as u8);
    kv_mutation.extend_from_slice(&[0; 3]);
    kv_mutation.push(family_constant(artifact, "yas.kv", "VALUE_INLINE") as u8);
    kv_mutation.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut kv_mutation, b"v");
    push_u32(&mut kv_mutation, 0);
    let mut kv_batch = Vec::new();
    push_u64(&mut kv_batch, 1);
    kv_batch.extend_from_slice(&[2; 16]);
    push_u16(&mut kv_batch, 0);
    push_u16(&mut kv_batch, 1);
    push_bytes_u32(&mut kv_batch, &kv_mutation);
    push_u32(&mut kv_batch, 0);
    push_vector(&mut vectors, "kv.batch.payload", &kv_batch);

    let mut channel_listen = Vec::new();
    channel_listen.extend_from_slice(&[1; 16]);
    push_bytes_u16(&mut channel_listen, b"rpc.echo");
    push_bytes_u32(&mut channel_listen, b"schema-v1");
    push_u32(&mut channel_listen, 0);
    push_vector(&mut vectors, "channel.listen.payload", &channel_listen);

    let mut channel_listen_max_metadata = Vec::new();
    channel_listen_max_metadata.extend_from_slice(&[2; 16]);
    push_bytes_u16(&mut channel_listen_max_metadata, b"rpc.boundary");
    let channel_metadata =
        vec![0x5a; family_constant(artifact, "yas.channel", "MAX_METADATA_BYTES") as usize];
    push_bytes_u32(&mut channel_listen_max_metadata, &channel_metadata);
    push_u32(&mut channel_listen_max_metadata, 0);
    push_vector(
        &mut vectors,
        "channel.listen.max_metadata.payload",
        &channel_listen_max_metadata,
    );

    let mut channel_connect = Vec::new();
    push_u64(&mut channel_connect, 1);
    push_u64(&mut channel_connect, 2);
    push_u64(&mut channel_connect, 64 * 1024);
    push_bytes_u32(&mut channel_connect, b"connector");
    push_u32(&mut channel_connect, 0);
    push_vector(&mut vectors, "channel.connect.payload", &channel_connect);

    let mut channel_descriptor = Vec::new();
    push_u32(&mut channel_descriptor, 2);
    channel_descriptor.push(family_constant(artifact, "yas.transfer", "MODE_MESSAGE") as u8);
    channel_descriptor.push(
        (family_constant(artifact, "yas.transfer", "DIRECTION_RECEIVER_TO_SENDER")
            | family_constant(artifact, "yas.transfer", "DIRECTION_SENDER_TO_RECEIVER"))
            as u8,
    );
    push_u16(&mut channel_descriptor, 0);
    push_u64(&mut channel_descriptor, 64 * 1024);
    push_u64(&mut channel_descriptor, 0);
    push_u64(&mut channel_descriptor, 1024);
    push_u32(&mut channel_descriptor, 1024);
    push_u16(&mut channel_descriptor, family_id(artifact, "yas.channel"));
    push_u16(
        &mut channel_descriptor,
        family_constant(artifact, "yas.channel", "CHANNEL_CONTENT_KIND") as u16,
    );
    push_u16(
        &mut channel_descriptor,
        family_version(artifact, "yas.channel"),
    );
    push_u32(&mut channel_descriptor, 20);
    push_u16(
        &mut channel_descriptor,
        family_constant(artifact, "yas.transfer", "MAX_OPEN_MESSAGES_EXTENSION") as u16,
    );
    push_u16(&mut channel_descriptor, 0);
    push_u32(&mut channel_descriptor, 4);
    push_u32(&mut channel_descriptor, 2);
    push_u16(
        &mut channel_descriptor,
        family_constant(artifact, "yas.transfer", "SENSITIVE_CONTENT_EXTENSION") as u16,
    );
    push_u16(&mut channel_descriptor, 1);
    push_u32(&mut channel_descriptor, 0);

    let mut channel_accept = Vec::new();
    push_u64(&mut channel_accept, 1);
    push_u64(&mut channel_accept, 2);
    push_u64(&mut channel_accept, 3);
    push_u64(&mut channel_accept, 4);
    channel_accept.extend_from_slice(&[7; 16]);
    push_bytes_u32(&mut channel_accept, b"listener");
    push_bytes_u32(&mut channel_accept, b"connector");
    push_bytes_u32(&mut channel_accept, &channel_descriptor);
    push_u32(&mut channel_accept, 0);
    push_vector(&mut vectors, "channel.accept.payload", &channel_accept);

    let mut process_cwd = Vec::new();
    process_cwd.push(family_constant(artifact, "yas.process", "CWD_FS") as u8);
    process_cwd.extend_from_slice(&[0; 3]);
    push_u64(&mut process_cwd, 9);
    push_u16(&mut process_cwd, 2);
    push_bytes_u16(&mut process_cwd, b"src");
    push_bytes_u16(&mut process_cwd, &[0xff]);
    let mut process_spawn = Vec::new();
    process_spawn.extend_from_slice(&[1; 16]);
    push_u16(
        &mut process_spawn,
        family_constant(artifact, "yas.process", "SPAWN_DETACHABLE") as u16,
    );
    process_spawn.push(family_constant(artifact, "yas.process", "ENV_SESSION") as u8);
    process_spawn.push(0);
    push_bytes_u32(&mut process_spawn, &process_cwd);
    push_u16(&mut process_spawn, 2);
    push_bytes_u32(&mut process_spawn, b"/bin/raw");
    push_bytes_u32(&mut process_spawn, &[0xff, b'a', b'r', b'g']);
    push_u16(&mut process_spawn, 2);
    push_bytes_u16(&mut process_spawn, b"A");
    push_bytes_u32(&mut process_spawn, &[0xff]);
    push_bytes_u16(&mut process_spawn, b"Z");
    push_bytes_u32(&mut process_spawn, b"last");
    push_u64(&mut process_spawn, 4096);
    push_u64(&mut process_spawn, 2048);
    push_u32(&mut process_spawn, 28);
    push_u16(
        &mut process_spawn,
        family_constant(artifact, "yas.process", "SPAWN_SURFACE_APP_EXTENSION") as u16,
    );
    push_u16(&mut process_spawn, 0);
    push_u32(&mut process_spawn, 8);
    push_u64(&mut process_spawn, 11);
    push_u16(
        &mut process_spawn,
        family_constant(artifact, "yas.process", "SPAWN_RESOURCE_TAG_EXTENSION") as u16,
    );
    push_u16(&mut process_spawn, 0);
    push_u32(&mut process_spawn, 4);
    process_spawn.extend_from_slice(b"pane");
    push_vector(&mut vectors, "process.spawn.payload", &process_spawn);

    let process_stdin = sensitive_byte_descriptor(
        artifact,
        2,
        family_constant(artifact, "yas.transfer", "DIRECTION_RECEIVER_TO_SENDER") as u8,
        4096,
        0,
        (
            "yas.process",
            family_constant(artifact, "yas.process", "STREAM_STDIN_CONTENT_KIND") as u16,
        ),
        None,
    );
    let process_stdout = sensitive_byte_descriptor(
        artifact,
        4,
        family_constant(artifact, "yas.transfer", "DIRECTION_SENDER_TO_RECEIVER") as u8,
        0,
        4096,
        (
            "yas.process",
            family_constant(artifact, "yas.process", "STREAM_STDOUT_CONTENT_KIND") as u16,
        ),
        None,
    );
    let process_stderr = sensitive_byte_descriptor(
        artifact,
        6,
        family_constant(artifact, "yas.transfer", "DIRECTION_SENDER_TO_RECEIVER") as u8,
        0,
        2048,
        (
            "yas.process",
            family_constant(artifact, "yas.process", "STREAM_STDERR_CONTENT_KIND") as u16,
        ),
        None,
    );
    let mut process_bundle = Vec::new();
    push_u64(&mut process_bundle, 1);
    push_u16(
        &mut process_bundle,
        (family_constant(artifact, "yas.process", "BUNDLE_STDIN")
            | family_constant(artifact, "yas.process", "BUNDLE_STDOUT")
            | family_constant(artifact, "yas.process", "BUNDLE_STDERR")) as u16,
    );
    push_u16(&mut process_bundle, 0);
    push_u64(&mut process_bundle, 10);
    push_u64(&mut process_bundle, 20);
    push_bytes_u32(&mut process_bundle, &process_stdin);
    push_bytes_u32(&mut process_bundle, &process_stdout);
    push_bytes_u32(&mut process_bundle, &process_stderr);
    push_u32(&mut process_bundle, 0);
    push_vector(
        &mut vectors,
        "process.stream_bundle.payload",
        &process_bundle,
    );

    let mut process_exit = Vec::new();
    process_exit.push(family_constant(artifact, "yas.process", "EXIT_KIND_CODE") as u8);
    process_exit.push(family_constant(artifact, "yas.process", "EXIT_REASON_UNKNOWN") as u8);
    push_u16(&mut process_exit, 0);
    push_i32(&mut process_exit, 7);
    push_u64(&mut process_exit, 99);
    push_bytes_u32(&mut process_exit, b"");
    push_vector(&mut vectors, "process.exit.payload", &process_exit);

    let fs_path_a = fs_path(&[b"a"]);
    let fs_path_b = fs_path(&[b"b"]);
    let mut fs_source = Vec::new();
    fs_source.push(family_constant(artifact, "yas.fs", "SOURCE_PLATFORM_PATH") as u8);
    fs_source.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut fs_source, b"/tmp");
    let mut fs_open = Vec::new();
    push_u16(
        &mut fs_open,
        family_constant(artifact, "yas.fs", "OPEN_READ_ONLY") as u16,
    );
    push_u16(&mut fs_open, 0);
    push_bytes_u32(&mut fs_open, &fs_source);
    push_u32(&mut fs_open, 0);
    push_vector(&mut vectors, "fs.open.payload", &fs_open);

    let mut fs_close = Vec::new();
    push_u64(&mut fs_close, 1);
    push_u32(&mut fs_close, 0);
    push_vector(&mut vectors, "fs.close.payload", &fs_close);

    let mut fs_state_watch = Vec::new();
    push_u16(&mut fs_state_watch, 0);
    push_u16(&mut fs_state_watch, 0);
    push_u64(&mut fs_state_watch, 1024);
    push_u32(&mut fs_state_watch, 0);
    let mut fs_watch = Vec::new();
    push_u64(&mut fs_watch, 1);
    push_u16(
        &mut fs_watch,
        family_constant(artifact, "yas.fs", "WATCH_CONTENT") as u16,
    );
    push_u16(&mut fs_watch, 0);
    push_u32(&mut fs_watch, 3);
    push_bytes_u32(&mut fs_watch, b"target/\n!target/keep");
    push_bytes_u32(&mut fs_watch, &fs_state_watch);
    push_vector(&mut vectors, "fs.watch.payload", &fs_watch);
    push_vector(&mut vectors, "fs.unwatch.payload", &7u32.to_le_bytes());

    let mut fs_fetch = Vec::new();
    push_u64(&mut fs_fetch, 1);
    push_bytes_u32(&mut fs_fetch, &fs_path_a);
    fs_fetch.push(1);
    fs_fetch.extend_from_slice(&[0; 3]);
    fs_fetch.extend_from_slice(&[2; 32]);
    push_u64(&mut fs_fetch, 1024);
    push_u32(&mut fs_fetch, 0);
    push_vector(&mut vectors, "fs.fetch.payload", &fs_fetch);

    let mut fs_read = Vec::new();
    push_u64(&mut fs_read, 1);
    push_u64(&mut fs_read, 1024);
    push_u16(&mut fs_read, 1);
    push_u16(&mut fs_read, 0);
    push_u16(
        &mut fs_read,
        family_constant(artifact, "yas.fs", "READ_STAT") as u16,
    );
    push_u16(&mut fs_read, 0);
    push_bytes_u32(&mut fs_read, &fs_path_a);
    push_u32(&mut fs_read, 0);
    push_vector(&mut vectors, "fs.read.payload", &fs_read);

    let mut fs_search = Vec::new();
    push_u64(&mut fs_search, 1);
    push_u16(
        &mut fs_search,
        family_constant(artifact, "yas.fs", "SEARCH_CASE_SENSITIVE") as u16,
    );
    push_u16(&mut fs_search, 10);
    push_bytes_u16(&mut fs_search, b"src");
    push_bytes_u16(&mut fs_search, b"");
    push_u64(&mut fs_search, 1024);
    push_u32(&mut fs_search, 0);
    push_vector(&mut vectors, "fs.search.payload", &fs_search);

    let mut fs_index = Vec::new();
    push_u64(&mut fs_index, 1);
    push_u16(
        &mut fs_index,
        family_constant(artifact, "yas.fs", "INDEX_INCLUDE_FILES") as u16,
    );
    push_u16(&mut fs_index, 10);
    push_bytes_u16(&mut fs_index, b"");
    push_u64(&mut fs_index, 1024);
    push_u32(&mut fs_index, 0);
    push_vector(&mut vectors, "fs.index.payload", &fs_index);

    let mut fs_grep = Vec::new();
    push_u64(&mut fs_grep, 1);
    push_u16(
        &mut fs_grep,
        family_constant(artifact, "yas.fs", "GREP_REGEX") as u16,
    );
    push_u16(&mut fs_grep, 20);
    push_u16(&mut fs_grep, 5);
    push_u16(&mut fs_grep, 0);
    push_bytes_u32(&mut fs_grep, b"yas.*wire");
    push_bytes_u16(&mut fs_grep, b"");
    push_u64(&mut fs_grep, 1024);
    push_u32(&mut fs_grep, 0);
    push_vector(&mut vectors, "fs.grep.payload", &fs_grep);

    let mut fs_precondition = Vec::new();
    fs_precondition.push(family_constant(artifact, "yas.fs", "PRECONDITION_ABSENT") as u8);
    fs_precondition.extend_from_slice(&[0; 3]);
    let mut fs_stage = Vec::new();
    push_u64(&mut fs_stage, 1);
    push_bytes_u32(&mut fs_stage, &fs_path_a);
    push_bytes_u32(&mut fs_stage, &fs_precondition);
    push_u16(
        &mut fs_stage,
        family_constant(artifact, "yas.fs", "STAGE_CREATE_PARENTS") as u16,
    );
    push_u16(&mut fs_stage, 0);
    push_u32(&mut fs_stage, 0o644);
    push_u64(&mut fs_stage, 3);
    fs_stage.extend_from_slice(&[3; 32]);
    push_u64(&mut fs_stage, 1024);
    push_u32(&mut fs_stage, 0);
    push_vector(&mut vectors, "fs.stage_write.payload", &fs_stage);

    let mut fs_commit = Vec::new();
    push_u64(&mut fs_commit, 2);
    fs_commit.extend_from_slice(&[4; 16]);
    push_u16(
        &mut fs_commit,
        family_constant(artifact, "yas.fs", "COMMIT_SYNC_DATA") as u16,
    );
    push_u16(&mut fs_commit, 0);
    push_u32(&mut fs_commit, 0);
    push_vector(&mut vectors, "fs.commit.payload", &fs_commit);

    let mut fs_apply_item_body = Vec::new();
    push_bytes_u32(&mut fs_apply_item_body, &fs_path_a);
    push_bytes_u32(&mut fs_apply_item_body, &fs_precondition);
    push_u32(&mut fs_apply_item_body, 0o644);
    push_bytes_u32(&mut fs_apply_item_body, b"yas");
    let mut fs_apply = Vec::new();
    push_u64(&mut fs_apply, 1);
    fs_apply.extend_from_slice(&[5; 16]);
    push_u16(
        &mut fs_apply,
        family_constant(artifact, "yas.fs", "APPLY_ITEM_CREATE_PARENTS") as u16,
    );
    push_u16(&mut fs_apply, 1);
    push_u32(&mut fs_apply, 4 + fs_apply_item_body.len() as u32);
    push_u16(
        &mut fs_apply,
        family_constant(artifact, "yas.fs", "APPLY_WRITE_INLINE") as u16,
    );
    push_u16(&mut fs_apply, 0);
    fs_apply.extend_from_slice(&fs_apply_item_body);
    push_u32(&mut fs_apply, 0);
    push_vector(&mut vectors, "fs.apply.payload", &fs_apply);

    let mut fs_entry = Vec::new();
    push_bytes_u32(&mut fs_entry, &fs_path_a);
    push_u64(&mut fs_entry, 1);
    fs_entry.push(family_constant(artifact, "yas.fs", "ENTRY_FILE") as u8);
    fs_entry.push(0);
    push_u16(&mut fs_entry, 0);
    push_u32(&mut fs_entry, 0o644);
    push_i64(&mut fs_entry, 1);
    push_u64(&mut fs_entry, 3);
    fs_entry.extend_from_slice(&[6; 32]);
    fs_entry.push(family_constant(artifact, "yas.fs", "CONTENT_INLINE") as u8);
    fs_entry.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut fs_entry, b"yas");
    push_u32(&mut fs_entry, 0);
    push_vector(&mut vectors, "fs.entry.inline.payload", &fs_entry);

    let mut fs_query_path = Vec::new();
    push_bytes_u32(&mut fs_query_path, &fs_path_a);
    push_u16(&mut fs_query_path, 0);
    push_u16(&mut fs_query_path, 0);
    push_vector(&mut vectors, "fs.query.path_record.payload", &fs_query_path);

    let mut fs_query_read = Vec::new();
    push_u16(&mut fs_query_read, 0);
    push_u16(&mut fs_query_read, status_code(artifact, "OK"));
    fs_query_read.push(1);
    fs_query_read.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut fs_query_read, &fs_path_a);
    push_bytes_u32(&mut fs_query_read, b"yas");
    push_vector(&mut vectors, "fs.query.read_record.payload", &fs_query_read);

    let mut fs_query_grep_file = Vec::new();
    push_u32(&mut fs_query_grep_file, 0);
    push_u32(&mut fs_query_grep_file, 1);
    push_u16(
        &mut fs_query_grep_file,
        family_constant(artifact, "yas.fs", "QUERY_GREP_FILE_IGNORED") as u16,
    );
    push_u16(&mut fs_query_grep_file, 0);
    push_bytes_u32(&mut fs_query_grep_file, &fs_path_a);
    push_vector(
        &mut vectors,
        "fs.query.grep_file_record.payload",
        &fs_query_grep_file,
    );

    let mut fs_query_grep_match = Vec::new();
    push_u32(&mut fs_query_grep_match, 0);
    push_u32(&mut fs_query_grep_match, 3);
    push_u32(&mut fs_query_grep_match, 4);
    push_u32(&mut fs_query_grep_match, 3);
    push_u32(&mut fs_query_grep_match, 7);
    push_bytes_u32(&mut fs_query_grep_match, b"yas");
    push_vector(
        &mut vectors,
        "fs.query.grep_match_record.payload",
        &fs_query_grep_match,
    );

    let mut fs_record_stream = Vec::new();
    push_u32(&mut fs_record_stream, 4 + fs_query_path.len() as u32);
    push_u16(
        &mut fs_record_stream,
        family_constant(artifact, "yas.fs", "QUERY_RECORD_PATH") as u16,
    );
    push_u16(&mut fs_record_stream, 0);
    fs_record_stream.extend_from_slice(&fs_query_path);
    let mut fs_page = Vec::new();
    push_bytes_u16(&mut fs_page, b"");
    push_u64(&mut fs_page, 1);
    push_u16(
        &mut fs_page,
        family_constant(artifact, "yas.fs", "PAGE_TRUNCATED") as u16,
    );
    push_u16(&mut fs_page, 0);
    fs_page.push(family_constant(artifact, "yas.fs", "PAGE_INLINE") as u8);
    fs_page.extend_from_slice(&[0; 3]);
    push_u16(&mut fs_page, 1);
    push_u16(&mut fs_page, 0);
    push_bytes_u32(&mut fs_page, &fs_record_stream);
    push_u32(&mut fs_page, 0);
    push_vector(&mut vectors, "fs.query.inline.payload", &fs_page);

    let mut fs_query_batch = Vec::new();
    push_u32(&mut fs_query_batch, 0);
    push_u16(&mut fs_query_batch, 1);
    push_u16(&mut fs_query_batch, 0);
    push_bytes_u32(&mut fs_query_batch, &fs_record_stream);
    push_vector(&mut vectors, "fs.query.batch.payload", &fs_query_batch);

    let mut fs_conflict = Vec::new();
    push_bytes_u32(&mut fs_conflict, &fs_path_a);
    fs_conflict.push(1);
    fs_conflict.push(1);
    push_u16(&mut fs_conflict, 0);
    push_u64(&mut fs_conflict, 8);
    push_i64(&mut fs_conflict, 9);
    fs_conflict.extend_from_slice(&[10; 32]);
    push_vector(&mut vectors, "fs.conflict_detail.payload", &fs_conflict);

    let mut fs_commit_result = Vec::new();
    push_u64(&mut fs_commit_result, 1);
    push_u64(&mut fs_commit_result, 2);
    push_i64(&mut fs_commit_result, 3);
    fs_commit_result.extend_from_slice(&[4; 32]);
    push_vector(&mut vectors, "fs.commit_result.payload", &fs_commit_result);

    let mut fs_apply_result = Vec::new();
    push_u64(&mut fs_apply_result, 5);
    push_u16(&mut fs_apply_result, 1);
    push_u16(&mut fs_apply_result, 0);
    push_u16(&mut fs_apply_result, 0);
    push_u16(&mut fs_apply_result, status_code(artifact, "CONFLICT"));
    push_u64(&mut fs_apply_result, 6);
    push_i64(&mut fs_apply_result, 7);
    fs_apply_result.push(1);
    fs_apply_result.extend_from_slice(&[0; 3]);
    fs_apply_result.extend_from_slice(&[8; 32]);
    push_bytes_u16(&mut fs_apply_result, b"changed");
    push_u32(&mut fs_apply_result, 0);
    push_vector(&mut vectors, "fs.apply_result.payload", &fs_apply_result);

    let mut fs_move = Vec::new();
    push_bytes_u32(&mut fs_move, &fs_path_a);
    push_bytes_u32(&mut fs_move, &fs_path_b);
    fs_move.push(1);
    fs_move.extend_from_slice(&[0; 3]);
    fs_move.extend_from_slice(&[7; 16]);
    push_vector(&mut vectors, "fs.state.move.payload", &fs_move);

    let mut git_oid_a = Vec::new();
    git_oid_a.push(family_constant(artifact, "yas.git", "OBJECT_SHA1") as u8);
    git_oid_a.push(20);
    push_u16(&mut git_oid_a, 0);
    git_oid_a.extend_from_slice(&[1; 20]);
    let mut git_oid_b = Vec::new();
    git_oid_b.push(family_constant(artifact, "yas.git", "OBJECT_SHA1") as u8);
    git_oid_b.push(20);
    push_u16(&mut git_oid_b, 0);
    git_oid_b.extend_from_slice(&[2; 20]);
    let mut git_oid_c = Vec::new();
    git_oid_c.push(family_constant(artifact, "yas.git", "OBJECT_SHA1") as u8);
    git_oid_c.push(20);
    push_u16(&mut git_oid_c, 0);
    git_oid_c.extend_from_slice(&[3; 20]);

    let git_query_payload =
        |repository_handle: u64, max_records: u16, cursor: &[u8], body: &[u8]| {
            let mut payload = Vec::new();
            push_u64(&mut payload, repository_handle);
            push_u16(&mut payload, max_records);
            push_u16(&mut payload, 0);
            push_bytes_u16(&mut payload, cursor);
            push_u64(&mut payload, 4096);
            push_bytes_u32(&mut payload, body);
            push_u32(&mut payload, 0);
            payload
        };

    let mut git_source = Vec::new();
    git_source.push(family_constant(artifact, "yas.git", "SOURCE_FS") as u8);
    git_source.extend_from_slice(&[0; 3]);
    push_u64(&mut git_source, 1);
    push_bytes_u32(&mut git_source, &fs_path(&[b"repo"]));
    let mut git_open = Vec::new();
    push_bytes_u32(&mut git_open, &git_source);
    push_u32(&mut git_open, 0);
    push_vector(&mut vectors, "git.open.payload", &git_open);

    let mut git_terminal_source = Vec::new();
    git_terminal_source.push(family_constant(artifact, "yas.git", "SOURCE_TERMINAL_CWD") as u8);
    git_terminal_source.extend_from_slice(&[0; 3]);
    push_u64(&mut git_terminal_source, 9);
    push_bytes_u32(&mut git_terminal_source, &fs_path(&[b"project"]));
    let mut git_open_terminal = Vec::new();
    push_bytes_u32(&mut git_open_terminal, &git_terminal_source);
    push_u32(&mut git_open_terminal, 0);
    push_vector(
        &mut vectors,
        "git.open_terminal.payload",
        &git_open_terminal,
    );

    let mut git_open_result = Vec::new();
    push_u64(&mut git_open_result, 1);
    push_u64(&mut git_open_result, 7);
    git_open_result.push(family_constant(artifact, "yas.git", "OBJECT_SHA1") as u8);
    git_open_result.push(0);
    push_u16(
        &mut git_open_result,
        (family_constant(artifact, "yas.git", "REPOSITORY_WRITABLE")
            | family_constant(artifact, "yas.git", "REPOSITORY_FETCHABLE")) as u16,
    );
    push_bytes_u32(&mut git_open_result, b"/repo");
    push_bytes_u32(&mut git_open_result, b"/repo/.git");
    push_u32(&mut git_open_result, 0);
    push_vector(&mut vectors, "git.open_result.payload", &git_open_result);

    let mut git_close = Vec::new();
    push_u64(&mut git_close, 1);
    push_u32(&mut git_close, 0);
    push_vector(&mut vectors, "git.close.payload", &git_close);

    let mut git_watch = Vec::new();
    push_u64(&mut git_watch, 1);
    push_u16(
        &mut git_watch,
        (family_constant(artifact, "yas.git", "WATCH_HEAD")
            | family_constant(artifact, "yas.git", "WATCH_REFS")) as u16,
    );
    push_u16(&mut git_watch, 0);
    push_bytes_u32(&mut git_watch, &fs_state_watch);
    push_vector(&mut vectors, "git.watch.payload", &git_watch);

    let mut git_watch_extensions = Vec::new();
    push_u16(
        &mut git_watch_extensions,
        family_constant(artifact, "yas.git", "WATCH_REFS_SETTLE_MS_EXTENSION") as u16,
    );
    push_u16(&mut git_watch_extensions, 0);
    push_bytes_u32(&mut git_watch_extensions, &50u16.to_le_bytes());
    push_u16(
        &mut git_watch_extensions,
        family_constant(artifact, "yas.git", "WATCH_STATUS_SETTLE_MS_EXTENSION") as u16,
    );
    push_u16(&mut git_watch_extensions, 0);
    push_bytes_u32(&mut git_watch_extensions, &500u16.to_le_bytes());
    let mut git_prefixes = Vec::new();
    push_u16(&mut git_prefixes, 2);
    push_bytes_u16(&mut git_prefixes, b"refs/heads/");
    push_bytes_u16(&mut git_prefixes, b"refs/remotes/");
    push_u16(
        &mut git_watch_extensions,
        family_constant(artifact, "yas.git", "WATCH_REF_PREFIXES_EXTENSION") as u16,
    );
    push_u16(&mut git_watch_extensions, 0);
    push_bytes_u32(&mut git_watch_extensions, &git_prefixes);
    let mut git_watch_state = Vec::new();
    push_u16(&mut git_watch_state, 0);
    push_u16(&mut git_watch_state, 0);
    push_u64(&mut git_watch_state, 4096);
    push_bytes_u32(&mut git_watch_state, &git_watch_extensions);
    let mut git_watch_options = Vec::new();
    push_u64(&mut git_watch_options, 1);
    push_u16(
        &mut git_watch_options,
        (family_constant(artifact, "yas.git", "WATCH_HEAD")
            | family_constant(artifact, "yas.git", "WATCH_REFS")
            | family_constant(artifact, "yas.git", "WATCH_STATUS")) as u16,
    );
    push_u16(&mut git_watch_options, 0);
    push_bytes_u32(&mut git_watch_options, &git_watch_state);
    push_vector(
        &mut vectors,
        "git.watch_options.payload",
        &git_watch_options,
    );

    let mut git_unwatch = Vec::new();
    push_u32(&mut git_unwatch, 1);
    push_vector(&mut vectors, "git.unwatch.payload", &git_unwatch);

    let mut git_diff_body = Vec::new();
    push_u16(
        &mut git_diff_body,
        family_constant(artifact, "yas.git", "QUERY_DIFF") as u16,
    );
    push_u16(&mut git_diff_body, 0);
    push_u16(
        &mut git_diff_body,
        family_constant(artifact, "yas.git", "DIFF_RENAMES") as u16,
    );
    git_diff_body.push(50);
    git_diff_body.push(0);
    git_diff_body.push(family_constant(artifact, "yas.git", "ENDPOINT_COMMIT") as u8);
    git_diff_body.extend_from_slice(&[0; 3]);
    git_diff_body.push(1);
    git_diff_body.extend_from_slice(&[0; 3]);
    git_diff_body.extend_from_slice(&git_oid_a);
    git_diff_body.push(family_constant(artifact, "yas.git", "ENDPOINT_COMMIT") as u8);
    git_diff_body.extend_from_slice(&[0; 3]);
    git_diff_body.push(1);
    git_diff_body.extend_from_slice(&[0; 3]);
    git_diff_body.extend_from_slice(&git_oid_b);
    push_bytes_u32(&mut git_diff_body, &[]);
    let mut git_query = Vec::new();
    push_u64(&mut git_query, 1);
    push_u16(&mut git_query, 16);
    push_u16(&mut git_query, 0);
    push_bytes_u16(&mut git_query, &[]);
    push_u64(&mut git_query, 4096);
    push_bytes_u32(&mut git_query, &git_diff_body);
    push_u32(&mut git_query, 0);
    push_vector(&mut vectors, "git.query.payload", &git_query);

    let mut git_resolve_body = Vec::new();
    push_u16(
        &mut git_resolve_body,
        family_constant(artifact, "yas.git", "QUERY_RESOLVE") as u16,
    );
    push_u16(&mut git_resolve_body, 0);
    push_bytes_u16(&mut git_resolve_body, b"main...topic");
    push_vector(
        &mut vectors,
        "git.resolve_query.payload",
        &git_query_payload(1, 8, &[], &git_resolve_body),
    );

    let mut git_merge_base_body = Vec::new();
    push_u16(
        &mut git_merge_base_body,
        family_constant(artifact, "yas.git", "QUERY_MERGE_BASE") as u16,
    );
    push_u16(&mut git_merge_base_body, 0);
    push_u16(&mut git_merge_base_body, 3);
    push_u16(&mut git_merge_base_body, 0);
    git_merge_base_body.extend_from_slice(&git_oid_a);
    git_merge_base_body.extend_from_slice(&git_oid_b);
    git_merge_base_body.extend_from_slice(&git_oid_c);
    push_vector(
        &mut vectors,
        "git.merge_base_query.payload",
        &git_query_payload(1, 8, &[], &git_merge_base_body),
    );

    let mut git_explicit_log_body = Vec::new();
    push_u16(
        &mut git_explicit_log_body,
        family_constant(artifact, "yas.git", "QUERY_LOG") as u16,
    );
    push_u16(&mut git_explicit_log_body, 0);
    push_u16(
        &mut git_explicit_log_body,
        (family_constant(artifact, "yas.git", "LOG_TOPO")
            | family_constant(artifact, "yas.git", "LOG_FULL_MESSAGE")) as u16,
    );
    push_u16(&mut git_explicit_log_body, 0);
    push_bytes_u16(&mut git_explicit_log_body, &[]);
    push_u16(&mut git_explicit_log_body, 1);
    push_u16(&mut git_explicit_log_body, 1);
    git_explicit_log_body.extend_from_slice(&git_oid_a);
    git_explicit_log_body.extend_from_slice(&git_oid_b);
    push_bytes_u32(&mut git_explicit_log_body, &fs_path(&[b"src"]));
    push_vector(
        &mut vectors,
        "git.log_query.payload",
        &git_query_payload(1, 32, &[], &git_explicit_log_body),
    );

    let git_tree_path = fs_path(&[b"src"]);
    let mut git_tree_body = Vec::new();
    push_u16(
        &mut git_tree_body,
        family_constant(artifact, "yas.git", "QUERY_TREE") as u16,
    );
    push_u16(&mut git_tree_body, 0);
    git_tree_body.extend_from_slice(&git_oid_a);
    push_bytes_u32(&mut git_tree_body, &git_tree_path);
    let mut git_path_cursor = Vec::new();
    git_path_cursor.push(family_constant(artifact, "yas.git", "CURSOR_PATH") as u8);
    git_path_cursor.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut git_path_cursor, &fs_path(&[b"src", b"next"]));
    push_vector(
        &mut vectors,
        "git.tree_query.payload",
        &git_query_payload(1, 64, &git_path_cursor, &git_tree_body),
    );

    let mut git_blob_body = Vec::new();
    push_u16(
        &mut git_blob_body,
        family_constant(artifact, "yas.git", "QUERY_BLOB") as u16,
    );
    push_u16(&mut git_blob_body, 0);
    push_u16(&mut git_blob_body, 0);
    push_u16(&mut git_blob_body, 0);
    git_blob_body.extend_from_slice(&git_oid_a);
    push_bytes_u32(&mut git_blob_body, &fs_path(&[b"README.md"]));
    push_u64(&mut git_blob_body, 128);
    push_u32(&mut git_blob_body, 4096);
    push_vector(
        &mut vectors,
        "git.blob_query.payload",
        &git_query_payload(1, 1, &[], &git_blob_body),
    );

    let mut git_index_body = Vec::new();
    push_u16(
        &mut git_index_body,
        family_constant(artifact, "yas.git", "QUERY_INDEX") as u16,
    );
    push_u16(&mut git_index_body, 0);
    push_u16(
        &mut git_index_body,
        family_constant(artifact, "yas.git", "INDEX_STAGED") as u16,
    );
    push_u16(&mut git_index_body, 0);
    push_bytes_u32(&mut git_index_body, &git_tree_path);
    push_vector(
        &mut vectors,
        "git.index_query.payload",
        &git_query_payload(1, 64, &git_path_cursor, &git_index_body),
    );

    let mut git_discover_source = Vec::new();
    git_discover_source.push(family_constant(artifact, "yas.git", "SOURCE_PLATFORM_PATH") as u8);
    git_discover_source.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut git_discover_source, b"/work");
    let mut git_discover_body = Vec::new();
    push_u16(
        &mut git_discover_body,
        family_constant(artifact, "yas.git", "QUERY_DISCOVER") as u16,
    );
    push_u16(&mut git_discover_body, 0);
    push_u16(
        &mut git_discover_body,
        (family_constant(artifact, "yas.git", "DISCOVER_NESTED")
            | family_constant(artifact, "yas.git", "DISCOVER_BARE")) as u16,
    );
    push_u16(&mut git_discover_body, 4);
    push_bytes_u32(&mut git_discover_body, &git_discover_source);
    push_vector(
        &mut vectors,
        "git.discover_query.payload",
        &git_query_payload(0, 64, &[], &git_discover_body),
    );

    let mut git_blame_body = Vec::new();
    push_u16(
        &mut git_blame_body,
        family_constant(artifact, "yas.git", "QUERY_BLAME") as u16,
    );
    push_u16(&mut git_blame_body, 0);
    push_u16(
        &mut git_blame_body,
        family_constant(artifact, "yas.git", "BLAME_FOLLOW_RENAMES") as u16,
    );
    push_u16(&mut git_blame_body, 0);
    git_blame_body.extend_from_slice(&git_oid_a);
    push_bytes_u32(&mut git_blame_body, &fs_path(&[b"src", b"lib.rs"]));
    push_u32(&mut git_blame_body, 1);
    push_u32(&mut git_blame_body, 200);
    let mut git_position_cursor = Vec::new();
    git_position_cursor.push(family_constant(artifact, "yas.git", "CURSOR_POSITION") as u8);
    git_position_cursor.extend_from_slice(&[0; 3]);
    push_u64(&mut git_position_cursor, 201);
    push_vector(
        &mut vectors,
        "git.blame_query.payload",
        &git_query_payload(1, 64, &git_position_cursor, &git_blame_body),
    );

    let mut git_reflog_body = Vec::new();
    push_u16(
        &mut git_reflog_body,
        family_constant(artifact, "yas.git", "QUERY_REFLOG") as u16,
    );
    push_u16(&mut git_reflog_body, 0);
    push_u16(
        &mut git_reflog_body,
        family_constant(artifact, "yas.git", "REFLOG_OLDEST_FIRST") as u16,
    );
    push_u16(&mut git_reflog_body, 0);
    push_bytes_u16(&mut git_reflog_body, b"HEAD");
    push_vector(
        &mut vectors,
        "git.reflog_query.payload",
        &git_query_payload(1, 100, &git_position_cursor, &git_reflog_body),
    );

    let mut git_worktrees_body = Vec::new();
    push_u16(
        &mut git_worktrees_body,
        family_constant(artifact, "yas.git", "QUERY_WORKTREES") as u16,
    );
    push_u16(&mut git_worktrees_body, 0);
    push_vector(
        &mut vectors,
        "git.worktrees_query.payload",
        &git_query_payload(1, 64, &git_position_cursor, &git_worktrees_body),
    );

    let mut git_log_body = Vec::new();
    push_u16(
        &mut git_log_body,
        family_constant(artifact, "yas.git", "QUERY_LOG") as u16,
    );
    push_u16(&mut git_log_body, 0);
    push_u16(
        &mut git_log_body,
        family_constant(artifact, "yas.git", "LOG_FIRST_PARENT") as u16,
    );
    push_u16(&mut git_log_body, 0);
    push_bytes_u16(&mut git_log_body, b"refs/heads/main");
    push_u16(&mut git_log_body, 0);
    push_u16(&mut git_log_body, 0);
    push_bytes_u32(&mut git_log_body, &[]);
    let mut git_watch_query = Vec::new();
    push_u64(&mut git_watch_query, 1);
    push_u16(&mut git_watch_query, 32);
    push_u16(&mut git_watch_query, 0);
    push_bytes_u32(&mut git_watch_query, &git_log_body);
    push_bytes_u32(&mut git_watch_query, &fs_state_watch);
    push_vector(&mut vectors, "git.watch_query.payload", &git_watch_query);

    let mut git_object = Vec::new();
    git_object.push(family_constant(artifact, "yas.git", "OBJECT_SHA1") as u8);
    git_object.push(20);
    push_u16(&mut git_object, 0);
    git_object.extend_from_slice(&[0x11; 20]);
    let mut git_object_record = Vec::new();
    git_object_record.push(family_constant(artifact, "yas.git", "OBJECT_ROLE_RESULT") as u8);
    git_object_record.extend_from_slice(&[0; 3]);
    git_object_record.extend_from_slice(&git_object);
    let mut git_record_stream = Vec::new();
    push_u32(&mut git_record_stream, 4 + git_object_record.len() as u32);
    push_u16(
        &mut git_record_stream,
        family_constant(artifact, "yas.git", "RESULT_OBJECT") as u16,
    );
    push_u16(&mut git_record_stream, 0);
    git_record_stream.extend_from_slice(&git_object_record);
    let mut git_log_cursor = Vec::new();
    git_log_cursor.push(family_constant(artifact, "yas.git", "CURSOR_LOG_FRONTIER") as u8);
    git_log_cursor.extend_from_slice(&[0; 3]);
    push_u16(&mut git_log_cursor, 1);
    push_u16(&mut git_log_cursor, 0);
    git_log_cursor.extend_from_slice(&git_oid_a);
    let mut git_watched_page = Vec::new();
    push_bytes_u16(&mut git_watched_page, &git_log_cursor);
    push_u64(&mut git_watched_page, 1);
    push_u16(
        &mut git_watched_page,
        family_constant(artifact, "yas.git", "QUERY_PAGE_MORE") as u16,
    );
    push_u16(&mut git_watched_page, 0);
    git_watched_page.push(family_constant(artifact, "yas.git", "PAGE_INLINE") as u8);
    git_watched_page.extend_from_slice(&[0; 3]);
    push_u16(&mut git_watched_page, 1);
    push_u16(&mut git_watched_page, 0);
    push_bytes_u32(&mut git_watched_page, &git_record_stream);
    push_u32(&mut git_watched_page, 0);

    let mut git_watched_value = Vec::new();
    push_u16(&mut git_watched_value, status_code(artifact, "OK"));
    push_u16(&mut git_watched_value, 0);
    push_bytes_u32(&mut git_watched_value, &[]);
    push_bytes_u32(&mut git_watched_value, &git_watched_page);

    let mut git_query_state_event = Vec::new();
    push_u32(&mut git_query_state_event, 7);
    git_query_state_event.push(state_constant(artifact, "PHASE_SNAPSHOT_RECORDS") as u8);
    git_query_state_event.push(0);
    push_u16(&mut git_query_state_event, 0);
    push_u64(&mut git_query_state_event, 1);
    push_u64(&mut git_query_state_event, 1);
    push_u16(&mut git_query_state_event, 1);
    push_u32(
        &mut git_query_state_event,
        4 + git_watched_value.len() as u32,
    );
    push_u16(
        &mut git_query_state_event,
        state_constant(artifact, "RECORD_ADD") as u16,
    );
    push_u16(&mut git_query_state_event, 0);
    git_query_state_event.extend_from_slice(&git_watched_value);
    let mut git_query_state = Vec::new();
    push_u32(&mut git_query_state, 7);
    push_bytes_u32(&mut git_query_state, &git_query_state_event);
    push_vector(&mut vectors, "git.query_state.payload", &git_query_state);

    let mut git_failed_value = Vec::new();
    push_u16(&mut git_failed_value, status_code(artifact, "NOT_FOUND"));
    push_u16(&mut git_failed_value, 0);
    push_bytes_u32(&mut git_failed_value, b"ref disappeared");
    push_u32(&mut git_failed_value, 0);
    let mut git_failed_state_event = Vec::new();
    push_u32(&mut git_failed_state_event, 7);
    git_failed_state_event.push(state_constant(artifact, "PHASE_DELTA") as u8);
    git_failed_state_event.push(0);
    push_u16(&mut git_failed_state_event, 0);
    push_u64(&mut git_failed_state_event, 1);
    push_u64(&mut git_failed_state_event, 2);
    push_u16(&mut git_failed_state_event, 1);
    push_u32(
        &mut git_failed_state_event,
        4 + git_failed_value.len() as u32,
    );
    push_u16(
        &mut git_failed_state_event,
        state_constant(artifact, "RECORD_REPLACE") as u16,
    );
    push_u16(&mut git_failed_state_event, 0);
    git_failed_state_event.extend_from_slice(&git_failed_value);
    let mut git_failed_state = Vec::new();
    push_u32(&mut git_failed_state, 7);
    push_bytes_u32(&mut git_failed_state, &git_failed_state_event);
    push_vector(
        &mut vectors,
        "git.query_state_error.payload",
        &git_failed_state,
    );

    let mut git_unwatch_query = Vec::new();
    push_u32(&mut git_unwatch_query, 2);
    push_vector(
        &mut vectors,
        "git.unwatch_query.payload",
        &git_unwatch_query,
    );

    let mut git_fetch = Vec::new();
    push_u64(&mut git_fetch, 1);
    git_fetch.extend_from_slice(&[3; 16]);
    push_u16(
        &mut git_fetch,
        family_constant(artifact, "yas.git", "FETCH_PRUNE") as u16,
    );
    push_u16(&mut git_fetch, 1);
    push_u32(&mut git_fetch, 30_000);
    push_bytes_u16(&mut git_fetch, b"origin");
    push_bytes_u16(&mut git_fetch, b"refs/heads/main:refs/remotes/origin/main");
    push_u32(&mut git_fetch, 0);
    push_vector(&mut vectors, "git.fetch.payload", &git_fetch);

    push_vector(&mut vectors, "git.object_id.payload", &git_oid_a);
    let mut git_object_record = Vec::new();
    git_object_record.push(family_constant(artifact, "yas.git", "OBJECT_ROLE_TIP") as u8);
    git_object_record.extend_from_slice(&[0; 3]);
    git_object_record.extend_from_slice(&git_oid_a);
    push_vector(
        &mut vectors,
        "git.object_record.payload",
        &git_object_record,
    );

    let git_patch_path = fs_path(&[b"src"]);
    let mut git_patch_body = Vec::new();
    push_u16(
        &mut git_patch_body,
        family_constant(artifact, "yas.git", "QUERY_PATCH") as u16,
    );
    push_u16(&mut git_patch_body, 0);
    push_u16(
        &mut git_patch_body,
        family_constant(artifact, "yas.git", "PATCH_TEXT") as u16,
    );
    git_patch_body.push(3);
    git_patch_body.push(50);
    push_u32(&mut git_patch_body, 4096);
    git_patch_body.push(family_constant(artifact, "yas.git", "ENDPOINT_COMMIT") as u8);
    git_patch_body.extend_from_slice(&[0; 3]);
    git_patch_body.push(1);
    git_patch_body.extend_from_slice(&[0; 3]);
    git_patch_body.extend_from_slice(&git_oid_a);
    git_patch_body.push(family_constant(artifact, "yas.git", "ENDPOINT_COMMIT") as u8);
    git_patch_body.extend_from_slice(&[0; 3]);
    git_patch_body.push(1);
    git_patch_body.extend_from_slice(&[0; 3]);
    git_patch_body.extend_from_slice(&git_oid_b);
    push_bytes_u32(&mut git_patch_body, &git_patch_path);
    let mut git_patch_query = Vec::new();
    push_u64(&mut git_patch_query, 1);
    push_u16(&mut git_patch_query, 10);
    push_u16(&mut git_patch_query, 0);
    push_bytes_u16(&mut git_patch_query, &[]);
    push_u64(&mut git_patch_query, 4096);
    push_bytes_u32(&mut git_patch_query, &git_patch_body);
    push_u32(&mut git_patch_query, 0);
    push_vector(&mut vectors, "git.patch_query.payload", &git_patch_query);
    let mut git_commit = Vec::new();
    push_u16(&mut git_commit, 0);
    push_u16(&mut git_commit, 0);
    git_commit.extend_from_slice(&git_oid_a);
    git_commit.extend_from_slice(&git_oid_b);
    push_u16(&mut git_commit, 1);
    push_u16(&mut git_commit, 0);
    git_commit.extend_from_slice(&git_oid_b);
    push_i64(&mut git_commit, 1);
    git_commit.extend_from_slice(&60i16.to_le_bytes());
    push_i64(&mut git_commit, 2);
    git_commit.extend_from_slice(&60i16.to_le_bytes());
    push_bytes_u16(&mut git_commit, b"A");
    push_bytes_u16(&mut git_commit, b"a@example.invalid");
    push_bytes_u16(&mut git_commit, b"C");
    push_bytes_u16(&mut git_commit, b"c@example.invalid");
    push_bytes_u32(&mut git_commit, b"message\0bytes");
    push_vector(&mut vectors, "git.commit.payload", &git_commit);

    let mut git_log_path = Vec::new();
    git_log_path.push(family_constant(artifact, "yas.git", "TREE_BLOB") as u8);
    git_log_path.push(1);
    push_u16(&mut git_log_path, 0);
    push_u32(&mut git_log_path, 0o100644);
    git_log_path.extend_from_slice(&git_oid_a);
    push_bytes_u32(&mut git_log_path, &fs_path(&[b"src", b"lib.rs"]));
    push_vector(&mut vectors, "git.log_path.payload", &git_log_path);

    let mut git_patch_file = Vec::new();
    git_patch_file.push(family_constant(artifact, "yas.git", "DIFF_RENAMED") as u8);
    git_patch_file.push(90);
    push_u16(&mut git_patch_file, 0);
    push_bytes_u32(&mut git_patch_file, &fs_path(&[b"old.rs"]));
    push_bytes_u32(&mut git_patch_file, &fs_path(&[b"new.rs"]));
    push_vector(&mut vectors, "git.patch_file.payload", &git_patch_file);

    let mut git_patch_row = Vec::new();
    push_u32(&mut git_patch_row, 4);
    push_u32(&mut git_patch_row, 4);
    push_bytes_u32(&mut git_patch_row, b"old text");
    push_bytes_u32(&mut git_patch_row, b"new text");
    push_u16(&mut git_patch_row, 1);
    push_u16(&mut git_patch_row, 0);
    push_u32(&mut git_patch_row, 0);
    push_u32(&mut git_patch_row, 3);
    push_u16(&mut git_patch_row, 1);
    push_u16(&mut git_patch_row, 0);
    push_u32(&mut git_patch_row, 0);
    push_u32(&mut git_patch_row, 3);
    push_vector(&mut vectors, "git.patch_row.payload", &git_patch_row);

    let mut git_patch_gap = Vec::new();
    push_u32(&mut git_patch_gap, 9);
    push_u32(&mut git_patch_gap, 10);
    push_vector(&mut vectors, "git.patch_gap.payload", &git_patch_gap);

    push_vector(&mut vectors, "git.patch_base.payload", &git_oid_b);

    let mut git_record_stream = Vec::new();
    push_u32(&mut git_record_stream, 4 + git_commit.len() as u32);
    push_u16(
        &mut git_record_stream,
        family_constant(artifact, "yas.git", "RESULT_COMMIT") as u16,
    );
    push_u16(&mut git_record_stream, 0);
    git_record_stream.extend_from_slice(&git_commit);
    let mut git_page = Vec::new();
    push_bytes_u16(&mut git_page, &git_log_cursor);
    push_u64(&mut git_page, 1);
    push_u16(
        &mut git_page,
        family_constant(artifact, "yas.git", "QUERY_PAGE_MORE") as u16,
    );
    push_u16(&mut git_page, 0);
    git_page.push(family_constant(artifact, "yas.git", "PAGE_INLINE") as u8);
    git_page.extend_from_slice(&[0; 3]);
    push_u16(&mut git_page, 1);
    push_u16(&mut git_page, 0);
    push_bytes_u32(&mut git_page, &git_record_stream);
    push_u32(&mut git_page, 0);
    push_vector(&mut vectors, "git.query_page.payload", &git_page);

    push_vector(&mut vectors, "git.query_cursor.payload", &git_log_cursor);

    let mut git_tree_entry = Vec::new();
    git_tree_entry.push(family_constant(artifact, "yas.git", "TREE_BLOB") as u8);
    git_tree_entry.extend_from_slice(&[0; 3]);
    push_u32(&mut git_tree_entry, 0o100644);
    push_bytes_u16(&mut git_tree_entry, b"lib.rs");
    git_tree_entry.extend_from_slice(&git_oid_a);
    push_vector(&mut vectors, "git.tree_entry.payload", &git_tree_entry);

    let mut git_blob_content = Vec::new();
    git_blob_content.extend_from_slice(&git_oid_a);
    push_u64(&mut git_blob_content, 3);
    push_u64(&mut git_blob_content, 0);
    push_u64(&mut git_blob_content, 3);
    git_blob_content.push(family_constant(artifact, "yas.git", "CONTENT_INLINE") as u8);
    git_blob_content.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut git_blob_content, b"yas");
    push_vector(&mut vectors, "git.blob_content.payload", &git_blob_content);

    let mut git_diff_record = Vec::new();
    git_diff_record.push(family_constant(artifact, "yas.git", "DIFF_RENAMED") as u8);
    git_diff_record.push(100);
    push_u16(
        &mut git_diff_record,
        family_constant(artifact, "yas.git", "DIFF_FILTERED_RECORD") as u16,
    );
    push_bytes_u32(&mut git_diff_record, &fs_path(&[b"old.rs"]));
    push_bytes_u32(&mut git_diff_record, &fs_path(&[b"new.rs"]));
    push_u32(&mut git_diff_record, 0o100644);
    push_u32(&mut git_diff_record, 0o100644);
    git_diff_record.push(1);
    git_diff_record.push(1);
    push_u16(&mut git_diff_record, 0);
    git_diff_record.extend_from_slice(&git_oid_a);
    git_diff_record.extend_from_slice(&git_oid_b);
    push_vector(&mut vectors, "git.diff_record.payload", &git_diff_record);

    let mut git_index_record = Vec::new();
    git_index_record.push(0);
    git_index_record.push(family_constant(artifact, "yas.git", "INDEX_STATUS_MODIFIED") as u8);
    push_u16(
        &mut git_index_record,
        family_constant(artifact, "yas.git", "INDEX_SKIP_WORKTREE") as u16,
    );
    push_bytes_u32(&mut git_index_record, &fs_path(&[b"src", b"lib.rs"]));
    push_u32(&mut git_index_record, 0o100644);
    push_u64(&mut git_index_record, 123);
    push_i64(&mut git_index_record, 456);
    git_index_record.extend_from_slice(&git_oid_a);
    push_vector(&mut vectors, "git.index_record.payload", &git_index_record);

    let mut git_discovery_record = Vec::new();
    push_u16(
        &mut git_discovery_record,
        family_constant(artifact, "yas.git", "DISCOVERY_LINKED") as u16,
    );
    push_u16(&mut git_discovery_record, 0);
    git_discovery_record.push(family_constant(artifact, "yas.git", "OBJECT_SHA1") as u8);
    git_discovery_record.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut git_discovery_record, b"/repo");
    push_bytes_u32(&mut git_discovery_record, b"/repo/.git");
    push_vector(
        &mut vectors,
        "git.discovery_record.payload",
        &git_discovery_record,
    );

    let mut git_blame_record = Vec::new();
    push_u16(&mut git_blame_record, 0);
    push_u16(&mut git_blame_record, 0);
    push_u32(&mut git_blame_record, 1);
    push_u32(&mut git_blame_record, 3);
    push_u32(&mut git_blame_record, 7);
    git_blame_record.extend_from_slice(&git_oid_a);
    push_bytes_u32(&mut git_blame_record, &fs_path(&[b"src", b"lib.rs"]));
    push_bytes_u16(&mut git_blame_record, b"Author");
    push_bytes_u16(&mut git_blame_record, b"summary");
    push_vector(&mut vectors, "git.blame_record.payload", &git_blame_record);

    let mut git_reflog_record = Vec::new();
    push_u16(&mut git_reflog_record, 0);
    push_u16(&mut git_reflog_record, 0);
    push_u64(&mut git_reflog_record, 2);
    git_reflog_record.extend_from_slice(&git_oid_a);
    git_reflog_record.extend_from_slice(&git_oid_b);
    push_bytes_u16(&mut git_reflog_record, b"Committer <c@example.invalid>");
    push_i64(&mut git_reflog_record, 3);
    git_reflog_record.extend_from_slice(&60i16.to_le_bytes());
    push_u16(&mut git_reflog_record, 0);
    push_bytes_u32(&mut git_reflog_record, b"update by push");
    push_vector(
        &mut vectors,
        "git.reflog_record.payload",
        &git_reflog_record,
    );

    let mut git_worktree_record = Vec::new();
    push_u16(
        &mut git_worktree_record,
        (family_constant(artifact, "yas.git", "WORKTREE_MAIN")
            | family_constant(artifact, "yas.git", "WORKTREE_CURRENT")
            | family_constant(artifact, "yas.git", "WORKTREE_LOCKED")) as u16,
    );
    push_u16(&mut git_worktree_record, 0);
    push_bytes_u32(&mut git_worktree_record, b"/repo");
    git_worktree_record.push(1);
    git_worktree_record.extend_from_slice(&[0; 3]);
    git_worktree_record.extend_from_slice(&git_oid_a);
    push_bytes_u16(&mut git_worktree_record, b"refs/heads/main");
    push_bytes_u16(&mut git_worktree_record, b"maintenance");
    push_vector(
        &mut vectors,
        "git.worktree_record.payload",
        &git_worktree_record,
    );

    let mut git_fetch_ref_result = Vec::new();
    push_u16(
        &mut git_fetch_ref_result,
        family_constant(artifact, "yas.git", "FETCH_REF_FORCED") as u16,
    );
    push_u16(&mut git_fetch_ref_result, status_code(artifact, "OK"));
    git_fetch_ref_result.push(1);
    git_fetch_ref_result.push(1);
    push_u16(&mut git_fetch_ref_result, 0);
    git_fetch_ref_result.extend_from_slice(&git_oid_a);
    git_fetch_ref_result.extend_from_slice(&git_oid_b);
    push_bytes_u16(&mut git_fetch_ref_result, b"refs/remotes/origin/main");
    push_bytes_u16(&mut git_fetch_ref_result, b"forced update");
    let mut git_fetch_result = Vec::new();
    push_u64(&mut git_fetch_result, 9);
    push_u16(&mut git_fetch_result, 1);
    push_u16(&mut git_fetch_result, 0);
    push_bytes_u32(&mut git_fetch_result, &git_fetch_ref_result);
    push_u32(&mut git_fetch_result, 0);
    push_vector(&mut vectors, "git.fetch_result.payload", &git_fetch_result);

    let mut git_head_body = Vec::new();
    push_u16(
        &mut git_head_body,
        family_constant(artifact, "yas.git", "HEAD_DETACHED") as u16,
    );
    push_u16(&mut git_head_body, 0);
    git_head_body.push(1);
    git_head_body.extend_from_slice(&[0; 3]);
    git_head_body.extend_from_slice(&git_oid_a);
    push_bytes_u16(&mut git_head_body, &[]);
    let mut git_entity = Vec::new();
    push_u16(
        &mut git_entity,
        family_constant(artifact, "yas.git", "ENTITY_HEAD") as u16,
    );
    push_u16(&mut git_entity, 0);
    push_bytes_u16(&mut git_entity, b"HEAD");
    push_u64(&mut git_entity, 1);
    push_bytes_u32(&mut git_entity, &git_head_body);
    push_u32(&mut git_entity, 0);
    push_vector(&mut vectors, "git.entity.payload", &git_entity);
    push_vector(&mut vectors, "git.entity.head.payload", &git_entity);

    let mut git_ref_body = Vec::new();
    push_u16(
        &mut git_ref_body,
        family_constant(artifact, "yas.git", "REF_PEELED") as u16,
    );
    push_u16(&mut git_ref_body, 0);
    git_ref_body.extend_from_slice(&git_oid_a);
    git_ref_body.push(1);
    git_ref_body.extend_from_slice(&[0; 3]);
    git_ref_body.extend_from_slice(&git_oid_b);
    push_bytes_u16(&mut git_ref_body, &[]);
    let mut git_ref_entity = Vec::new();
    push_u16(
        &mut git_ref_entity,
        family_constant(artifact, "yas.git", "ENTITY_REF") as u16,
    );
    push_u16(&mut git_ref_entity, 0);
    push_bytes_u16(&mut git_ref_entity, b"refs/tags/v1");
    push_u64(&mut git_ref_entity, 2);
    push_bytes_u32(&mut git_ref_entity, &git_ref_body);
    push_u32(&mut git_ref_entity, 0);
    push_vector(&mut vectors, "git.entity.ref.payload", &git_ref_entity);

    let mut git_remote_body = Vec::new();
    push_u16(
        &mut git_remote_body,
        family_constant(artifact, "yas.git", "REMOTE_DEFAULT") as u16,
    );
    push_u16(&mut git_remote_body, 0);
    push_bytes_u32(&mut git_remote_body, b"ssh://host/repo");
    push_bytes_u32(&mut git_remote_body, &[]);
    let mut git_remote_entity = Vec::new();
    push_u16(
        &mut git_remote_entity,
        family_constant(artifact, "yas.git", "ENTITY_REMOTE") as u16,
    );
    push_u16(&mut git_remote_entity, 0);
    push_bytes_u16(&mut git_remote_entity, b"origin");
    push_u64(&mut git_remote_entity, 3);
    push_bytes_u32(&mut git_remote_entity, &git_remote_body);
    push_u32(&mut git_remote_entity, 0);
    push_vector(
        &mut vectors,
        "git.entity.remote.payload",
        &git_remote_entity,
    );

    let mut git_operation_body = Vec::new();
    git_operation_body.push(family_constant(artifact, "yas.git", "OPERATION_REBASE") as u8);
    git_operation_body.push(family_constant(artifact, "yas.git", "OPERATION_HEAD_PRESENT") as u8);
    push_u16(&mut git_operation_body, 0);
    git_operation_body.push(1);
    git_operation_body.extend_from_slice(&[0; 3]);
    git_operation_body.extend_from_slice(&git_oid_a);
    push_bytes_u16(&mut git_operation_body, b"onto main");
    let mut git_operation_entity = Vec::new();
    push_u16(
        &mut git_operation_entity,
        family_constant(artifact, "yas.git", "ENTITY_OPERATION") as u16,
    );
    push_u16(&mut git_operation_entity, 0);
    push_bytes_u16(&mut git_operation_entity, b"operation");
    push_u64(&mut git_operation_entity, 4);
    push_bytes_u32(&mut git_operation_entity, &git_operation_body);
    push_u32(&mut git_operation_entity, 0);
    push_vector(
        &mut vectors,
        "git.entity.operation.payload",
        &git_operation_entity,
    );

    let git_status_path = fs_path(&[b"new"]);
    let git_old_status_path = fs_path(&[b"old"]);
    let mut git_status_body = Vec::new();
    git_status_body.push(family_constant(artifact, "yas.git", "WORKTREE_STATUS_RENAMED") as u8);
    git_status_body.push(family_constant(artifact, "yas.git", "WORKTREE_STATUS_MODIFIED") as u8);
    push_u16(
        &mut git_status_body,
        (family_constant(artifact, "yas.git", "STATE_STATUS_CONTENT_PRESENT")
            | family_constant(artifact, "yas.git", "STATE_STATUS_OLD_PATH_PRESENT")) as u16,
    );
    git_status_body.extend_from_slice(&[1, 1, 0, 0]);
    git_status_body.extend_from_slice(&git_oid_b);
    push_bytes_u32(&mut git_status_body, &git_old_status_path);
    let mut git_status_entity = Vec::new();
    push_u16(
        &mut git_status_entity,
        family_constant(artifact, "yas.git", "ENTITY_STATUS") as u16,
    );
    push_u16(&mut git_status_entity, 0);
    push_bytes_u16(&mut git_status_entity, &git_status_path);
    push_u64(&mut git_status_entity, 5);
    push_bytes_u32(&mut git_status_entity, &git_status_body);
    push_u32(&mut git_status_entity, 0);
    push_vector(
        &mut vectors,
        "git.entity.status.payload",
        &git_status_entity,
    );

    let mut git_upstream_body = Vec::new();
    push_u16(
        &mut git_upstream_body,
        family_constant(artifact, "yas.git", "UPSTREAM_COUNTS_VALID") as u16,
    );
    push_u16(&mut git_upstream_body, 0);
    push_u32(&mut git_upstream_body, 2);
    push_u32(&mut git_upstream_body, 3);
    push_bytes_u16(&mut git_upstream_body, b"refs/remotes/origin/main");
    let mut git_upstream_entity = Vec::new();
    push_u16(
        &mut git_upstream_entity,
        family_constant(artifact, "yas.git", "ENTITY_UPSTREAM") as u16,
    );
    push_u16(&mut git_upstream_entity, 0);
    push_bytes_u16(&mut git_upstream_entity, b"refs/heads/main");
    push_u64(&mut git_upstream_entity, 6);
    push_bytes_u32(&mut git_upstream_entity, &git_upstream_body);
    push_u32(&mut git_upstream_entity, 0);
    push_vector(
        &mut vectors,
        "git.entity.upstream.payload",
        &git_upstream_entity,
    );

    let mut git_stash_body = Vec::new();
    git_stash_body.extend_from_slice(&git_oid_a);
    push_i64(&mut git_stash_body, 7);
    git_stash_body.extend_from_slice(&60i16.to_le_bytes());
    push_u16(&mut git_stash_body, 0);
    push_bytes_u32(&mut git_stash_body, b"WIP on main\0raw");
    let mut git_stash_entity = Vec::new();
    push_u16(
        &mut git_stash_entity,
        family_constant(artifact, "yas.git", "ENTITY_STASH") as u16,
    );
    push_u16(&mut git_stash_entity, 0);
    push_bytes_u16(&mut git_stash_entity, &0u32.to_le_bytes());
    push_u64(&mut git_stash_entity, 7);
    push_bytes_u32(&mut git_stash_entity, &git_stash_body);
    push_u32(&mut git_stash_entity, 0);
    push_vector(&mut vectors, "git.entity.stash.payload", &git_stash_entity);

    let mut git_worktree_generation_body = Vec::new();
    push_u32(&mut git_worktree_generation_body, 2);
    push_u32(&mut git_worktree_generation_body, 0);
    push_u64(&mut git_worktree_generation_body, 0x1122_3344_5566_7788);
    let mut git_worktree_generation_entity = Vec::new();
    push_u16(
        &mut git_worktree_generation_entity,
        family_constant(artifact, "yas.git", "ENTITY_WORKTREE_GENERATION") as u16,
    );
    push_u16(&mut git_worktree_generation_entity, 0);
    push_bytes_u16(&mut git_worktree_generation_entity, b"worktrees");
    push_u64(&mut git_worktree_generation_entity, 8);
    push_bytes_u32(
        &mut git_worktree_generation_entity,
        &git_worktree_generation_body,
    );
    push_u32(&mut git_worktree_generation_entity, 0);
    push_vector(
        &mut vectors,
        "git.entity.worktree_generation.payload",
        &git_worktree_generation_entity,
    );

    let mut git_progress = Vec::new();
    git_progress.extend_from_slice(&[3; 16]);
    git_progress.push(family_constant(artifact, "yas.git", "PROGRESS_RECEIVING") as u8);
    git_progress.push(family_constant(artifact, "yas.git", "PROGRESS_TOTAL_KNOWN") as u8);
    push_u16(&mut git_progress, 0);
    push_u64(&mut git_progress, 1);
    push_u64(&mut git_progress, 2);
    push_bytes_u16(&mut git_progress, b"objects");
    push_vector(&mut vectors, "git.progress.payload", &git_progress);

    let mut git_closed = Vec::new();
    push_u64(&mut git_closed, 1);
    push_u64(&mut git_closed, 9);
    git_closed.push(family_constant(artifact, "yas.git", "CLOSED_REPOSITORY_GONE") as u8);
    git_closed.extend_from_slice(&[0; 3]);
    push_bytes_u16(&mut git_closed, b"repository disappeared");
    push_vector(&mut vectors, "git.closed.payload", &git_closed);

    let lsp_repo_path = fs_path(&[b"repo"]);
    let lsp_file_path = fs_path(&[b"src", b"main.rs"]);
    let mut lsp_fs_source = Vec::new();
    lsp_fs_source.push(family_constant(artifact, "yas.lsp", "SOURCE_FS") as u8);
    lsp_fs_source.extend_from_slice(&[0; 3]);
    push_u64(&mut lsp_fs_source, 1);
    push_bytes_u32(&mut lsp_fs_source, &lsp_repo_path);
    let mut lsp_open = Vec::new();
    push_bytes_u32(&mut lsp_open, &lsp_fs_source);
    lsp_open.push(family_constant(artifact, "yas.lsp", "OPEN_EXPLICIT") as u8);
    lsp_open.push(0);
    push_u16(&mut lsp_open, 250);
    push_bytes_u16(&mut lsp_open, b"rust");
    push_bytes_u16(&mut lsp_open, b"default");
    push_bytes_u32(&mut lsp_open, br#"{"cargo":{"allFeatures":true}}"#);
    push_u32(&mut lsp_open, 0);
    push_vector(&mut vectors, "lsp.open.payload", &lsp_open);

    let mut lsp_terminal_source = Vec::new();
    lsp_terminal_source.push(family_constant(artifact, "yas.lsp", "SOURCE_TERMINAL_CWD") as u8);
    lsp_terminal_source.extend_from_slice(&[0; 3]);
    push_u64(&mut lsp_terminal_source, 7);
    push_bytes_u32(&mut lsp_terminal_source, &fs_path(&[b"workspace"]));
    let mut lsp_open_auto = Vec::new();
    push_bytes_u32(&mut lsp_open_auto, &lsp_terminal_source);
    lsp_open_auto.push(family_constant(artifact, "yas.lsp", "OPEN_AUTO_DISCOVER") as u8);
    lsp_open_auto.push(0);
    push_u16(&mut lsp_open_auto, 0);
    push_bytes_u16(&mut lsp_open_auto, b"");
    push_bytes_u16(&mut lsp_open_auto, b"");
    push_bytes_u32(&mut lsp_open_auto, b"");
    push_u32(&mut lsp_open_auto, 0);
    push_vector(&mut vectors, "lsp.open_auto.payload", &lsp_open_auto);

    let mut lsp_open_result = Vec::new();
    push_u64(&mut lsp_open_result, 1);
    push_u64(&mut lsp_open_result, 2);
    lsp_open_result.push(family_constant(artifact, "yas.lsp", "POSITION_UTF8") as u8);
    lsp_open_result.push(0);
    push_u16(&mut lsp_open_result, 1);
    push_u64(
        &mut lsp_open_result,
        family_constant(artifact, "yas.lsp", "CAPABILITIES"),
    );
    push_bytes_u32(&mut lsp_open_result, b"/workspace");
    push_u32(&mut lsp_open_result, 0);
    push_vector(&mut vectors, "lsp.open_result.payload", &lsp_open_result);

    let mut lsp_no_backend_value = Vec::new();
    push_bytes_u32(&mut lsp_no_backend_value, b"no supported language found");
    let mut lsp_no_backend_entries = Vec::new();
    push_u16(
        &mut lsp_no_backend_entries,
        family_constant(artifact, "yas.lsp", "OPEN_NO_BACKEND_DETAIL_EXTENSION") as u16,
    );
    push_u16(&mut lsp_no_backend_entries, 1);
    push_bytes_u32(&mut lsp_no_backend_entries, &lsp_no_backend_value);
    let mut lsp_open_result_no_backend = Vec::new();
    push_u64(&mut lsp_open_result_no_backend, 1);
    push_u64(&mut lsp_open_result_no_backend, 2);
    lsp_open_result_no_backend.push(family_constant(artifact, "yas.lsp", "POSITION_UTF8") as u8);
    lsp_open_result_no_backend.push(0);
    push_u16(&mut lsp_open_result_no_backend, 0);
    push_u64(&mut lsp_open_result_no_backend, 0);
    push_bytes_u32(&mut lsp_open_result_no_backend, b"/workspace");
    push_bytes_u32(&mut lsp_open_result_no_backend, &lsp_no_backend_entries);
    push_vector(
        &mut vectors,
        "lsp.open_result_no_backend.payload",
        &lsp_open_result_no_backend,
    );

    let mut lsp_platform_source = Vec::new();
    lsp_platform_source.push(family_constant(artifact, "yas.lsp", "SOURCE_PLATFORM_PATH") as u8);
    lsp_platform_source.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut lsp_platform_source, b"/workspace");
    push_vector(
        &mut vectors,
        "lsp.workspace_source.platform.payload",
        &lsp_platform_source,
    );

    let mut lsp_closed = Vec::new();
    push_u64(&mut lsp_closed, 1);
    lsp_closed.push(family_constant(artifact, "yas.lsp", "CLOSED_ROOT_GONE") as u8);
    lsp_closed.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut lsp_closed, b"root was removed");
    push_vector(&mut vectors, "lsp.closed.payload", &lsp_closed);

    let mut lsp_close = Vec::new();
    push_u64(&mut lsp_close, 1);
    push_u32(&mut lsp_close, 0);
    push_vector(&mut vectors, "lsp.close.payload", &lsp_close);

    let mut lsp_watch = Vec::new();
    push_u64(&mut lsp_watch, 1);
    push_u16(
        &mut lsp_watch,
        (family_constant(artifact, "yas.lsp", "WATCH_BACKEND")
            | family_constant(artifact, "yas.lsp", "WATCH_DIAGNOSTICS")) as u16,
    );
    push_u16(&mut lsp_watch, 0);
    push_bytes_u32(&mut lsp_watch, &fs_state_watch);
    push_vector(&mut vectors, "lsp.watch.payload", &lsp_watch);

    let mut lsp_unwatch = Vec::new();
    push_u32(&mut lsp_unwatch, 1);
    push_vector(&mut vectors, "lsp.unwatch.payload", &lsp_unwatch);

    let mut lsp_target = Vec::new();
    push_bytes_u32(&mut lsp_target, &lsp_file_path);
    push_u64(&mut lsp_target, 3);
    lsp_target.extend_from_slice(&[9; 32]);
    let mut lsp_query_body = Vec::new();
    push_u16(
        &mut lsp_query_body,
        family_constant(artifact, "yas.lsp", "QUERY_RENAME") as u16,
    );
    push_u16(&mut lsp_query_body, 0);
    lsp_query_body.extend_from_slice(&lsp_target);
    push_u32(&mut lsp_query_body, 2);
    push_u32(&mut lsp_query_body, 4);
    push_bytes_u16(&mut lsp_query_body, b"renamed");
    let mut lsp_query = Vec::new();
    push_u64(&mut lsp_query, 1);
    push_u16(&mut lsp_query, 16);
    push_u16(&mut lsp_query, 0);
    push_bytes_u16(&mut lsp_query, b"");
    push_u64(&mut lsp_query, 4096);
    push_bytes_u32(&mut lsp_query, &lsp_query_body);
    push_u32(&mut lsp_query, 0);
    push_vector(&mut vectors, "lsp.query.payload", &lsp_query);

    let mut lsp_signature_body = Vec::new();
    push_u16(
        &mut lsp_signature_body,
        family_constant(artifact, "yas.lsp", "QUERY_SIGNATURE_HELP") as u16,
    );
    push_u16(&mut lsp_signature_body, 0);
    lsp_signature_body.extend_from_slice(&lsp_target);
    push_u32(&mut lsp_signature_body, 2);
    push_u32(&mut lsp_signature_body, 4);
    push_vector(
        &mut vectors,
        "lsp.signature_query.payload",
        &lsp_signature_body,
    );

    let mut lsp_buffer_put = Vec::new();
    push_u64(&mut lsp_buffer_put, 1);
    lsp_buffer_put.extend_from_slice(&[1; 16]);
    push_u64(&mut lsp_buffer_put, 0);
    push_bytes_u32(&mut lsp_buffer_put, &lsp_file_path);
    push_bytes_u32(&mut lsp_buffer_put, b"fn main() {}\n");
    push_u32(&mut lsp_buffer_put, 0);
    push_vector(&mut vectors, "lsp.buffer_put.payload", &lsp_buffer_put);

    let mut lsp_buffer_begin = Vec::new();
    push_u64(&mut lsp_buffer_begin, 1);
    push_u64(&mut lsp_buffer_begin, 2);
    push_bytes_u32(&mut lsp_buffer_begin, &lsp_file_path);
    push_u64(&mut lsp_buffer_begin, 65_536);
    lsp_buffer_begin.extend_from_slice(&[2; 32]);
    push_u64(&mut lsp_buffer_begin, 4096);
    push_u32(&mut lsp_buffer_begin, 0);
    push_vector(&mut vectors, "lsp.buffer_begin.payload", &lsp_buffer_begin);

    let mut lsp_buffer_commit = Vec::new();
    push_u64(&mut lsp_buffer_commit, 2);
    lsp_buffer_commit.extend_from_slice(&[3; 16]);
    push_u32(&mut lsp_buffer_commit, 0);
    push_vector(
        &mut vectors,
        "lsp.buffer_commit.payload",
        &lsp_buffer_commit,
    );

    let mut lsp_buffer_close = Vec::new();
    push_u64(&mut lsp_buffer_close, 4);
    push_u64(&mut lsp_buffer_close, 5);
    lsp_buffer_close.extend_from_slice(&[4; 16]);
    push_u32(&mut lsp_buffer_close, 0);
    push_vector(&mut vectors, "lsp.buffer_close.payload", &lsp_buffer_close);

    let mut lsp_list_servers = Vec::new();
    push_u64(&mut lsp_list_servers, 1);
    push_u32(&mut lsp_list_servers, 0);
    push_vector(&mut vectors, "lsp.list_servers.payload", &lsp_list_servers);

    let mut lsp_stop_server = Vec::new();
    push_u64(&mut lsp_stop_server, 1);
    push_u64(&mut lsp_stop_server, 1);
    lsp_stop_server.extend_from_slice(&[5; 16]);
    push_u32(&mut lsp_stop_server, 0);
    push_vector(&mut vectors, "lsp.stop_server.payload", &lsp_stop_server);

    let lsp_buffer_descriptor = sensitive_byte_descriptor(
        artifact,
        3,
        family_constant(artifact, "yas.transfer", "DIRECTION_RECEIVER_TO_SENDER") as u8,
        4096,
        0,
        (
            "yas.lsp",
            family_constant(artifact, "yas.lsp", "BUFFER_CONTENT_KIND") as u16,
        ),
        Some(2),
    );
    let mut lsp_begin_result = Vec::new();
    push_u64(&mut lsp_begin_result, 2);
    push_bytes_u32(&mut lsp_begin_result, &lsp_buffer_descriptor);
    push_u32(&mut lsp_begin_result, 0);
    push_vector(
        &mut vectors,
        "lsp.buffer_begin_result.payload",
        &lsp_begin_result,
    );

    let mut lsp_location = Vec::new();
    push_bytes_u32(&mut lsp_location, &lsp_file_path);
    push_u64(&mut lsp_location, 3);
    lsp_location.extend_from_slice(&[9; 32]);
    push_u32(&mut lsp_location, 2);
    push_u32(&mut lsp_location, 4);
    push_u32(&mut lsp_location, 2);
    push_u32(&mut lsp_location, 8);
    push_u16(
        &mut lsp_location,
        family_constant(artifact, "yas.lsp", "LOCATION_DECLARATION") as u16,
    );
    push_u16(&mut lsp_location, 0);
    push_vector(&mut vectors, "lsp.location.payload", &lsp_location);

    let mut lsp_hover = Vec::new();
    push_bytes_u32(&mut lsp_hover, &lsp_location);
    lsp_hover.push(family_constant(artifact, "yas.lsp", "MARKUP_MARKDOWN") as u8);
    lsp_hover.extend_from_slice(&[0; 3]);
    push_bytes_u32(&mut lsp_hover, b"**type**");
    push_vector(&mut vectors, "lsp.hover.payload", &lsp_hover);

    let mut lsp_symbol = Vec::new();
    push_u16(
        &mut lsp_symbol,
        family_constant(artifact, "yas.lsp", "SYMBOL_FUNCTION") as u16,
    );
    push_u16(&mut lsp_symbol, 0);
    push_u16(&mut lsp_symbol, 2);
    push_u16(&mut lsp_symbol, 0);
    push_bytes_u16(&mut lsp_symbol, b"main");
    push_bytes_u16(&mut lsp_symbol, b"fn main()");
    push_bytes_u32(&mut lsp_symbol, &lsp_file_path);
    lsp_symbol.push(1);
    lsp_symbol.extend_from_slice(&[0; 3]);
    lsp_symbol.extend_from_slice(&[9; 32]);
    for value in [2, 4, 2, 8, 2, 4, 2, 8] {
        push_u32(&mut lsp_symbol, value);
    }
    push_vector(&mut vectors, "lsp.symbol.payload", &lsp_symbol);

    let mut lsp_edit = Vec::new();
    push_bytes_u32(&mut lsp_edit, &lsp_file_path);
    push_u64(&mut lsp_edit, 3);
    lsp_edit.extend_from_slice(&[9; 32]);
    for value in [2, 4, 2, 8] {
        push_u32(&mut lsp_edit, value);
    }
    push_bytes_u32(&mut lsp_edit, b"renamed");
    push_vector(&mut vectors, "lsp.edit.payload", &lsp_edit);

    let mut lsp_signature = Vec::new();
    push_u16(
        &mut lsp_signature,
        family_constant(artifact, "yas.lsp", "SIGNATURE_ACTIVE") as u16,
    );
    push_u16(&mut lsp_signature, 0);
    push_u32(&mut lsp_signature, 3);
    push_u32(&mut lsp_signature, 8);
    push_bytes_u16(&mut lsp_signature, b"fn main(value: u32)");
    push_bytes_u32(&mut lsp_signature, b"Calls main.");
    push_vector(&mut vectors, "lsp.signature.payload", &lsp_signature);

    let mut lsp_record_stream = Vec::new();
    push_u32(&mut lsp_record_stream, 4 + lsp_location.len() as u32);
    push_u16(
        &mut lsp_record_stream,
        family_constant(artifact, "yas.lsp", "RESULT_LOCATION") as u16,
    );
    push_u16(&mut lsp_record_stream, 0);
    lsp_record_stream.extend_from_slice(&lsp_location);
    let mut lsp_page = Vec::new();
    push_u16(&mut lsp_page, status_code(artifact, "OK"));
    push_u16(&mut lsp_page, 0);
    push_bytes_u32(&mut lsp_page, b"");
    push_bytes_u16(&mut lsp_page, b"");
    push_u64(&mut lsp_page, 1);
    lsp_page.push(family_constant(artifact, "yas.lsp", "PAGE_INLINE") as u8);
    lsp_page.extend_from_slice(&[0; 3]);
    push_u16(&mut lsp_page, 1);
    push_u16(&mut lsp_page, 0);
    push_bytes_u32(&mut lsp_page, &lsp_record_stream);
    push_u32(&mut lsp_page, 0);
    push_vector(&mut vectors, "lsp.query_page.payload", &lsp_page);

    let mut lsp_incomplete_page = Vec::new();
    push_u16(
        &mut lsp_incomplete_page,
        status_code(artifact, "UNAVAILABLE"),
    );
    push_u16(
        &mut lsp_incomplete_page,
        family_constant(artifact, "yas.lsp", "PAGE_INCOMPLETE") as u16,
    );
    push_bytes_u32(&mut lsp_incomplete_page, b"backend is indexing");
    push_bytes_u16(&mut lsp_incomplete_page, b"");
    push_u64(&mut lsp_incomplete_page, 0);
    lsp_incomplete_page.push(family_constant(artifact, "yas.lsp", "PAGE_INLINE") as u8);
    lsp_incomplete_page.extend_from_slice(&[0; 3]);
    push_u16(&mut lsp_incomplete_page, 0);
    push_u16(&mut lsp_incomplete_page, 0);
    push_bytes_u32(&mut lsp_incomplete_page, b"");
    push_u32(&mut lsp_incomplete_page, 0);
    push_vector(
        &mut vectors,
        "lsp.query_page_incomplete.payload",
        &lsp_incomplete_page,
    );

    let mut lsp_server = Vec::new();
    push_u64(&mut lsp_server, 1);
    push_u64(&mut lsp_server, 1);
    push_u64(&mut lsp_server, 2);
    push_u64(&mut lsp_server, 3);
    lsp_server.push(family_constant(artifact, "yas.lsp", "SERVER_READY") as u8);
    lsp_server.push(100);
    push_u16(&mut lsp_server, 0);
    push_u32(&mut lsp_server, 4);
    push_u32(&mut lsp_server, 2);
    push_u32(&mut lsp_server, 0);
    push_u64(&mut lsp_server, 65_536);
    push_u64(
        &mut lsp_server,
        family_constant(artifact, "yas.lsp", "CAPABILITIES"),
    );
    push_bytes_u16(&mut lsp_server, b"rust");
    push_bytes_u16(&mut lsp_server, b"default");
    push_bytes_u16(&mut lsp_server, b"rust-analyzer");
    push_bytes_u32(&mut lsp_server, b"ready");
    push_u32(&mut lsp_server, 0);
    push_vector(&mut vectors, "lsp.server.payload", &lsp_server);

    let mut lsp_diagnostic = Vec::new();
    push_u64(&mut lsp_diagnostic, 1);
    lsp_diagnostic.push(family_constant(artifact, "yas.lsp", "DIAGNOSTIC_WARNING") as u8);
    lsp_diagnostic.push(0);
    push_u16(&mut lsp_diagnostic, 0);
    push_u32(&mut lsp_diagnostic, 2);
    push_u32(&mut lsp_diagnostic, 4);
    push_u32(&mut lsp_diagnostic, 2);
    push_u32(&mut lsp_diagnostic, 8);
    push_bytes_u16(&mut lsp_diagnostic, b"unused");
    push_bytes_u16(&mut lsp_diagnostic, b"rustc");
    push_bytes_u32(&mut lsp_diagnostic, b"unused value");
    let mut lsp_diagnostics = Vec::new();
    push_bytes_u32(&mut lsp_diagnostics, &lsp_file_path);
    push_u64(&mut lsp_diagnostics, 3);
    lsp_diagnostics.extend_from_slice(&[9; 32]);
    push_u64(&mut lsp_diagnostics, 4);
    push_u16(&mut lsp_diagnostics, 1);
    push_u16(&mut lsp_diagnostics, 0);
    push_bytes_u32(&mut lsp_diagnostics, &lsp_diagnostic);
    push_u32(&mut lsp_diagnostics, 0);
    push_vector(&mut vectors, "lsp.diagnostics.payload", &lsp_diagnostics);

    let mut lsp_remove = Vec::new();
    push_u16(
        &mut lsp_remove,
        family_constant(artifact, "yas.lsp", "ENTITY_DIAGNOSTICS") as u16,
    );
    push_u16(&mut lsp_remove, 0);
    push_bytes_u32(&mut lsp_remove, &lsp_file_path);
    push_u64(&mut lsp_remove, 5);
    push_vector(&mut vectors, "lsp.remove.payload", &lsp_remove);

    let mut events_set_config = Vec::new();
    events_set_config.extend_from_slice(&[1; 16]);
    push_u64(&mut events_set_config, 7);
    push_u64(
        &mut events_set_config,
        family_constant(artifact, "yas.events", "DEFAULT_RING_BYTES"),
    );
    for value in 1..=4 {
        push_u64(&mut events_set_config, value);
    }
    push_u32(&mut events_set_config, 0);
    push_vector(
        &mut vectors,
        "events.set_config.payload",
        &events_set_config,
    );

    let events_descriptor = sensitive_byte_descriptor(
        artifact,
        2,
        family_constant(artifact, "yas.transfer", "DIRECTION_SENDER_TO_RECEIVER") as u8,
        0,
        65_536,
        (
            "yas.events",
            family_constant(artifact, "yas.events", "DUMP_CONTENT_KIND") as u16,
        ),
        None,
    );
    let mut events_dump = Vec::new();
    push_u64(&mut events_dump, 12_345);
    events_dump.extend_from_slice(&[9; 32]);
    push_bytes_u32(&mut events_dump, &events_descriptor);
    push_u32(&mut events_dump, 0);
    push_vector(&mut vectors, "events.dump_result.payload", &events_dump);

    let events_codec = artifact
        .codecs
        .iter()
        .find(|codec| codec.name == "events-v1")
        .unwrap();
    let mut events_record = Vec::new();
    push_u64(&mut events_record, 1);
    push_u16(&mut events_record, events_codec.id);
    push_u16(&mut events_record, events_codec.version);
    push_u64(&mut events_record, 10);
    push_u16(&mut events_record, 1);
    push_u16(&mut events_record, 0);
    push_u32(&mut events_record, 31);
    push_u64(&mut events_record, 10);
    push_u64(&mut events_record, 99);
    push_u32(
        &mut events_record,
        family_constant(artifact, "yas.events", "EVENT_SERVER_START") as u32,
    );
    push_u16(&mut events_record, 0);
    push_u16(&mut events_record, 0x1234);
    events_record.extend_from_slice(b"yas");
    push_vector(&mut vectors, "events.record.payload", &events_record);

    let mut events_recording = Vec::new();
    push_u64(&mut events_recording, 1);
    events_recording.push(family_constant(artifact, "yas.events", "RECORDING_RUNNING") as u8);
    events_recording.push(0);
    push_u16(
        &mut events_recording,
        family_constant(artifact, "yas.events", "RECORDING_HISTORY") as u16,
    );
    push_u64(&mut events_recording, 2);
    push_u64(&mut events_recording, 512);
    push_u64(&mut events_recording, 0);
    push_bytes_u32(&mut events_recording, b"/tmp/events.yas");
    push_bytes_u32(&mut events_recording, b"");
    push_u32(&mut events_recording, 0);
    push_vector(
        &mut vectors,
        "events.recording_info.payload",
        &events_recording,
    );

    let extension_upload_descriptor = sensitive_byte_descriptor(
        artifact,
        3,
        family_constant(artifact, "yas.transfer", "DIRECTION_RECEIVER_TO_SENDER") as u8,
        4096,
        0,
        (
            "yas.extension",
            family_constant(artifact, "yas.extension", "OBJECT_CONTENT_KIND") as u16,
        ),
        Some(1),
    );
    let mut extension_object_begin = Vec::new();
    extension_object_begin.push(family_constant(artifact, "yas.extension", "OBJECT_UPLOAD") as u8);
    extension_object_begin.extend_from_slice(&[0; 7]);
    push_u64(&mut extension_object_begin, 1);
    push_bytes_u32(&mut extension_object_begin, &extension_upload_descriptor);
    push_u32(&mut extension_object_begin, 0);
    push_vector(
        &mut vectors,
        "extension.object_begin_result.payload",
        &extension_object_begin,
    );

    let mut extension_runtime_limits = Vec::new();
    push_u64(&mut extension_runtime_limits, 1 << 20);
    push_u64(&mut extension_runtime_limits, 64 << 10);
    push_u32(&mut extension_runtime_limits, 1);
    push_u32(&mut extension_runtime_limits, 2);
    push_u64(&mut extension_runtime_limits, 4096);
    push_u64(&mut extension_runtime_limits, 1_000_000_000);
    push_u32(&mut extension_runtime_limits, 0);

    let extension_flags = (family_constant(artifact, "yas.extension", "DEFINITION_ENABLED")
        | family_constant(artifact, "yas.extension", "DEFINITION_DESIRED_RUNNING"))
        as u16;
    let mut extension_deploy = Vec::new();
    extension_deploy.extend_from_slice(&[4; 16]);
    push_u64(&mut extension_deploy, 0);
    push_u64(&mut extension_deploy, 0);
    push_u64(&mut extension_deploy, 0);
    push_u16(&mut extension_deploy, extension_flags);
    extension_deploy.push(family_constant(artifact, "yas.extension", "RUNTIME_WASMI") as u8);
    extension_deploy.push(family_constant(artifact, "yas.extension", "RESTART_ON_FAILURE") as u8);
    push_bytes_u16(&mut extension_deploy, b"demo");
    extension_deploy.extend_from_slice(&[5; 32]);
    push_u16(&mut extension_deploy, 2);
    push_bytes_u32(&mut extension_deploy, b"--raw");
    push_bytes_u32(&mut extension_deploy, &[0xff, 0]);
    push_bytes_u32(&mut extension_deploy, &extension_runtime_limits);
    push_u32(&mut extension_deploy, 0);
    push_vector(&mut vectors, "extension.deploy.payload", &extension_deploy);

    let mut extension_state = Vec::new();
    push_u64(&mut extension_state, 1);
    push_u64(&mut extension_state, 1);
    push_u64(&mut extension_state, 1);
    extension_state.push(family_constant(artifact, "yas.extension", "PHASE_RUNNING") as u8);
    extension_state.push(family_constant(artifact, "yas.extension", "RUNTIME_WASMI") as u8);
    extension_state.push(family_constant(artifact, "yas.extension", "RESTART_ON_FAILURE") as u8);
    extension_state.push(0);
    push_u16(&mut extension_state, extension_flags);
    push_u16(&mut extension_state, 0);
    push_u64(&mut extension_state, 1);
    push_u64(&mut extension_state, 1);
    push_u32(&mut extension_state, 42);
    push_u32(&mut extension_state, 0);
    push_u64(&mut extension_state, 0);
    push_u64(&mut extension_state, 1);
    extension_state.extend_from_slice(&[5; 32]);
    push_bytes_u16(&mut extension_state, b"demo");
    push_bytes_u32(&mut extension_state, &[]);
    push_bytes_u32(&mut extension_state, &extension_runtime_limits);
    push_u32(&mut extension_state, 0);
    push_vector(&mut vectors, "extension.state.payload", &extension_state);

    let mut extension_follow_descriptor = Vec::new();
    push_u32(&mut extension_follow_descriptor, 4);
    extension_follow_descriptor
        .push(family_constant(artifact, "yas.transfer", "MODE_MESSAGE") as u8);
    extension_follow_descriptor.push(family_constant(
        artifact,
        "yas.transfer",
        "DIRECTION_SENDER_TO_RECEIVER",
    ) as u8);
    push_u16(&mut extension_follow_descriptor, 0);
    push_u64(&mut extension_follow_descriptor, 0);
    push_u64(&mut extension_follow_descriptor, 4096);
    push_u64(&mut extension_follow_descriptor, 4096);
    push_u32(&mut extension_follow_descriptor, 1024);
    push_u16(
        &mut extension_follow_descriptor,
        family_id(artifact, "yas.extension"),
    );
    push_u16(
        &mut extension_follow_descriptor,
        family_constant(artifact, "yas.extension", "FOLLOW_CONTENT_KIND") as u16,
    );
    push_u16(
        &mut extension_follow_descriptor,
        family_version(artifact, "yas.extension"),
    );
    push_u32(&mut extension_follow_descriptor, 8);
    push_u16(
        &mut extension_follow_descriptor,
        family_constant(artifact, "yas.transfer", "SENSITIVE_CONTENT_EXTENSION") as u16,
    );
    push_u16(&mut extension_follow_descriptor, 1);
    push_u32(&mut extension_follow_descriptor, 0);
    let mut extension_follow = Vec::new();
    push_u64(&mut extension_follow, 1);
    push_u64(&mut extension_follow, 0);
    push_u64(&mut extension_follow, 1);
    push_bytes_u32(&mut extension_follow, &extension_follow_descriptor);
    push_u32(&mut extension_follow, 0);
    push_vector(
        &mut vectors,
        "extension.follow_result.payload",
        &extension_follow,
    );

    let mut extension_output = Vec::new();
    push_u64(&mut extension_output, 7);
    push_u16(&mut extension_output, 1);
    push_u16(&mut extension_output, 0);
    extension_output.push(family_constant(artifact, "yas.extension", "OUTPUT_STDOUT") as u8);
    extension_output.extend_from_slice(&[0; 3]);
    push_u64(&mut extension_output, 7);
    push_u64(&mut extension_output, 99);
    push_bytes_u32(&mut extension_output, b"hello");
    push_vector(
        &mut vectors,
        "extension.output_batch.payload",
        &extension_output,
    );

    let mut extension_command = Vec::new();
    push_u64(&mut extension_command, 2);
    push_u64(&mut extension_command, 0);
    push_u16(&mut extension_command, 1);
    push_u16(&mut extension_command, 0);
    push_u64(&mut extension_command, 1);
    push_u64(&mut extension_command, 1);
    push_u64(&mut extension_command, 1);
    extension_command.extend_from_slice(&[5; 32]);
    push_u64(&mut extension_command, 2);
    push_u64(&mut extension_command, 1);
    push_bytes_u16(&mut extension_command, b"demo");
    push_bytes_u16(&mut extension_command, b"run");
    push_bytes_u32(&mut extension_command, b"{\"args\":[]}");
    push_u32(&mut extension_command, 0);
    push_vector(
        &mut vectors,
        "extension.command_page.payload",
        &extension_command,
    );

    let mut extension_attempt = Vec::new();
    push_u64(&mut extension_attempt, 1);
    push_u64(&mut extension_attempt, 1);
    push_u64(&mut extension_attempt, 1);
    push_u64(&mut extension_attempt, 1);
    push_u32(&mut extension_attempt, 42);
    push_u16(&mut extension_attempt, extension_flags);
    extension_attempt.push(family_constant(artifact, "yas.extension", "RUNTIME_WASMI") as u8);
    extension_attempt.push(0);
    extension_attempt.extend_from_slice(&[5; 32]);
    push_bytes_u16(&mut extension_attempt, b"demo");
    push_u16(&mut extension_attempt, 2);
    push_bytes_u32(&mut extension_attempt, b"--raw");
    push_bytes_u32(&mut extension_attempt, &[0xff, 0]);
    push_u32(&mut extension_attempt, 0);
    push_vector(
        &mut vectors,
        "extension.attempt_context.payload",
        &extension_attempt,
    );

    let mut net_peer = Vec::new();
    net_peer.push(family_constant(artifact, "yas.net", "ADDRESS_TCP") as u8);
    net_peer.extend_from_slice(&[0; 3]);
    push_bytes_u16(&mut net_peer, b"example.com");
    push_u16(&mut net_peer, 443);
    push_u16(&mut net_peer, 0);
    let mut net_tls = Vec::new();
    net_tls.push(family_constant(artifact, "yas.net", "TLS_VERIFY_STRICT") as u8);
    net_tls.extend_from_slice(&[0; 3]);
    push_bytes_u16(&mut net_tls, b"example.com");
    push_u16(&mut net_tls, 2);
    push_u16(&mut net_tls, 0);
    push_bytes_u16(&mut net_tls, b"h2");
    push_bytes_u16(&mut net_tls, b"http/1.1");
    push_u32(&mut net_tls, 0);
    let mut net_open = Vec::new();
    net_open.extend_from_slice(&[1; 16]);
    push_bytes_u32(&mut net_open, &net_peer);
    net_open.push(family_constant(artifact, "yas.net", "DELIVERY_PREFERENCE_NOT_APPLICABLE") as u8);
    net_open.push(family_constant(artifact, "yas.net", "DROP_NOT_APPLICABLE") as u8);
    push_u16(&mut net_open, 0);
    push_u64(&mut net_open, 4096);
    push_bytes_u32(&mut net_open, b"GET ");
    push_bytes_u32(&mut net_open, &net_tls);
    push_u32(&mut net_open, 0);
    push_vector(&mut vectors, "net.open.payload", &net_open);

    let net_descriptor = sensitive_byte_descriptor(
        artifact,
        2,
        family_constant(artifact, "yas.net", "DIRECTION_DUPLEX") as u8,
        4096,
        4096,
        (
            "yas.net",
            family_constant(artifact, "yas.net", "FLOW_CONTENT_KIND") as u16,
        ),
        None,
    );
    let mut net_endpoint = Vec::new();
    push_u64(&mut net_endpoint, 1);
    net_endpoint.push(family_constant(artifact, "yas.net", "MODE_BYTE") as u8);
    net_endpoint.push(family_constant(artifact, "yas.net", "DIRECTION_DUPLEX") as u8);
    net_endpoint.push(family_constant(artifact, "yas.net", "DELIVERY_NOT_APPLICABLE") as u8);
    net_endpoint.push(0);
    push_u32(&mut net_endpoint, 0);
    push_u32(&mut net_endpoint, 0);
    push_u64(&mut net_endpoint, 0);
    push_u32(&mut net_endpoint, 0);
    push_bytes_u32(&mut net_endpoint, &net_peer);
    push_bytes_u16(&mut net_endpoint, b"h2");
    push_bytes_u32(&mut net_endpoint, &net_descriptor);
    push_u32(&mut net_endpoint, 0);
    push_vector(&mut vectors, "net.endpoint.payload", &net_endpoint);

    let mut net_datagram = Vec::new();
    push_u64(&mut net_datagram, 2);
    push_u64(&mut net_datagram, 3);
    net_datagram.extend_from_slice(b"udp");
    push_vector(&mut vectors, "net.datagram.payload", &net_datagram);

    let mut net_stats = Vec::new();
    push_u64(&mut net_stats, 2);
    push_u64(&mut net_stats, 1);
    push_u16(
        &mut net_stats,
        family_constant(artifact, "yas.net", "DATAGRAM_STATS_FINAL") as u16,
    );
    push_u16(&mut net_stats, 0);
    for value in 1..=7 {
        push_u64(&mut net_stats, value);
    }
    push_u32(&mut net_stats, 0);
    push_vector(&mut vectors, "net.datagram_stats.payload", &net_stats);

    for codec in &artifact.codecs {
        vectors.push(GoldenVector {
            name: format!("packed_codec.{}.payload", codec.name),
            hex: codec.golden_hex.to_ascii_lowercase(),
        });
    }

    VectorArtifact {
        schema: artifact.schema,
        vectors,
    }
}

fn push_vector(vectors: &mut Vec<GoldenVector>, name: &str, bytes: &[u8]) {
    vectors.push(GoldenVector {
        name: name.into(),
        hex: hex(bytes),
    });
}

fn family_id(artifact: &Artifact, name: &str) -> u16 {
    artifact
        .families
        .iter()
        .find(|family| family.name == name)
        .unwrap()
        .id
}

fn family_version(artifact: &Artifact, name: &str) -> u16 {
    artifact
        .families
        .iter()
        .find(|family| family.name == name)
        .unwrap()
        .version
}

fn family_constant(artifact: &Artifact, family_name: &str, name: &str) -> u64 {
    artifact
        .families
        .iter()
        .find(|family| family.name == family_name)
        .and_then(|family| family.constants.iter().find(|value| value.name == name))
        .unwrap()
        .value
}

fn request_kind(artifact: &Artifact, family_name: &str, name: &str) -> u16 {
    artifact
        .families
        .iter()
        .find(|family| family.name == family_name)
        .and_then(|family| family.requests.iter().find(|value| value.name == name))
        .unwrap()
        .kind
}

fn event_kind(artifact: &Artifact, family_name: &str, name: &str) -> u16 {
    artifact
        .families
        .iter()
        .find(|family| family.name == family_name)
        .and_then(|family| family.events.iter().find(|value| value.name == name))
        .unwrap()
        .kind
}

fn family_descriptor(
    artifact: &Artifact,
    family_name: &str,
    runtime_state: u8,
    operations: &[(u8, u8, u16)],
    limits: &[u8],
) -> Vec<u8> {
    let mut body = Vec::new();
    push_u16(&mut body, family_id(artifact, family_name));
    push_u16(&mut body, family_version(artifact, family_name));
    body.push(runtime_state);
    body.push(0);
    push_u16(&mut body, operations.len() as u16);
    for &(direction, class, kind) in operations {
        body.push(direction);
        body.push(class);
        push_u16(&mut body, kind);
    }
    push_bytes_u32(&mut body, limits);
    let mut record = Vec::new();
    push_bytes_u32(&mut record, &body);
    record
}

fn family_limit_extensions(artifact: &Artifact, family_name: &str) -> Vec<u8> {
    let mut extensions = Vec::new();
    let family = artifact
        .families
        .iter()
        .find(|family| family.name == family_name)
        .unwrap();
    for limit in &family.limits {
        push_u16(&mut extensions, limit.tag);
        push_u16(&mut extensions, 0);
        match limit.value_type {
            LimitValueType::U32 => {
                push_u32(&mut extensions, 4);
                push_u32(&mut extensions, limit.hard_max as u32);
            }
            LimitValueType::U64 => {
                push_u32(&mut extensions, 8);
                push_u64(&mut extensions, limit.hard_max);
            }
        }
    }
    extensions
}

fn state_constant(artifact: &Artifact, name: &str) -> u64 {
    artifact
        .state
        .constants
        .iter()
        .find(|value| value.name == name)
        .unwrap()
        .value
}

fn status_code(artifact: &Artifact, name: &str) -> u16 {
    artifact
        .statuses
        .iter()
        .find(|status| status.name == name)
        .unwrap()
        .code
}

fn transport_codec(artifact: &Artifact, name: &str) -> u16 {
    artifact
        .transport
        .codec
        .iter()
        .find(|codec| codec.name == name)
        .unwrap()
        .id
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_bytes_u16(out: &mut Vec<u8>, value: &[u8]) {
    push_u16(out, value.len() as u16);
    out.extend_from_slice(value);
}

fn push_bytes_u32(out: &mut Vec<u8>, value: &[u8]) {
    push_u32(out, value.len() as u32);
    out.extend_from_slice(value);
}

fn sensitive_byte_descriptor(
    artifact: &Artifact,
    transfer_id: u32,
    direction: u8,
    receiver_send_credit: u64,
    sender_send_credit: u64,
    content: (&str, u16),
    upload_stage_handle: Option<u64>,
) -> Vec<u8> {
    let mut descriptor = Vec::new();
    push_u32(&mut descriptor, transfer_id);
    descriptor.push(family_constant(artifact, "yas.transfer", "MODE_BYTE") as u8);
    descriptor.push(direction);
    push_u16(&mut descriptor, 0);
    push_u64(&mut descriptor, receiver_send_credit);
    push_u64(&mut descriptor, sender_send_credit);
    push_u64(&mut descriptor, 0);
    push_u32(&mut descriptor, 1024);
    push_u16(&mut descriptor, family_id(artifact, content.0));
    push_u16(&mut descriptor, content.1);
    push_u16(&mut descriptor, family_version(artifact, content.0));
    push_u32(
        &mut descriptor,
        if upload_stage_handle.is_some() { 32 } else { 8 },
    );
    push_u16(
        &mut descriptor,
        family_constant(artifact, "yas.transfer", "SENSITIVE_CONTENT_EXTENSION") as u16,
    );
    push_u16(&mut descriptor, 1);
    push_u32(&mut descriptor, 0);
    if let Some(staging_handle) = upload_stage_handle {
        push_u16(
            &mut descriptor,
            family_constant(artifact, "yas.transfer", "UPLOAD_STAGE_EXTENSION") as u16,
        );
        push_u16(&mut descriptor, 1);
        push_u32(&mut descriptor, 16);
        push_u64(&mut descriptor, staging_handle);
        push_u64(&mut descriptor, 1);
    }
    descriptor
}

fn fs_path(components: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    push_u16(&mut out, components.len() as u16);
    for component in components {
        push_bytes_u16(&mut out, component);
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[usize::from(byte >> 4)] as char);
        out.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    out
}

fn generate_markdown(artifact: &Artifact) -> String {
    let mut out = String::from(
        "<!-- @generated by crates/yas/schema_codegen.rs; do not edit. -->\n\
         # YAS v1 wire registry\n\n\
         This is a deterministic view of the canonical TOML schema. Layout text is normative;\n\
         implementation and resource-lifecycle rules remain in `docs/design/yas.md`.\n\n",
    );
    out.push_str("## Transport\n\n| Property | Value |\n| --- | ---: |\n");
    for (name, value) in [
        (
            "Protocol major",
            artifact.transport.protocol_major.to_string(),
        ),
        ("Preface", format!("`{}`", artifact.transport.preface_hex)),
        (
            "WebSocket subprotocol",
            format!("`{}`", artifact.transport.websocket_subprotocol),
        ),
        (
            "Event header bytes",
            artifact.transport.event_header_bytes.to_string(),
        ),
        (
            "Correlated header bytes",
            artifact.transport.correlated_header_bytes.to_string(),
        ),
        (
            "Hard maximum wire frame",
            artifact.transport.limits.wire_frame.to_string(),
        ),
        (
            "Hard maximum decoded frame",
            artifact.transport.limits.decoded_frame.to_string(),
        ),
        (
            "Hard maximum datagram",
            artifact.transport.limits.datagram.to_string(),
        ),
        (
            "Hard maximum bulk chunk",
            artifact.transport.limits.bulk_chunk.to_string(),
        ),
        (
            "Hard aggregate buffered bytes",
            artifact.transport.limits.buffered.to_string(),
        ),
    ] {
        out.push_str(&format!("| {name} | {value} |\n"));
    }

    out.push_str(
        "\n## Families\n\n| ID | Name | Version | Dependencies |\n| ---: | --- | ---: | --- |\n",
    );
    for family in &artifact.families {
        let dependencies = if family.dependencies.is_empty() {
            "—".to_owned()
        } else {
            family
                .dependencies
                .iter()
                .map(|id| format!("`0x{id:04x}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        out.push_str(&format!(
            "| `0x{:04x}` | `{}` | {} | {} |\n",
            family.id, family.name, family.version, dependencies
        ));
    }

    out.push_str("\n## Statuses\n\n| Code | Name |\n| ---: | --- |\n");
    for status in &artifact.statuses {
        out.push_str(&format!("| {} | `{}` |\n", status.code, status.name));
    }

    for family in &artifact.families {
        out.push_str(&format!(
            "\n## `{}` (`0x{:04x}`/v{})\n",
            family.name, family.id, family.version
        ));
        for (title, class, operations) in [
            ("Requests", "Request", &family.requests),
            ("Events", "Event", &family.events),
        ] {
            if operations.is_empty() {
                continue;
            }
            out.push_str(&format!(
                "\n### {title}\n\n| Kind | Name | Direction | Sensitive | Compression | Datagram | Required layout |\n| ---: | --- | --- | --- | --- | --- | --- |\n"
            ));
            for operation in operations {
                out.push_str(&format!(
                    "| `0x{:04x}` | `{}` | `{}` | `{}` | `{}` | `{}` | {} |\n",
                    operation.kind,
                    operation.name,
                    operation.direction.name(),
                    operation.sensitive.name(),
                    operation.compression.name(),
                    operation.datagram.name(),
                    markdown_cell(&operation.layout)
                ));
            }
            if class == "Request" {
                out.push_str(
                    "\nEvery Request kind has a correlated Result with the same family and kind.\n",
                );
            }
        }
        if !family.limits.is_empty() {
            out.push_str(
                "\n### Limits\n\n| Tag | Name | Width | Required | Hard minimum | Hard maximum |\n| ---: | --- | ---: | --- | ---: | ---: |\n",
            );
            for limit in &family.limits {
                out.push_str(&format!(
                    "| {} | `{}` | {} | {} | {} | {} |\n",
                    limit.tag,
                    limit.name,
                    limit.value_type.width(),
                    limit.required,
                    limit.hard_min,
                    limit.hard_max
                ));
            }
        }
        if !family.types.is_empty() {
            out.push_str("\n### Shared types\n\n| Name | Required layout |\n| --- | --- |\n");
            for value in &family.types {
                out.push_str(&format!(
                    "| `{}` | {} |\n",
                    value.name,
                    markdown_cell(&value.layout)
                ));
            }
        }
    }

    out.push_str("\n## Packed codecs\n\n| Family | ID | Version | Name | Direction | Required layout |\n| --- | ---: | ---: | --- | --- | --- |\n");
    for codec in &artifact.codecs {
        out.push_str(&format!(
            "| `{}` | {} | {} | `{}` | `{}` | {} |\n",
            codec.family,
            codec.id,
            codec.version,
            codec.name,
            codec.direction.name(),
            markdown_cell(&codec.layout)
        ));
    }
    out
}

fn markdown_cell(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], " ")
}

fn generate_inspection(artifact: &Artifact) -> String {
    let mut packets = Vec::new();
    for family in &artifact.families {
        for operation in &family.requests {
            for (class, class_id) in [
                ("request", artifact.transport.class.request),
                ("result", artifact.transport.class.result),
            ] {
                packets.push(serde_json::json!({
                    "key": format!("{}/{class_id}/{}", family.id, operation.kind),
                    "family": family.id,
                    "family_name": family.name,
                    "family_version": family.version,
                    "class": class,
                    "class_id": class_id,
                    "kind": operation.kind,
                    "name": operation.name,
                    "header_bytes": artifact.transport.correlated_header_bytes,
                    "correlated": true,
                    "direction": operation.direction.name(),
                    "sensitive": operation.sensitive.name(),
                    "compression": operation.compression.name(),
                    "datagram": operation.datagram.name(),
                    "layout": operation.layout,
                }));
            }
        }
        for operation in &family.events {
            let class_id = artifact.transport.class.event;
            packets.push(serde_json::json!({
                "key": format!("{}/{class_id}/{}", family.id, operation.kind),
                "family": family.id,
                "family_name": family.name,
                "family_version": family.version,
                "class": "event",
                "class_id": class_id,
                "kind": operation.kind,
                "name": operation.name,
                "header_bytes": artifact.transport.event_header_bytes,
                "correlated": false,
                "direction": operation.direction.name(),
                "sensitive": operation.sensitive.name(),
                "compression": operation.compression.name(),
                "datagram": operation.datagram.name(),
                "layout": operation.layout,
            }));
        }
    }
    let value = serde_json::json!({
        "schema": artifact.schema,
        "protocol_major": artifact.transport.protocol_major,
        "preface_hex": artifact.transport.preface_hex,
        "classes": {
            artifact.transport.class.event.to_string(): "event",
            artifact.transport.class.request.to_string(): "request",
            artifact.transport.class.result.to_string(): "result",
        },
        "meta": {
            "class_mask": artifact.transport.class.mask,
            "compressed": artifact.transport.meta.compressed,
            "sensitive": artifact.transport.meta.sensitive,
            "reserved_mask": artifact.transport.meta.reserved_mask,
        },
        "packets": packets,
        "packed_codecs": artifact.codecs,
    });
    format!("{}\n", serde_json::to_string_pretty(&value).unwrap())
}

fn generate_rust(artifact: &Artifact, vectors: &VectorArtifact) -> String {
    let mut out = String::from(
        "// @generated by crates/yas/schema_codegen.rs; do not edit.\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct OperationMetadata { pub name: &'static str, pub class: u8, pub kind: u16, pub direction: u8, pub sensitive: u8, pub compression: u8, pub datagram: u8, pub layout: &'static str }\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct TypeMetadata { pub name: &'static str, pub layout: &'static str }\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct ConstantMetadata { pub name: &'static str, pub value: u64 }\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub enum LimitValueType { U32, U64 }\n\
         impl LimitValueType { pub const fn width(self) -> usize { match self { Self::U32 => 4, Self::U64 => 8 } } }\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct LimitMetadata { pub name: &'static str, pub tag: u16, pub value_type: LimitValueType, pub required: bool, pub hard_min: u64, pub hard_max: u64 }\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct CodecMetadata { pub name: &'static str, pub family: u16, pub id: u16, pub version: u16, pub direction: u8, pub layout: &'static str, pub golden_hex: &'static str, pub constants: &'static [ConstantMetadata] }\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct FamilyMetadata { pub name: &'static str, pub id: u16, pub version: u16, pub dependencies: &'static [u16], pub operations: &'static [OperationMetadata], pub limits: &'static [LimitMetadata], pub types: &'static [TypeMetadata], pub constants: &'static [ConstantMetadata] }\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct GoldenVector { pub name: &'static str, pub hex: &'static str }\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub enum GeneratedHeaderError { Truncated, InvalidClass, ReservedMeta, RequestIdPresence, ZeroRequestId }\n\
         #[derive(Clone, Copy, Debug, PartialEq, Eq)]\n\
         pub struct GeneratedFrameHeader { pub family: u16, pub kind: u16, pub class: u8, pub request_id: Option<u32>, pub compressed: bool, pub sensitive: bool }\n\
         impl GeneratedFrameHeader {\n\
         pub const fn encoded_len(&self) -> usize { if self.class == transport::class::EVENT { transport::EVENT_HEADER_BYTES } else { transport::CORRELATED_HEADER_BYTES } }\n\
         pub fn encode(self) -> Result<([u8; 9], usize), GeneratedHeaderError> {\n\
         let event = self.class == transport::class::EVENT;\n\
         if !event && self.class != transport::class::REQUEST && self.class != transport::class::RESULT { return Err(GeneratedHeaderError::InvalidClass); }\n\
         if event != self.request_id.is_none() { return Err(GeneratedHeaderError::RequestIdPresence); }\n\
         if self.request_id == Some(0) { return Err(GeneratedHeaderError::ZeroRequestId); }\n\
         let mut output = [0u8; 9];\n\
         output[0..2].copy_from_slice(&self.family.to_le_bytes());\n\
         output[2..4].copy_from_slice(&self.kind.to_le_bytes());\n\
         output[4] = self.class | if self.compressed { transport::META_COMPRESSED } else { 0 } | if self.sensitive { transport::META_SENSITIVE } else { 0 };\n\
         let len = self.encoded_len();\n\
         if let Some(request_id) = self.request_id { output[5..9].copy_from_slice(&request_id.to_le_bytes()); }\n\
         Ok((output, len))\n\
         }\n\
         pub fn decode(input: &[u8]) -> Result<(Self, usize), GeneratedHeaderError> {\n\
         if input.len() < transport::EVENT_HEADER_BYTES { return Err(GeneratedHeaderError::Truncated); }\n\
         let meta = input[4];\n\
         if meta & transport::META_RESERVED != 0 { return Err(GeneratedHeaderError::ReservedMeta); }\n\
         let class = meta & transport::CLASS_MASK;\n\
         let correlated = class == transport::class::REQUEST || class == transport::class::RESULT;\n\
         if class != transport::class::EVENT && !correlated { return Err(GeneratedHeaderError::InvalidClass); }\n\
         let len = if correlated { transport::CORRELATED_HEADER_BYTES } else { transport::EVENT_HEADER_BYTES };\n\
         if input.len() < len { return Err(GeneratedHeaderError::Truncated); }\n\
         let request_id = if correlated { let value = u32::from_le_bytes([input[5], input[6], input[7], input[8]]); if value == 0 { return Err(GeneratedHeaderError::ZeroRequestId); } Some(value) } else { None };\n\
         Ok((Self { family: u16::from_le_bytes([input[0], input[1]]), kind: u16::from_le_bytes([input[2], input[3]]), class, request_id, compressed: meta & transport::META_COMPRESSED != 0, sensitive: meta & transport::META_SENSITIVE != 0 }, len))\n\
         }\n\
         }\n",
    );
    let preface = decode_hex(&artifact.transport.preface_hex);
    out.push_str("pub mod transport {\n");
    out.push_str(&format!("pub const PREFACE: [u8; 8] = {:?};\n", preface));
    out.push_str(&format!(
        "pub const PROTOCOL_MAJOR: u16 = {};\n",
        artifact.transport.protocol_major
    ));
    out.push_str(&format!(
        "pub const WEBSOCKET_SUBPROTOCOL: &str = {:?};\n",
        artifact.transport.websocket_subprotocol
    ));
    out.push_str(&format!(
        "pub const STREAM_LENGTH_BITS: u8 = {};\n",
        artifact.transport.stream_length_bits
    ));
    out.push_str(&format!(
        "pub const STREAM_LENGTH_BYTES: usize = {};\n",
        artifact.transport.stream_length_bits / 8
    ));
    out.push_str(&format!(
        "pub const EVENT_HEADER_BYTES: usize = {};\n",
        artifact.transport.event_header_bytes
    ));
    out.push_str(&format!(
        "pub const CORRELATED_HEADER_BYTES: usize = {};\n",
        artifact.transport.correlated_header_bytes
    ));
    out.push_str(&format!(
        "pub const RECOMMENDED_WIRE_FRAME: u32 = {};\n",
        artifact.transport.recommended.wire_frame
    ));
    out.push_str(&format!(
        "pub const RECOMMENDED_DECODED_FRAME: u32 = {};\n",
        artifact.transport.recommended.decoded_frame
    ));
    out.push_str(&format!(
        "pub const RECOMMENDED_BUFFERED: u64 = {};\n",
        artifact.transport.recommended.buffered
    ));
    out.push_str(&format!(
        "pub const CLASS_MASK: u8 = {};\n",
        artifact.transport.class.mask
    ));
    out.push_str(&format!(
        "pub const META_COMPRESSED: u8 = {};\n",
        artifact.transport.meta.compressed
    ));
    out.push_str(&format!(
        "pub const META_SENSITIVE: u8 = {};\n",
        artifact.transport.meta.sensitive
    ));
    out.push_str(&format!(
        "pub const META_RESERVED: u8 = {};\n",
        artifact.transport.meta.reserved_mask
    ));
    out.push_str(&format!(
        "pub const PRE_HELLO_MAX_FRAME: u32 = {};\n",
        artifact.transport.limits.pre_hello_frame
    ));
    out.push_str(&format!(
        "pub const HARD_MAX_WIRE_FRAME: u32 = {};\n",
        artifact.transport.limits.wire_frame
    ));
    out.push_str(&format!(
        "pub const HARD_MAX_DECODED_FRAME: u32 = {};\n",
        artifact.transport.limits.decoded_frame
    ));
    out.push_str(&format!(
        "pub const HARD_MAX_DATAGRAM: u32 = {};\n",
        artifact.transport.limits.datagram
    ));
    out.push_str(&format!(
        "pub const HARD_MAX_BULK_CHUNK: u32 = {};\n",
        artifact.transport.limits.bulk_chunk
    ));
    out.push_str(&format!(
        "pub const HARD_MAX_BUFFERED: u64 = {};\n",
        artifact.transport.limits.buffered
    ));
    out.push_str(&format!(
        "pub const HARD_MAX_EXTENSION_ENTRIES: usize = {};\n",
        artifact.transport.limits.extension_entries
    ));
    out.push_str(&format!(
        "pub const HARD_MAX_TYPED_RECORDS: usize = {};\n",
        artifact.transport.limits.typed_records
    ));
    out.push_str("pub mod class {\n");
    out.push_str(&format!(
        "pub const EVENT: u8 = {};\n",
        artifact.transport.class.event
    ));
    out.push_str(&format!(
        "pub const REQUEST: u8 = {};\n",
        artifact.transport.class.request
    ));
    out.push_str(&format!(
        "pub const RESULT: u8 = {};\n",
        artifact.transport.class.result
    ));
    out.push_str("}\npub mod codec {\n");
    for codec in &artifact.transport.codec {
        out.push_str(&format!("pub const {}: u16 = {};\n", codec.name, codec.id));
    }
    out.push_str(
        "}\npub mod direction {\n\
                  pub const CLIENT_TO_SERVER: u8 = 0;\n\
                  pub const SERVER_TO_CLIENT: u8 = 1;\n\
                  pub const BIDIRECTIONAL: u8 = 2;\n\
                  }\npub mod policy {\n\
                  pub const ALLOWED: u8 = 0;\n\
                  pub const REQUIRED: u8 = 1;\n\
                  pub const FORBIDDEN: u8 = 2;\n\
                  }\npub mod datagram_predicate {\n",
    );
    out.push_str(&format!(
        "pub const FORBIDDEN: u8 = {};\n",
        artifact.transport.datagram_predicate.forbidden
    ));
    out.push_str(&format!(
        "pub const NET_NATIVE_FLOW: u8 = {};\n",
        artifact.transport.datagram_predicate.net_native_flow
    ));
    out.push_str(&format!(
        "pub const SURFACE_FRAME: u8 = {};\n",
        artifact.transport.datagram_predicate.surface_frame
    ));
    out.push_str(&format!(
        "pub const MEDIA_FRAME: u8 = {};\n",
        artifact.transport.datagram_predicate.media_frame
    ));
    out.push_str("}\n}\n");

    out.push_str("pub mod state {\n");
    for constant in &artifact.state.constants {
        out.push_str(&format!(
            "pub const {}: u64 = {};\n",
            constant.name, constant.value
        ));
    }
    out.push_str("pub static TYPES: &[super::TypeMetadata] = &[\n");
    for value in &artifact.state.types {
        out.push_str(&format!(
            "super::TypeMetadata {{ name: {:?}, layout: {:?} }},\n",
            value.name, value.layout
        ));
    }
    out.push_str("];\n}\n");
    out.push_str("pub mod family {\n");
    for family in &artifact.families {
        out.push_str(&format!(
            "pub const {}: u16 = 0x{:04x};\n",
            family.const_name, family.id
        ));
    }
    out.push_str("}\n");

    out.push_str("pub mod packed_codec {\n");
    for codec in &artifact.codecs {
        let module = codec.const_name.to_ascii_lowercase();
        out.push_str(&format!("pub mod {module} {{\n"));
        out.push_str(&format!("pub const ID: u16 = {};\n", codec.id));
        out.push_str(&format!("pub const VERSION: u16 = {};\n", codec.version));
        for constant in &codec.constants {
            out.push_str(&format!(
                "pub const {}: u64 = {};\n",
                constant.name, constant.value
            ));
        }
        out.push_str("pub static CONSTANTS: &[super::super::ConstantMetadata] = &[\n");
        for constant in &codec.constants {
            out.push_str(&format!(
                "super::super::ConstantMetadata {{ name: {:?}, value: {} }},\n",
                constant.name, constant.value
            ));
        }
        out.push_str("];\n}\n");
        out.push_str(&format!(
            "pub const {}: u16 = {}::ID;\n",
            codec.const_name, module
        ));
    }
    out.push_str("}\n");

    for family in &artifact.families {
        if family.requests.is_empty()
            && family.events.is_empty()
            && family.types.is_empty()
            && family.constants.is_empty()
        {
            continue;
        }
        let module = family.name.strip_prefix("yas.").unwrap().replace('-', "_");
        out.push_str(&format!("pub mod {module} {{\n"));
        out.push_str(&format!(
            "pub const FAMILY: u16 = super::family::{};\n",
            family.const_name
        ));
        out.push_str(&format!("pub const VERSION: u16 = {};\n", family.version));
        for (class, operations) in [("request", &family.requests), ("event", &family.events)] {
            out.push_str(&format!("pub mod {class} {{\n"));
            for operation in operations {
                out.push_str(&format!(
                    "pub const {}: u16 = 0x{:04x};\n",
                    operation.name, operation.kind
                ));
            }
            out.push_str("}\n");
        }
        if family.name == "yas.core" {
            out.push_str("pub mod status {\n");
            for status in &artifact.statuses {
                out.push_str(&format!(
                    "pub const {}: u16 = {};\n",
                    status.name, status.code
                ));
            }
            out.push_str("}\n");
        }
        for constant in &family.constants {
            out.push_str(&format!(
                "pub const {}: u64 = {};\n",
                constant.name, constant.value
            ));
        }
        let family_codec_prefix = format!("{}_", family.const_name);
        for codec in artifact
            .codecs
            .iter()
            .filter(|codec| codec.family == family.name)
        {
            let alias = codec
                .const_name
                .strip_prefix(&family_codec_prefix)
                .unwrap_or(codec.const_name.as_str());
            out.push_str(&format!(
                "pub const {alias}: u64 = super::packed_codec::{} as u64;\n",
                codec.const_name
            ));
        }
        let mut codec_constant_counts = BTreeMap::new();
        for constant in artifact
            .codecs
            .iter()
            .filter(|codec| codec.family == family.name)
            .flat_map(|codec| &codec.constants)
        {
            *codec_constant_counts
                .entry(constant.name.as_str())
                .or_insert(0usize) += 1;
        }
        for codec in artifact
            .codecs
            .iter()
            .filter(|codec| codec.family == family.name)
        {
            let module = codec.const_name.to_ascii_lowercase();
            for constant in &codec.constants {
                if codec_constant_counts[constant.name.as_str()] == 1
                    && !family
                        .constants
                        .iter()
                        .any(|value| value.name == constant.name)
                {
                    out.push_str(&format!(
                        "pub const {}: u64 = super::packed_codec::{module}::{};\n",
                        constant.name, constant.name
                    ));
                }
            }
        }
        out.push_str("pub static OPERATIONS: &[super::OperationMetadata] = &[\n");
        for (class, operations) in [(1u8, &family.requests), (0u8, &family.events)] {
            for operation in operations {
                out.push_str(&format!(
                    "super::OperationMetadata {{ name: {:?}, class: {class}, kind: {}, direction: {}, sensitive: {}, compression: {}, datagram: {}, layout: {:?} }},\n",
                    operation.name,
                    operation.kind,
                    operation.direction.wire(),
                    operation.sensitive.wire(),
                    operation.compression.wire(),
                    operation.datagram.wire(&artifact.transport.datagram_predicate),
                    operation.layout
                ));
            }
        }
        out.push_str("];\npub static TYPES: &[super::TypeMetadata] = &[\n");
        for value in &family.types {
            out.push_str(&format!(
                "super::TypeMetadata {{ name: {:?}, layout: {:?} }},\n",
                value.name, value.layout
            ));
        }
        out.push_str("];\npub static LIMITS: &[super::LimitMetadata] = &[\n");
        for limit in &family.limits {
            let value_type = match limit.value_type {
                LimitValueType::U32 => "U32",
                LimitValueType::U64 => "U64",
            };
            out.push_str(&format!(
                "super::LimitMetadata {{ name: {:?}, tag: {}, value_type: super::LimitValueType::{value_type}, required: {}, hard_min: {}, hard_max: {} }},\n",
                limit.name,
                limit.tag,
                limit.required,
                limit.hard_min,
                limit.hard_max
            ));
        }
        out.push_str("];\npub static CONSTANTS: &[super::ConstantMetadata] = &[\n");
        for constant in &family.constants {
            out.push_str(&format!(
                "super::ConstantMetadata {{ name: {:?}, value: {} }},\n",
                constant.name, constant.value
            ));
        }
        for codec in artifact
            .codecs
            .iter()
            .filter(|codec| codec.family == family.name)
        {
            let alias = codec
                .const_name
                .strip_prefix(&family_codec_prefix)
                .unwrap_or(codec.const_name.as_str());
            out.push_str(&format!(
                "super::ConstantMetadata {{ name: {:?}, value: super::packed_codec::{} as u64 }},\n",
                alias, codec.const_name
            ));
        }
        for codec in artifact
            .codecs
            .iter()
            .filter(|codec| codec.family == family.name)
        {
            let module = codec.const_name.to_ascii_lowercase();
            for constant in &codec.constants {
                if codec_constant_counts[constant.name.as_str()] == 1
                    && !family
                        .constants
                        .iter()
                        .any(|value| value.name == constant.name)
                {
                    out.push_str(&format!(
                        "super::ConstantMetadata {{ name: {:?}, value: super::packed_codec::{module}::{} }},\n",
                        constant.name, constant.name
                    ));
                }
            }
        }
        out.push_str("];\n}\n");
    }

    out.push_str("pub static CODECS: &[CodecMetadata] = &[\n");
    for codec in &artifact.codecs {
        let module = codec.const_name.to_ascii_lowercase();
        let family = artifact
            .families
            .iter()
            .find(|family| family.name == codec.family)
            .unwrap();
        out.push_str(&format!(
            "CodecMetadata {{ name: {:?}, family: family::{}, id: {}, version: {}, direction: {}, layout: {:?}, golden_hex: {:?}, constants: packed_codec::{module}::CONSTANTS }},\n",
            codec.name,
            family.const_name,
            codec.id,
            codec.version,
            codec.direction.wire(),
            codec.layout,
            codec.golden_hex
        ));
    }
    out.push_str("];\n");

    out.push_str("pub static FAMILIES: &[FamilyMetadata] = &[\n");
    for family in &artifact.families {
        let module = family.name.strip_prefix("yas.").unwrap().replace('-', "_");
        let (operations, limits, types, constants) = if family.requests.is_empty()
            && family.events.is_empty()
            && family.limits.is_empty()
            && family.types.is_empty()
            && family.constants.is_empty()
        {
            (
                "&[]".to_owned(),
                "&[]".to_owned(),
                "&[]".to_owned(),
                "&[]".to_owned(),
            )
        } else {
            (
                format!("{module}::OPERATIONS"),
                format!("{module}::LIMITS"),
                format!("{module}::TYPES"),
                format!("{module}::CONSTANTS"),
            )
        };
        out.push_str(&format!(
            "FamilyMetadata {{ name: {:?}, id: {}, version: {}, dependencies: &{:?}, operations: {operations}, limits: {limits}, types: {types}, constants: {constants} }},\n",
            family.name, family.id, family.version, family.dependencies
        ));
    }
    out.push_str("];\npub static GOLDEN_VECTORS: &[GoldenVector] = &[\n");
    for vector in &vectors.vectors {
        out.push_str(&format!(
            "GoldenVector {{ name: {:?}, hex: {:?} }},\n",
            vector.name, vector.hex
        ));
    }
    out.push_str("];\n");
    out
}

fn decode_hex(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |byte: u8| match byte {
                b'0'..=b'9' => byte - b'0',
                b'a'..=b'f' => byte - b'a' + 10,
                b'A'..=b'F' => byte - b'A' + 10,
                _ => unreachable!(),
            };
            digit(pair[0]) << 4 | digit(pair[1])
        })
        .collect()
}

fn generate_typescript(
    artifact: &Artifact,
    _vector_artifact: &VectorArtifact,
    json: &str,
    vectors: &str,
) -> String {
    let mut out = String::from("// @generated by crates/yas/schema_codegen.rs; do not edit.\n");
    out.push_str(&format!(
        "export const YAS_PREFACE_HEX = {:?} as const;\n",
        artifact.transport.preface_hex
    ));
    out.push_str(&format!(
        "export const YAS_PROTOCOL_MAJOR = {} as const;\n",
        artifact.transport.protocol_major
    ));
    out.push_str(&format!(
        "export const YAS_WEBSOCKET_SUBPROTOCOL = {:?} as const;\n",
        artifact.transport.websocket_subprotocol
    ));
    out.push_str(&format!(
        "export const YAS_STREAM_LENGTH_BITS = {} as const;\n",
        artifact.transport.stream_length_bits
    ));
    out.push_str(&format!(
        "export const YAS_STREAM_LENGTH_BYTES = {} as const;\n",
        artifact.transport.stream_length_bits / 8
    ));
    out.push_str(&format!(
        "export const YAS_EVENT_HEADER_BYTES = {} as const;\n",
        artifact.transport.event_header_bytes
    ));
    out.push_str(&format!(
        "export const YAS_CORRELATED_HEADER_BYTES = {} as const;\n",
        artifact.transport.correlated_header_bytes
    ));
    out.push_str(&format!(
        "export const YAS_RECOMMENDED_WIRE_FRAME = {} as const;\n",
        artifact.transport.recommended.wire_frame
    ));
    out.push_str(&format!(
        "export const YAS_RECOMMENDED_DECODED_FRAME = {} as const;\n",
        artifact.transport.recommended.decoded_frame
    ));
    out.push_str(&format!(
        "export const YAS_RECOMMENDED_BUFFERED = {} as const;\n",
        artifact.transport.recommended.buffered
    ));
    out.push_str(&format!(
        "export const YAS_CLASS_EVENT = {} as const;\n",
        artifact.transport.class.event
    ));
    out.push_str(&format!(
        "export const YAS_CLASS_REQUEST = {} as const;\n",
        artifact.transport.class.request
    ));
    out.push_str(&format!(
        "export const YAS_CLASS_RESULT = {} as const;\n",
        artifact.transport.class.result
    ));
    out.push_str(&format!(
        "export const YAS_META_CLASS_MASK = {} as const;\n",
        artifact.transport.class.mask
    ));
    out.push_str(&format!(
        "export const YAS_META_COMPRESSED = {} as const;\n",
        artifact.transport.meta.compressed
    ));
    out.push_str(&format!(
        "export const YAS_META_SENSITIVE = {} as const;\n",
        artifact.transport.meta.sensitive
    ));
    out.push_str(&format!(
        "export const YAS_META_RESERVED = {} as const;\n",
        artifact.transport.meta.reserved_mask
    ));
    out.push_str(&format!(
        "export const YAS_PRE_HELLO_MAX_FRAME = {} as const;\n",
        artifact.transport.limits.pre_hello_frame
    ));
    out.push_str(&format!(
        "export const YAS_HARD_MAX_WIRE_FRAME = {} as const;\n",
        artifact.transport.limits.wire_frame
    ));
    out.push_str(&format!(
        "export const YAS_HARD_MAX_DECODED_FRAME = {} as const;\n",
        artifact.transport.limits.decoded_frame
    ));
    out.push_str(&format!(
        "export const YAS_HARD_MAX_DATAGRAM = {} as const;\n",
        artifact.transport.limits.datagram
    ));
    out.push_str(&format!(
        "export const YAS_HARD_MAX_BULK_CHUNK = {} as const;\n",
        artifact.transport.limits.bulk_chunk
    ));
    out.push_str(&format!(
        "export const YAS_HARD_MAX_BUFFERED = {} as const;\n",
        artifact.transport.limits.buffered
    ));
    out.push_str(&format!(
        "export const YAS_HARD_MAX_EXTENSION_ENTRIES = {} as const;\n",
        artifact.transport.limits.extension_entries
    ));
    out.push_str(&format!(
        "export const YAS_HARD_MAX_TYPED_RECORDS = {} as const;\n",
        artifact.transport.limits.typed_records
    ));
    out.push_str(
        r#"export interface YasGeneratedFrameHeader {
  family: number;
  kind: number;
  class: typeof YAS_CLASS_EVENT | typeof YAS_CLASS_REQUEST | typeof YAS_CLASS_RESULT;
  requestId: number | undefined;
  compressed: boolean;
  sensitive: boolean;
}
function yasGeneratedU16(value: number, name: string): number {
  if (!Number.isInteger(value) || value < 0 || value > 0xffff) throw new RangeError(`${name} is not a u16`);
  return value;
}
export function yasEncodeGeneratedFrameHeader(header: YasGeneratedFrameHeader): Uint8Array {
  const correlated = header.class === YAS_CLASS_REQUEST || header.class === YAS_CLASS_RESULT;
  if (header.class !== YAS_CLASS_EVENT && !correlated) throw new RangeError("invalid YAS frame class");
  if (correlated !== (header.requestId !== undefined)) throw new RangeError("invalid YAS request ID presence");
  if (header.requestId !== undefined && (!Number.isInteger(header.requestId) || header.requestId <= 0 || header.requestId > 0xffff_ffff)) throw new RangeError("invalid YAS request ID");
  const output = new Uint8Array(correlated ? YAS_CORRELATED_HEADER_BYTES : YAS_EVENT_HEADER_BYTES);
  const view = new DataView(output.buffer);
  view.setUint16(0, yasGeneratedU16(header.family, "family"), true);
  view.setUint16(2, yasGeneratedU16(header.kind, "kind"), true);
  view.setUint8(4, header.class | (header.compressed ? YAS_META_COMPRESSED : 0) | (header.sensitive ? YAS_META_SENSITIVE : 0));
  if (header.requestId !== undefined) view.setUint32(5, header.requestId, true);
  return output;
}
export function yasDecodeGeneratedFrameHeader(input: Uint8Array): { header: YasGeneratedFrameHeader; bytesRead: number } {
  if (input.byteLength < YAS_EVENT_HEADER_BYTES) throw new RangeError("truncated YAS frame header");
  const view = new DataView(input.buffer, input.byteOffset, input.byteLength);
  const meta = view.getUint8(4);
  if ((meta & YAS_META_RESERVED) !== 0) throw new RangeError("reserved YAS frame metadata");
  const frameClass = meta & YAS_META_CLASS_MASK;
  const correlated = frameClass === YAS_CLASS_REQUEST || frameClass === YAS_CLASS_RESULT;
  if (frameClass !== YAS_CLASS_EVENT && !correlated) throw new RangeError("invalid YAS frame class");
  const bytesRead = correlated ? YAS_CORRELATED_HEADER_BYTES : YAS_EVENT_HEADER_BYTES;
  if (input.byteLength < bytesRead) throw new RangeError("truncated YAS correlated frame header");
  const requestId = correlated ? view.getUint32(5, true) : undefined;
  if (requestId === 0) throw new RangeError("zero YAS request ID");
  return { header: { family: view.getUint16(0, true), kind: view.getUint16(2, true), class: frameClass as YasGeneratedFrameHeader["class"], requestId, compressed: (meta & YAS_META_COMPRESSED) !== 0, sensitive: (meta & YAS_META_SENSITIVE) !== 0 }, bytesRead };
}
"#,
    );
    out.push_str("export const YAS_DIRECTION_CLIENT_TO_SERVER = 0 as const;\n");
    out.push_str("export const YAS_DIRECTION_SERVER_TO_CLIENT = 1 as const;\n");
    out.push_str("export const YAS_DIRECTION_BIDIRECTIONAL = 2 as const;\n");
    out.push_str("export const YAS_POLICY_ALLOWED = 0 as const;\n");
    out.push_str("export const YAS_POLICY_REQUIRED = 1 as const;\n");
    out.push_str("export const YAS_POLICY_FORBIDDEN = 2 as const;\n");
    out.push_str(&format!(
        "export const YAS_DATAGRAM_FORBIDDEN = {} as const;\n",
        artifact.transport.datagram_predicate.forbidden
    ));
    out.push_str(&format!(
        "export const YAS_DATAGRAM_NET_NATIVE_FLOW = {} as const;\n",
        artifact.transport.datagram_predicate.net_native_flow
    ));
    out.push_str(&format!(
        "export const YAS_DATAGRAM_SURFACE_FRAME = {} as const;\n",
        artifact.transport.datagram_predicate.surface_frame
    ));
    out.push_str(&format!(
        "export const YAS_DATAGRAM_MEDIA_FRAME = {} as const;\n",
        artifact.transport.datagram_predicate.media_frame
    ));
    for codec in &artifact.transport.codec {
        out.push_str(&format!(
            "export const YAS_CODEC_{} = {} as const;\n",
            codec.name, codec.id
        ));
    }
    for codec in &artifact.codecs {
        out.push_str(&format!(
            "export const YAS_PACKED_CODEC_{} = {} as const;\n",
            codec.const_name, codec.id
        ));
        out.push_str(&format!(
            "export const YAS_PACKED_CODEC_{}_VERSION = {} as const;\n",
            codec.const_name, codec.version
        ));
        out.push_str(&format!(
            "export const YAS_{} = YAS_PACKED_CODEC_{};\n",
            codec.const_name, codec.const_name
        ));
        for constant in &codec.constants {
            out.push_str(&format!(
                "export const YAS_PACKED_CODEC_{}_{} = {} as const;\n",
                codec.const_name, constant.name, constant.value
            ));
        }
    }
    for family in &artifact.families {
        let codecs = artifact
            .codecs
            .iter()
            .filter(|codec| codec.family == family.name)
            .collect::<Vec<_>>();
        let mut counts = BTreeMap::new();
        for constant in codecs.iter().flat_map(|codec| &codec.constants) {
            *counts.entry(constant.name.as_str()).or_insert(0usize) += 1;
        }
        for codec in codecs {
            for constant in &codec.constants {
                if counts[constant.name.as_str()] == 1
                    && !family
                        .constants
                        .iter()
                        .any(|value| value.name == constant.name)
                {
                    out.push_str(&format!(
                        "export const YAS_{}_{} = YAS_PACKED_CODEC_{}_{};\n",
                        family.const_name, constant.name, codec.const_name, constant.name
                    ));
                }
            }
        }
    }
    for constant in &artifact.state.constants {
        out.push_str(&format!(
            "export const YAS_STATE_{} = {} as const;\n",
            constant.name, constant.value
        ));
    }
    for family in &artifact.families {
        let prefix = family.const_name.as_str();
        out.push_str(&format!(
            "export const YAS_FAMILY_{prefix} = {} as const;\n",
            family.id
        ));
        out.push_str(&format!(
            "export const YAS_{prefix}_VERSION = {} as const;\n",
            family.version
        ));
        for operation in family.requests.iter().chain(&family.events) {
            out.push_str(&format!(
                "export const YAS_{prefix}_{} = {} as const;\n",
                operation.name, operation.kind
            ));
        }
        for constant in &family.constants {
            out.push_str(&format!(
                "export const YAS_{prefix}_{} = {} as const;\n",
                constant.name, constant.value
            ));
        }
    }
    for status in &artifact.statuses {
        out.push_str(&format!(
            "export const YAS_STATUS_{} = {} as const;\n",
            status.name, status.code
        ));
    }
    out.push_str(
        "export const YAS_FAMILY_DEPENDENCIES: Readonly<Record<number, readonly number[]>> = {\n",
    );
    for family in &artifact.families {
        out.push_str(&format!("  {}: {:?},\n", family.id, family.dependencies));
    }
    out.push_str("};\n");
    out.push_str(
        "export type YasFamilyLimitPolicy = readonly [tag: number, width: 4 | 8, required: boolean, hardMin: bigint, hardMax: bigint];\n",
    );
    out.push_str(
        "export const YAS_FAMILY_LIMIT_POLICIES: Readonly<Record<number, readonly YasFamilyLimitPolicy[]>> = {\n",
    );
    for family in &artifact.families {
        out.push_str(&format!("  {}: [\n", family.id));
        for limit in &family.limits {
            out.push_str(&format!(
                "    [{}, {}, {}, {}n, {}n],\n",
                limit.tag,
                limit.value_type.width(),
                limit.required,
                limit.hard_min,
                limit.hard_max
            ));
        }
        out.push_str("  ],\n");
    }
    out.push_str("};\n");
    out.push_str(
        "export type YasOperationPolicy = readonly [sensitive: 0 | 1 | 2, compression: 0 | 1 | 2, datagram: number];\n",
    );
    out.push_str(
        "export const YAS_OPERATION_POLICIES: Readonly<Record<string, YasOperationPolicy>> = {\n",
    );
    for family in &artifact.families {
        for operation in &family.requests {
            for class in [
                artifact.transport.class.request,
                artifact.transport.class.result,
            ] {
                out.push_str(&format!(
                    "  {:?}: [{}, {}, {}],\n",
                    format!("{}/{class}/{}", family.id, operation.kind),
                    operation.sensitive.wire(),
                    operation.compression.wire(),
                    operation
                        .datagram
                        .wire(&artifact.transport.datagram_predicate)
                ));
            }
        }
        for operation in &family.events {
            out.push_str(&format!(
                "  {:?}: [{}, {}, {}],\n",
                format!(
                    "{}/{}/{}",
                    family.id, artifact.transport.class.event, operation.kind
                ),
                operation.sensitive.wire(),
                operation.compression.wire(),
                operation
                    .datagram
                    .wire(&artifact.transport.datagram_predicate)
            ));
        }
    }
    out.push_str("};\n");
    out.push_str(
        "export const YAS_OPERATION_DIRECTION_MASKS: Readonly<Record<string, number>> = {\n",
    );
    for family in &artifact.families {
        for (class, operations) in [
            (artifact.transport.class.request, &family.requests),
            (artifact.transport.class.event, &family.events),
        ] {
            for operation in operations {
                let direction = match operation.direction {
                    Direction::ClientToServer => 1,
                    Direction::ServerToClient => 2,
                    Direction::Bidirectional => 3,
                };
                out.push_str(&format!(
                    "  {:?}: {},\n",
                    format!("{}/{class}/{}", family.id, operation.kind),
                    direction
                ));
            }
        }
    }
    out.push_str("};\n");
    out.push_str("export const YAS_SCHEMA = ");
    out.push_str(json.trim_end());
    out.push_str(" as const;\nexport const YAS_GOLDEN_VECTORS = ");
    out.push_str(vectors.trim_end());
    out.push_str(" as const;\n");
    out
}
