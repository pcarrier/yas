mod compatibility;
mod schema_codegen;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=YAS_UPDATE_SCHEMA");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let workspace = manifest.join("../..");
    let schema_dir = workspace.join("protocol/yas");
    for name in [
        "registry.toml",
        "transport.toml",
        "state.toml",
        "families/core.toml",
        "families/transfer.toml",
        "families/relay.toml",
        "families/font.toml",
    ] {
        println!("cargo:rerun-if-changed={}", schema_dir.join(name).display());
    }
    println!(
        "cargo:rerun-if-changed={}",
        schema_dir.join("families").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        schema_dir.join("codecs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        schema_dir.join("history/v1.json").display()
    );

    let generated = schema_codegen::generate(&schema_dir).unwrap_or_else(|error| {
        panic!("invalid canonical YAS schema: {error}");
    });
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out_dir.join("yas_schema.rs"), &generated.rust).unwrap();
    fs::write(out_dir.join("yas_schema.json"), &generated.json).unwrap();
    fs::write(out_dir.join("yas_vectors.json"), &generated.vectors).unwrap();
    fs::write(out_dir.join("yas_schema.ts"), &generated.typescript).unwrap();
    fs::write(out_dir.join("yas_wire.md"), &generated.markdown).unwrap();
    fs::write(out_dir.join("yas_inspection.json"), &generated.inspection).unwrap();

    let update = env::var_os("YAS_UPDATE_SCHEMA").as_deref() == Some(std::ffi::OsStr::new("1"));
    check_or_update(&schema_dir.join("schema.json"), &generated.json, update);
    check_or_update(&schema_dir.join("vectors.json"), &generated.vectors, update);
    check_or_update(&schema_dir.join("generated.rs"), &generated.rust, update);
    check_or_update(&schema_dir.join("wire.md"), &generated.markdown, update);
    check_or_update(
        &schema_dir.join("inspection.json"),
        &generated.inspection,
        update,
    );
    check_or_update(
        &workspace.join("js/core/src/yas/generated.ts"),
        &generated.typescript,
        update,
    );
    let baseline_path = schema_dir.join("history/v1.json");
    let baseline = fs::read_to_string(&baseline_path).unwrap_or_else(|_| {
        panic!(
            "missing YAS v1 compatibility baseline {}; run `cargo xtask protocol --bless-baseline`",
            baseline_path.display()
        )
    });
    compatibility::check(&baseline, &generated.json).unwrap_or_else(|error| panic!("{error}"));
}

fn check_or_update(path: &Path, expected: &str, update: bool) {
    if update {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, expected).unwrap();
        return;
    }
    let actual = fs::read_to_string(path).unwrap_or_else(|_| {
        panic!(
            "missing generated YAS artifact {}; run `cargo xtask protocol`",
            path.display()
        )
    });
    assert_eq!(
        actual,
        expected,
        "generated YAS artifact {} is stale; run `cargo xtask protocol`",
        path.display()
    );
}
