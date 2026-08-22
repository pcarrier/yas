#[path = "../compatibility.rs"]
mod compatibility;

use std::path::{Path, PathBuf};

fn canonical(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../protocol/yas")
        .join(name)
}

fn baseline() -> String {
    std::fs::read_to_string(canonical("history/v1.json")).unwrap()
}

fn current() -> String {
    std::fs::read_to_string(canonical("schema.json")).unwrap()
}

fn mutate(mutator: impl FnOnce(&mut serde_json::Value)) -> String {
    let mut value: serde_json::Value = serde_json::from_str(&current()).unwrap();
    mutator(&mut value);
    serde_json::to_string_pretty(&value).unwrap()
}

#[test]
fn canonical_schema_preserves_v1() {
    compatibility::check(&baseline(), &current()).unwrap();
}

#[test]
fn rejects_family_id_reuse_and_removed_versions() {
    let reused = mutate(|schema| {
        schema["families"][0]["name"] = "yas.reused".into();
    });
    assert!(
        compatibility::check(&baseline(), &reused)
            .unwrap_err()
            .contains("changed family yas.core (0).name")
    );

    let removed = mutate(|schema| {
        schema["families"].as_array_mut().unwrap().remove(0);
    });
    assert!(
        compatibility::check(&baseline(), &removed)
            .unwrap_err()
            .contains("removed family id 0")
    );

    let version = mutate(|schema| {
        schema["families"][0]["version"] = 2.into();
    });
    assert!(
        compatibility::check(&baseline(), &version)
            .unwrap_err()
            .contains(".version")
    );
}

#[test]
fn rejects_required_layout_and_semantics_changes() {
    let layout = mutate(|schema| {
        schema["families"][0]["requests"][0]["layout"] = "changed".into();
    });
    assert!(
        compatibility::check(&baseline(), &layout)
            .unwrap_err()
            .contains(".layout")
    );

    let sensitivity = mutate(|schema| {
        schema["families"][0]["requests"][0]["sensitive"] = "required".into();
    });
    assert!(
        compatibility::check(&baseline(), &sensitivity)
            .unwrap_err()
            .contains(".sensitive")
    );

    let datagram = mutate(|schema| {
        schema["families"][5]["events"][0]["datagram"] = "media_frame".into();
    });
    assert!(
        compatibility::check(&baseline(), &datagram)
            .unwrap_err()
            .contains(".datagram")
    );
}

#[test]
fn rejects_extension_tag_and_status_reuse() {
    let extension = mutate(|schema| {
        let constants = schema["families"][0]["constants"].as_array_mut().unwrap();
        constants
            .iter_mut()
            .find(|constant| constant["name"] == "CLIENT_HELLO_PLATFORM_EXTENSION")
            .unwrap()["value"] = 99.into();
    });
    assert!(
        compatibility::check(&baseline(), &extension)
            .unwrap_err()
            .contains("CLIENT_HELLO_PLATFORM_EXTENSION")
    );

    let status = mutate(|schema| {
        schema["statuses"][0]["name"] = "REUSED".into();
    });
    assert!(
        compatibility::check(&baseline(), &status)
            .unwrap_err()
            .contains("statuses[0].name")
    );
}

#[test]
fn permits_strictly_new_ids() {
    let extended = mutate(|schema| {
        let mut operation = schema["families"][0]["requests"][0].clone();
        operation["name"] = "FUTURE".into();
        operation["kind"] = 65535.into();
        schema["families"][0]["requests"]
            .as_array_mut()
            .unwrap()
            .push(operation);
    });
    compatibility::check(&baseline(), &extended).unwrap();
}
