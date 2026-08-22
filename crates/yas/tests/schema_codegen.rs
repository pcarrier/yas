#[path = "../schema_codegen.rs"]
mod schema_codegen;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn canonical() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../protocol/yas")
}

fn temporary_schema() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("yas-wire-schema-{}-{nonce}", std::process::id()));
    fs::create_dir_all(path.join("families")).unwrap();
    fs::create_dir_all(path.join("codecs")).unwrap();
    for name in ["registry.toml", "transport.toml", "state.toml"] {
        fs::copy(canonical().join(name), path.join(name)).unwrap();
    }
    for entry in fs::read_dir(canonical().join("families")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), path.join("families").join(entry.file_name())).unwrap();
    }
    for entry in fs::read_dir(canonical().join("codecs")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), path.join("codecs").join(entry.file_name())).unwrap();
    }
    path
}

#[test]
fn every_registered_family_requires_a_specification() {
    let path = temporary_schema();
    fs::remove_file(path.join("families/media.toml")).unwrap();
    let error = match schema_codegen::generate(&path) {
        Ok(_) => panic!("accepted registry family without a specification"),
        Err(error) => error,
    };
    assert!(error.contains("missing specification for yas.media"));
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn canonical_schema_parses_and_generates_every_artifact() {
    let generated = schema_codegen::generate(&canonical()).unwrap();
    assert!(generated.rust.contains("pub mod transport"));
    assert!(generated.rust.contains("pub static CODECS"));
    assert!(generated.typescript.contains("YAS_RELAY_CONNECT"));
    assert!(generated.typescript.contains("YAS_TERMINAL_GRID_CODEC_V1"));
    assert!(
        generated
            .typescript
            .contains("YAS_CORE_SERVER_HELLO_NEGOTIATED_CODECS_EXTENSION")
    );
    assert!(generated.typescript.contains("YAS_RECOMMENDED_WIRE_FRAME"));
    assert!(generated.typescript.contains("YAS_DATAGRAM_SURFACE_FRAME"));
    assert!(generated.typescript.contains("YAS_FAMILY_DEPENDENCIES"));
    assert!(generated.typescript.contains("YAS_FAMILY_LIMIT_POLICIES"));
    assert!(
        generated
            .typescript
            .contains("YAS_OPERATION_DIRECTION_MASKS")
    );
    assert!(generated.typescript.contains("YAS_OPERATION_POLICIES"));
    assert!(generated.json.contains("yas.font"));
    assert!(generated.markdown.contains("# YAS v1 wire registry"));
    assert!(generated.markdown.contains("`yas.net`"));
    assert!(generated.inspection.contains("\"class\": \"result\""));
    assert!(generated.inspection.contains("\"sensitive\": \"required\""));
    assert!(
        generated
            .json
            .contains("\"dependencies\": [\n        1,\n        66\n      ]")
    );
    assert!(generated.rust.contains("dependencies: &[1, 66]"));
    assert!(generated.rust.contains(
        "LimitMetadata { name: \"MAX_ROUTES\", tag: 1, value_type: super::LimitValueType::U32, required: true, hard_min: 1, hard_max: 65536 }"
    ));
    assert!(generated.vectors.contains("core.client_hello.payload"));
    assert!(generated.vectors.contains("core.session_update.payload"));
    assert!(generated.vectors.contains("transport.shutdown.frame"));
    assert!(
        generated
            .vectors
            .contains("packed_codec.terminal-grid-v1.payload")
    );
}

#[test]
fn family_limit_policies_are_complete_and_bounded() {
    let path = temporary_schema();
    let channel_path = path.join("families/channel.toml");
    let channel = fs::read_to_string(&channel_path).unwrap();
    let missing = channel.replacen(
        "  { name = \"MAX_NAME_BYTES\", tag = 1, type = \"u32\", required = true, hard_min = 1, hard_max = 255 },\n",
        "",
        1,
    );
    assert_ne!(missing, channel);
    fs::write(&channel_path, missing).unwrap();
    let error = schema_codegen::generate(&path).unwrap_err();
    assert!(error.contains("every LIMIT_ constant"));
    fs::remove_dir_all(path).unwrap();

    let path = temporary_schema();
    let channel_path = path.join("families/channel.toml");
    let channel = fs::read_to_string(&channel_path).unwrap();
    let mismatched = channel.replacen("tag = 1, type = \"u32\"", "tag = 2, type = \"u32\"", 1);
    assert_ne!(mismatched, channel);
    fs::write(&channel_path, mismatched).unwrap();
    let error = schema_codegen::generate(&path).unwrap_err();
    assert!(error.contains("invalid limit policy MAX_NAME_BYTES"));
    fs::remove_dir_all(path).unwrap();

    let path = temporary_schema();
    let channel_path = path.join("families/channel.toml");
    let channel = fs::read_to_string(&channel_path).unwrap();
    let oversized = channel.replacen("hard_max = 255", "hard_max = 4294967296", 1);
    assert_ne!(oversized, channel);
    fs::write(&channel_path, oversized).unwrap();
    let error = schema_codegen::generate(&path).unwrap_err();
    assert!(error.contains("invalid limit policy MAX_NAME_BYTES"));
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn family_dependencies_are_canonical_and_acyclic() {
    let path = temporary_schema();
    let registry = fs::read_to_string(path.join("registry.toml")).unwrap();
    let invalid = registry.replacen(
        "dependencies = [\"yas.transfer\"]",
        "dependencies = [\"yas.unknown\"]",
        1,
    );
    fs::write(path.join("registry.toml"), invalid).unwrap();
    let error = match schema_codegen::generate(&path) {
        Ok(_) => panic!("accepted an unknown family dependency"),
        Err(error) => error,
    };
    assert!(error.contains("unknown dependency yas.unknown"));
    fs::remove_dir_all(path).unwrap();

    let path = temporary_schema();
    let registry = fs::read_to_string(path.join("registry.toml")).unwrap();
    let invalid = registry.replacen(
        "dependencies = [\"yas.transfer\"]",
        "dependencies = [\"yas.relay\"]",
        1,
    );
    fs::write(path.join("registry.toml"), invalid).unwrap();
    let error = match schema_codegen::generate(&path) {
        Ok(_) => panic!("accepted a forward family dependency"),
        Err(error) => error,
    };
    assert!(error.contains("must be unique, ordered, and precede"));
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn duplicate_registry_ids_and_invalid_toml_are_rejected() {
    let path = temporary_schema();
    let registry = fs::read_to_string(path.join("registry.toml")).unwrap();
    let duplicate = registry.replacen("id = 0x0001", "id = 0x0000", 1);
    fs::write(path.join("registry.toml"), duplicate).unwrap();
    assert!(schema_codegen::generate(&path).is_err());

    fs::write(path.join("registry.toml"), "not = [valid").unwrap();
    assert!(schema_codegen::generate(&path).is_err());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn invalid_transport_resource_limits_are_rejected() {
    let path = temporary_schema();
    let transport = fs::read_to_string(path.join("transport.toml")).unwrap();
    let invalid = transport.replace("buffered = 1073741824", "buffered = 0");
    fs::write(path.join("transport.toml"), invalid).unwrap();
    assert!(schema_codegen::generate(&path).is_err());
    fs::remove_dir_all(path).unwrap();

    let path = temporary_schema();
    let transport = fs::read_to_string(path.join("transport.toml")).unwrap();
    let invalid = transport.replace("decoded_frame = 4194304", "decoded_frame = 524288");
    fs::write(path.join("transport.toml"), invalid).unwrap();
    assert!(schema_codegen::generate(&path).is_err());
    fs::remove_dir_all(path).unwrap();

    let path = temporary_schema();
    let transport = fs::read_to_string(path.join("transport.toml")).unwrap();
    let invalid = transport.replace(
        "preface_hex = \"5941530001000d0a\"",
        "preface_hex = \"5941530002000d0a\"",
    );
    fs::write(path.join("transport.toml"), invalid).unwrap();
    assert!(schema_codegen::generate(&path).is_err());
    fs::remove_dir_all(path).unwrap();

    let path = temporary_schema();
    let transport = fs::read_to_string(path.join("transport.toml")).unwrap();
    let invalid = transport.replace("reserved_mask = 0xf0", "reserved_mask = 0x00");
    fs::write(path.join("transport.toml"), invalid).unwrap();
    assert!(schema_codegen::generate(&path).is_err());
    fs::remove_dir_all(path).unwrap();
}

#[test]
fn every_operation_requires_explicit_transport_policies() {
    for policy in [
        "sensitive = \"allowed\"\n",
        "compression = \"forbidden\"\n",
        "datagram = \"forbidden\"\n",
    ] {
        let path = temporary_schema();
        let core_path = path.join("families/core.toml");
        let core = fs::read_to_string(&core_path).unwrap();
        let invalid = core.replacen(policy, "", 1);
        assert_ne!(invalid, core, "test fixture does not contain {policy:?}");
        fs::write(core_path, invalid).unwrap();
        assert!(schema_codegen::generate(&path).is_err());
        fs::remove_dir_all(path).unwrap();
    }
}
