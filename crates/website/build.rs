use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn collect(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read web dist") {
        let path = entry.expect("read web dist entry").path();
        if path.is_dir() {
            collect(root, &path, files);
        } else if path.is_file() {
            files.push(path.strip_prefix(root).unwrap().to_path_buf());
        }
    }
}

fn main() {
    println!("cargo:rerun-if-env-changed=YAS_WEB_DIST");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let dist = env::var_os("YAS_WEB_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../js/web/dist"));
    println!("cargo:rerun-if-changed={}", dist.display());

    if !dist.join("index.html").is_file() {
        panic!(
            "{} is missing; run `pnpm --dir js --filter yas-web build` first",
            dist.join("index.html").display()
        );
    }

    let dist = dist.canonicalize().expect("canonical web dist path");
    let mut files = Vec::new();
    collect(&dist, &dist, &mut files);
    files.sort();

    let mut generated = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    for relative in files {
        let route = relative.to_string_lossy().replace('\\', "/");
        let source = dist.join(&relative);
        generated.push_str(&format!(
            "    ({route:?}, include_bytes!({source:?})),\n",
            source = source.to_string_lossy()
        ));
    }
    generated.push_str("];\n");

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("assets.rs");
    fs::write(out, generated).expect("write embedded asset table");
}
