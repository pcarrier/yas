use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const MODULES: &[&str] = &[
    "core",
    "transfer",
    "state",
    "relay",
    "font",
    "terminal",
    "client",
    "surface",
    "selection",
    "desktop",
    "media",
    "fs",
    "git",
    "lsp",
    "kv",
    "process",
    "net",
    "channel",
    "extension",
    "events",
    "env",
];

fn main() {
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let source = manifest.join("../crates/yas/src");
    let mut decoders = BTreeSet::from([
        "yas_wire::Extensions".to_owned(),
        "yas_wire::core::Ping".to_owned(),
    ]);

    for module in MODULES {
        let path = source.join(format!("{module}.rs"));
        println!("cargo:rerun-if-changed={}", path.display());
        collect_decoders(&path, module, &mut decoders);
    }

    let mut generated = String::from(
        "type Decoder = fn(&[u8]);\n\nfn decode<T: yas_wire::Decode>(input: &[u8]) {\n    drop(T::decode(input));\n}\n\nconst DECODERS: &[Decoder] = &[\n",
    );
    for decoder in &decoders {
        generated.push_str("    decode::<");
        generated.push_str(decoder);
        generated.push_str(">,\n");
    }
    generated.push_str("];\n");

    let output = PathBuf::from(env::var_os("OUT_DIR").expect("output directory"));
    fs::write(output.join("family_decoders.rs"), generated).expect("write decoder registry");
}

fn collect_decoders(path: &Path, module: &str, decoders: &mut BTreeSet<String>) {
    let source = fs::read_to_string(path).expect("read YAS codec module");
    for line in source.lines() {
        let Some(rest) = line.trim().strip_prefix("impl Decode for ") else {
            continue;
        };
        let name = rest
            .split(|character: char| character.is_whitespace() || character == '{')
            .next()
            .expect("decoder type");
        if name.starts_with('$') {
            continue;
        }
        assert!(
            name.chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_'),
            "unsupported decoder type in {}: {name}",
            path.display()
        );
        decoders.insert(format!("yas_wire::{module}::{name}"));
    }
}
