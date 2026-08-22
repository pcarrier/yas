use std::collections::{BTreeMap, VecDeque};

use serde::Deserialize;
use serde_json::Value;

const EVENT_CAP: usize = 500;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct MusterRun {
    #[serde(deserialize_with = "handle")]
    pub pty: u64,
    pub exit_code: Option<i32>,
    pub seq: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(super) struct MusterSurface {
    #[serde(deserialize_with = "handle")]
    pub id: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct MusterUnit {
    pub name: String,
    #[serde(default)]
    pub instance: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub phase: String,
    #[serde(default, deserialize_with = "optional_handle")]
    pub pty: Option<u64>,
    #[serde(default)]
    pub restarts: u64,
    #[serde(default)]
    pub last_exit: Option<i32>,
    #[serde(default)]
    pub requires: Vec<String>,
    #[serde(default = "yes")]
    pub autostart: bool,
    #[serde(default)]
    pub stale: bool,
    #[serde(default = "simple", rename = "type")]
    pub unit_type: String,
    #[serde(default)]
    pub surfaces: Vec<MusterSurface>,
    #[serde(default)]
    pub runs: Vec<MusterRun>,
}

impl MusterUnit {
    pub(super) fn preview_terminal(&self) -> Option<u64> {
        self.pty.or_else(|| self.runs.first().map(|run| run.pty))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub(super) struct MusterInstance {
    pub name: String,
    #[serde(default)]
    pub stack: String,
    #[serde(default)]
    pub members: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct MusterEvent {
    pub seq: u64,
    #[serde(default)]
    pub ts: u64,
    #[serde(default)]
    pub unit: String,
    #[serde(default)]
    pub event: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub instance: Option<String>,
    #[serde(default)]
    pub cause: Option<String>,
    #[serde(default, deserialize_with = "optional_handle")]
    pub pty: Option<u64>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Default)]
pub(super) struct MusterState {
    pub units: BTreeMap<String, MusterUnit>,
    pub instances: BTreeMap<String, MusterInstance>,
    pub events: VecDeque<MusterEvent>,
    pub dir: String,
    pub ready: bool,
}

impl MusterState {
    pub(super) fn apply(&mut self, bytes: &[u8]) -> Result<(), String> {
        let frame: Value = serde_json::from_slice(bytes)
            .map_err(|error| format!("invalid JSON from Muster: {error}"))?;
        let object = frame
            .as_object()
            .ok_or_else(|| "Muster Channel message is not an object".to_string())?;
        match object.get("type").and_then(Value::as_str) {
            Some("hello") => {
                if object.get("version").and_then(Value::as_u64) != Some(1) {
                    return Err("unsupported Muster Channel version".into());
                }
                self.dir = string(object.get("dir"));
            }
            Some("state") => {
                let full = object.get("full").and_then(Value::as_bool) == Some(true);
                if full {
                    self.units.clear();
                    self.instances.clear();
                }
                if let Some(units) = object.get("units").and_then(Value::as_array) {
                    for value in units {
                        if let Ok(unit) = serde_json::from_value::<MusterUnit>(value.clone())
                            && !unit.name.is_empty()
                        {
                            self.units.insert(unit.name.clone(), unit);
                        }
                    }
                }
                if let Some(gone) = object.get("gone").and_then(Value::as_array) {
                    for name in gone.iter().filter_map(Value::as_str) {
                        self.units.remove(name);
                    }
                }
                if full && let Some(instances) = object.get("instances").and_then(Value::as_array) {
                    for value in instances {
                        if let Ok(instance) =
                            serde_json::from_value::<MusterInstance>(value.clone())
                            && !instance.name.is_empty()
                        {
                            self.instances.insert(instance.name.clone(), instance);
                        }
                    }
                }
                if full {
                    self.dir = string(object.get("dir"));
                    self.ready = true;
                }
            }
            Some("events") => {
                if let Some(events) = object.get("records").and_then(Value::as_array) {
                    for value in events {
                        if let Ok(event) = serde_json::from_value::<MusterEvent>(value.clone()) {
                            self.events.push_back(event);
                        }
                    }
                    while self.events.len() > EVENT_CAP {
                        self.events.pop_front();
                    }
                }
            }
            Some(other) => return Err(format!("unknown Muster Channel message type {other:?}")),
            None => return Err("Muster Channel message has no type".into()),
        }
        Ok(())
    }
}

fn yes() -> bool {
    true
}

fn simple() -> String {
    "simple".into()
}

fn string(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn decode_handle(value: &str) -> Result<u64, String> {
    if value.len() != 16
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("handle is not canonical 16-digit lowercase hex".into());
    }
    let handle = u64::from_str_radix(value, 16).map_err(|error| error.to_string())?;
    if handle == 0 {
        Err("handle is zero".into())
    } else {
        Ok(handle)
    }
}

fn handle<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    decode_handle(&value).map_err(serde::de::Error::custom)
}

fn optional_handle<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map(|value| decode_handle(&value).map_err(serde::de::Error::custom))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(name: &str, phase: &str) -> Value {
        serde_json::json!({
            "name": name,
            "instance": "dev",
            "phase": phase,
            "pty": "000000000000002a",
            "type": "simple"
        })
    }

    #[test]
    fn full_and_partial_frames_replace_whole_units() {
        let mut state = MusterState::default();
        state
            .apply(
                serde_json::json!({
                    "type": "state",
                    "full": true,
                    "dir": "/tmp/muster",
                    "instances": [{"name":"dev", "stack":"web", "members":["dev/api"]}],
                    "units": [unit("dev/api", "running")],
                    "gone": []
                })
                .to_string()
                .as_bytes(),
            )
            .unwrap();
        assert!(state.ready);
        assert_eq!(state.units["dev/api"].pty, Some(42));

        state
            .apply(
                serde_json::json!({
                    "type":"state",
                    "units":[unit("dev/api", "failed")],
                    "gone":[]
                })
                .to_string()
                .as_bytes(),
            )
            .unwrap();
        assert_eq!(state.units["dev/api"].phase, "failed");
        assert_eq!(state.instances.len(), 1);

        state
            .apply(br#"{"type":"state","units":[],"gone":["dev/api"]}"#)
            .unwrap();
        assert!(state.units.is_empty());
    }

    #[test]
    fn invalid_handles_do_not_replace_a_valid_unit() {
        let mut state = MusterState::default();
        state
            .apply(
                serde_json::json!({
                    "type":"state",
                    "units":[unit("api", "running")],
                    "gone":[]
                })
                .to_string()
                .as_bytes(),
            )
            .unwrap();
        state
            .apply(br#"{"type":"state","units":[{"name":"api","pty":"42"}],"gone":[]}"#)
            .unwrap();
        assert_eq!(state.units["api"].pty, Some(42));
    }
}
