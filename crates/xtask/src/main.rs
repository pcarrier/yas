#[path = "../../yas/compatibility.rs"]
mod compatibility;
#[path = "../../yas/schema_codegen.rs"]
mod schema_codegen;

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

struct Artifact<'a> {
    path: PathBuf,
    contents: &'a str,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("xtask: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args().skip(1);
    let Some(command) = arguments.next() else {
        return Err(usage());
    };
    if command != "protocol" {
        return Err(format!("unknown command {command:?}\n{}", usage()));
    }
    let mut check = false;
    let mut bless_baseline = false;
    for argument in arguments {
        match argument.as_str() {
            "--check" => check = true,
            "--bless-baseline" => bless_baseline = true,
            "--help" | "-h" => {
                println!("{}", usage());
                return Ok(());
            }
            _ => return Err(format!("unknown protocol option {argument:?}\n{}", usage())),
        }
    }
    if check && bless_baseline {
        return Err("--check and --bless-baseline are mutually exclusive".into());
    }

    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema_dir = workspace.join("protocol/yas");
    let generated = schema_codegen::generate(&schema_dir)
        .map_err(|error| format!("invalid canonical YAS schema: {error}"))?;
    let baseline_path = schema_dir.join("history/v1.json");
    if bless_baseline {
        if baseline_path.exists() {
            return Err(format!(
                "refusing to replace immutable compatibility baseline {}",
                relative(&workspace, &baseline_path)
            ));
        }
        write(&baseline_path, &generated.json)?;
        println!("blessed {}", relative(&workspace, &baseline_path));
    } else {
        let baseline = fs::read_to_string(&baseline_path).map_err(|error| {
            format!(
                "missing compatibility baseline {}: {error}; use --bless-baseline for the initial v1 baseline",
                relative(&workspace, &baseline_path)
            )
        })?;
        compatibility::check(&baseline, &generated.json)?;
    }

    let artifacts = [
        Artifact {
            path: schema_dir.join("schema.json"),
            contents: &generated.json,
        },
        Artifact {
            path: schema_dir.join("vectors.json"),
            contents: &generated.vectors,
        },
        Artifact {
            path: schema_dir.join("generated.rs"),
            contents: &generated.rust,
        },
        Artifact {
            path: schema_dir.join("wire.md"),
            contents: &generated.markdown,
        },
        Artifact {
            path: schema_dir.join("inspection.json"),
            contents: &generated.inspection,
        },
        Artifact {
            path: workspace.join("js/core/src/yas/generated.ts"),
            contents: &generated.typescript,
        },
    ];

    let mut stale = Vec::new();
    for artifact in artifacts {
        if check {
            match fs::read_to_string(&artifact.path) {
                Ok(actual) if actual == artifact.contents => {}
                _ => stale.push(relative(&workspace, &artifact.path)),
            }
        } else {
            write(&artifact.path, artifact.contents)?;
        }
    }
    if !stale.is_empty() {
        return Err(format!(
            "generated YAS artifacts are stale:\n- {}\nrun `cargo xtask protocol`",
            stale.join("\n- ")
        ));
    }
    if check {
        println!("YAS schema, compatibility baseline, and generated artifacts are current");
    } else {
        println!("generated {} deterministic YAS artifacts", artifacts_len());
    }
    Ok(())
}

const fn artifacts_len() -> usize {
    6
}

fn write(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::write(path, contents).map_err(|error| format!("write {}: {error}", path.display()))
}

fn relative(workspace: &Path, path: &Path) -> String {
    path.strip_prefix(workspace)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn usage() -> String {
    "usage: cargo xtask protocol [--check | --bless-baseline]\n\
     \n\
     Generate canonical YAS artifacts, or verify checked-in artifacts and the\n\
     immutable v1 wire baseline. --bless-baseline is only for establishing a\n\
     new retained protocol-major baseline, and refuses to overwrite one."
        .into()
}
