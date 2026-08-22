use std::collections::BTreeMap;

use serde_json::Value;

/// Check that `current` preserves every wire contract recorded in `baseline`.
///
/// A baseline is a previously shipped `schema.json`. New families, operations,
/// limits, constants, types, statuses, and packed codecs may be appended, but
/// every existing identifier and its normative metadata must remain present
/// and unchanged. This deliberately errs on the conservative side: a change
/// that is safe only with semantic knowledge requires a new protocol or family
/// version and a separately retained baseline.
pub fn check(baseline: &str, current: &str) -> Result<(), String> {
    let baseline: Value = serde_json::from_str(baseline)
        .map_err(|error| format!("invalid compatibility baseline: {error}"))?;
    let current: Value = serde_json::from_str(current)
        .map_err(|error| format!("invalid generated schema: {error}"))?;
    let mut errors = Vec::new();

    compare_fields(&baseline, &current, "schema", &["schema"], &mut errors);

    if let (Some(old), Some(new)) = (
        object_field(&baseline, "transport", "schema", &mut errors),
        object_field(&current, "transport", "schema", &mut errors),
    ) {
        compare_fields(
            old,
            new,
            "transport",
            &[
                "schema",
                "preface_hex",
                "protocol_major",
                "websocket_subprotocol",
                "stream_length_bits",
                "event_header_bytes",
                "correlated_header_bytes",
                "header",
                "class",
                "meta",
                "datagram_predicate",
                "limits",
            ],
            &mut errors,
        );
        compare_array_entries(
            old,
            new,
            "codec",
            &["id"],
            &["name", "id"],
            "transport.codec",
            &mut errors,
        );
    }

    if let (Some(old), Some(new)) = (
        object_field(&baseline, "state", "schema", &mut errors),
        object_field(&current, "state", "schema", &mut errors),
    ) {
        compare_fields(old, new, "state", &["schema"], &mut errors);
        compare_array_entries(
            old,
            new,
            "constant",
            &["name"],
            &["name", "value"],
            "state.constants",
            &mut errors,
        );
        compare_array_entries(
            old,
            new,
            "type",
            &["name"],
            &["name", "layout"],
            "state.types",
            &mut errors,
        );
    }

    compare_array_entries(
        &baseline,
        &current,
        "statuses",
        &["code"],
        &["name", "code"],
        "statuses",
        &mut errors,
    );
    compare_array_entries(
        &baseline,
        &current,
        "codecs",
        &["family", "id"],
        &[
            "schema",
            "name",
            "const_name",
            "family",
            "id",
            "version",
            "direction",
            "layout",
            "golden_hex",
            "constants",
        ],
        "codecs",
        &mut errors,
    );

    compare_families(&baseline, &current, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "YAS wire compatibility check failed:\n- {}",
            errors.join("\n- ")
        ))
    }
}

fn compare_families(baseline: &Value, current: &Value, errors: &mut Vec<String>) {
    let Some(old_families) = array_field(baseline, "families", "schema", errors) else {
        return;
    };
    let Some(new_families) = array_field(current, "families", "schema", errors) else {
        return;
    };
    let Ok(new_by_id) = index(new_families, &["id"], "families") else {
        errors.push("current families cannot be indexed by id".into());
        return;
    };

    for old in old_families {
        let Ok(key) = entry_key(old, &["id"], "families") else {
            errors.push("baseline family lacks an id".into());
            continue;
        };
        let Some(new) = new_by_id.get(&key).copied() else {
            errors.push(format!("removed family id {key}"));
            continue;
        };
        let name = old
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("<unknown>");
        let path = format!("family {name} ({key})");
        compare_fields(
            old,
            new,
            &path,
            &["name", "const_name", "id", "version", "dependencies"],
            errors,
        );
        for (field, keys, fields) in [
            (
                "requests",
                &["kind"][..],
                &[
                    "name",
                    "kind",
                    "direction",
                    "sensitive",
                    "compression",
                    "datagram",
                    "layout",
                ][..],
            ),
            (
                "events",
                &["kind"][..],
                &[
                    "name",
                    "kind",
                    "direction",
                    "sensitive",
                    "compression",
                    "datagram",
                    "layout",
                ][..],
            ),
            (
                "limits",
                &["tag"][..],
                &["name", "tag", "type", "required", "hard_min", "hard_max"][..],
            ),
            ("types", &["name"][..], &["name", "layout"][..]),
            ("constants", &["name"][..], &["name", "value"][..]),
        ] {
            compare_array_entries(
                old,
                new,
                field,
                keys,
                fields,
                &format!("{path}.{field}"),
                errors,
            );
        }
    }
}

fn compare_array_entries(
    baseline: &Value,
    current: &Value,
    field: &str,
    keys: &[&str],
    fields: &[&str],
    path: &str,
    errors: &mut Vec<String>,
) {
    let Some(old_entries) = array_field(baseline, field, path, errors) else {
        return;
    };
    let Some(new_entries) = array_field(current, field, path, errors) else {
        return;
    };
    let new_by_key = match index(new_entries, keys, path) {
        Ok(values) => values,
        Err(error) => {
            errors.push(error);
            return;
        }
    };
    for old in old_entries {
        let key = match entry_key(old, keys, path) {
            Ok(key) => key,
            Err(error) => {
                errors.push(error);
                continue;
            }
        };
        let Some(new) = new_by_key.get(&key).copied() else {
            errors.push(format!("removed {path} entry {key}"));
            continue;
        };
        compare_fields(old, new, &format!("{path}[{key}]"), fields, errors);
    }
}

fn compare_fields(
    baseline: &Value,
    current: &Value,
    path: &str,
    fields: &[&str],
    errors: &mut Vec<String>,
) {
    for field in fields {
        let old = baseline.get(*field);
        let new = current.get(*field);
        if old != new {
            errors.push(format!(
                "changed {path}.{field}: baseline {} current {}",
                display(old),
                display(new)
            ));
        }
    }
}

fn index<'a>(
    entries: &'a [Value],
    keys: &[&str],
    path: &str,
) -> Result<BTreeMap<String, &'a Value>, String> {
    let mut indexed = BTreeMap::new();
    for entry in entries {
        let key = entry_key(entry, keys, path)?;
        if indexed.insert(key.clone(), entry).is_some() {
            return Err(format!("duplicate {path} compatibility key {key}"));
        }
    }
    Ok(indexed)
}

fn entry_key(entry: &Value, keys: &[&str], path: &str) -> Result<String, String> {
    keys.iter()
        .map(|key| {
            let value = entry
                .get(*key)
                .ok_or_else(|| format!("{path} entry lacks key {key}"))?;
            match value {
                Value::String(value) => Ok(value.clone()),
                Value::Number(value) => Ok(value.to_string()),
                _ => Err(format!("{path} key {key} is not scalar")),
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|parts| parts.join("/"))
}

fn object_field<'a>(
    value: &'a Value,
    field: &str,
    path: &str,
    errors: &mut Vec<String>,
) -> Option<&'a Value> {
    let value = value.get(field);
    if value.is_none_or(|value| !value.is_object()) {
        errors.push(format!("{path}.{field} is missing or not an object"));
        None
    } else {
        value
    }
}

fn array_field<'a>(
    value: &'a Value,
    field: &str,
    path: &str,
    errors: &mut Vec<String>,
) -> Option<&'a [Value]> {
    let value = value.get(field).and_then(Value::as_array);
    if value.is_none() {
        errors.push(format!("{path}.{field} is missing or not an array"));
    }
    value.map(Vec::as_slice)
}

fn display(value: Option<&Value>) -> String {
    value.map_or_else(|| "<missing>".into(), Value::to_string)
}
