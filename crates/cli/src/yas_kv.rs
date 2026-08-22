//! Native YAS KV commands over the typed KV and Transfer families.

use std::collections::BTreeMap;
use std::io::{Read as _, Write as _};

use yas_wire::{
    Decode, Encode, Extensions,
    core::Status,
    family,
    kv::{
        self, Close, Delete, EntryRecord, Get, GetResult, MutationResult, Open, OpenResult,
        Precondition, Put, RemovedEntry, StageValue, StageValueResult, ValueSource, Watch,
    },
    state::{Phase, RecordKind, StateAck, StateEvent, Unwatch, WatchResult},
};

use crate::{cli::KvCommand, yas_native::NativeClient};

const STATE_CREDIT: u64 = yas_wire::schema::transport::RECOMMENDED_BUFFERED;

struct Namespace {
    client: NativeClient,
    handle: u64,
}

pub(crate) async fn dispatch(
    on: Option<&str>,
    hub: &str,
    command: KvCommand,
) -> Result<i32, String> {
    match command {
        KvCommand::Get { key } => cmd_get(on, hub, key).await,
        KvCommand::Put {
            key,
            value,
            if_hash,
            force,
            durable,
            json,
        } => cmd_put(on, hub, key, value, false, if_hash, force, durable, json).await,
        KvCommand::Rm {
            key,
            if_hash,
            force,
            durable,
            json,
        } => cmd_put(on, hub, key, None, true, if_hash, force, durable, json).await,
        KvCommand::Ls {
            prefix,
            watch,
            values,
            json,
        } => cmd_ls(on, hub, prefix, watch, values, json).await,
    }
}

impl Namespace {
    async fn open(on: Option<&str>, hub: &str, prefix: &[u8]) -> Result<Self, String> {
        let mut client = NativeClient::connect(on, hub).await?;
        let result: OpenResult = client
            .request_typed(
                family::KV,
                kv::request_kind::OPEN,
                &Open {
                    prefix: prefix.to_vec(),
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        Ok(Self {
            client,
            handle: result.namespace_handle,
        })
    }

    async fn close(&mut self) -> Result<(), String> {
        self.client
            .request(
                family::KV,
                kv::request_kind::CLOSE,
                Close {
                    namespace_handle: self.handle,
                    extensions: Extensions::default(),
                }
                .encode()
                .map_err(wire_error)?,
                false,
            )
            .await
            .map(|_| ())
    }
}

/// `yas kv get KEY`: write the value without text conversion.
pub async fn cmd_get(on: Option<&str>, hub: &str, key: String) -> Result<i32, String> {
    let mut namespace = Namespace::open(on, hub, b"").await?;
    let request = Get {
        namespace_handle: namespace.handle,
        relative_key: key.into_bytes(),
        initial_receive_credit: kv::MAX_VALUE_BYTES as u64,
        extensions: Extensions::default(),
    };
    let prefix = namespace
        .client
        .request_result(
            family::KV,
            kv::request_kind::GET,
            request.encode().map_err(wire_error)?,
            true,
        )
        .await?;
    if prefix.status == Status::NotFound {
        namespace.close().await?;
        return Ok(1);
    }
    if prefix.status != Status::Ok {
        return Err(format!("KV GET failed with {:?}", prefix.status));
    }
    let result = GetResult::decode(&prefix.body).map_err(wire_error)?;
    let bytes = namespace
        .client
        .receive_inline_or_transfer(result.value, kv::MAX_VALUE_BYTES as u64)
        .await?;
    std::io::stdout()
        .write_all(&bytes)
        .map_err(|error| format!("writing stdout: {error}"))?;
    namespace.close().await?;
    Ok(0)
}

/// `yas kv put` and `yas kv rm` share the mutation path.
#[allow(clippy::too_many_arguments)]
pub async fn cmd_put(
    on: Option<&str>,
    hub: &str,
    key: String,
    value: Option<String>,
    delete: bool,
    if_hash: Option<String>,
    force: bool,
    durable: bool,
    json: bool,
) -> Result<i32, String> {
    let bytes = if delete {
        Vec::new()
    } else if let Some(value) = value {
        value.into_bytes()
    } else {
        let mut bytes = Vec::new();
        std::io::stdin()
            .read_to_end(&mut bytes)
            .map_err(|error| format!("reading stdin: {error}"))?;
        bytes
    };
    if bytes.len() > kv::MAX_VALUE_BYTES {
        return Err(format!(
            "KV value is {} bytes; native YAS limit is {}",
            bytes.len(),
            kv::MAX_VALUE_BYTES
        ));
    }
    let precondition = mutation_precondition(if_hash.as_deref(), force)?;
    let mut namespace = Namespace::open(on, hub, b"").await?;
    let operation_id = rand::random();
    let key_bytes = key.as_bytes().to_vec();
    let (kind, payload) = if delete {
        let request = Delete {
            namespace_handle: namespace.handle,
            operation_id,
            durable,
            relative_key: key_bytes,
            precondition,
            extensions: Extensions::default(),
        };
        (
            kv::request_kind::DELETE,
            request.encode().map_err(wire_error)?,
        )
    } else {
        let source = if bytes.len() <= kv::MAX_INLINE_BYTES {
            ValueSource::Inline(bytes)
        } else {
            let hash = *blake3::hash(&bytes).as_bytes();
            let stage: StageValueResult = namespace
                .client
                .request_typed(
                    family::KV,
                    kv::request_kind::STAGE_VALUE,
                    &StageValue {
                        byte_len: bytes.len() as u64,
                        content_hash: hash,
                        extensions: Extensions::default(),
                    },
                    true,
                )
                .await?;
            if stage.byte_len != bytes.len() as u64 || stage.content_hash != hash {
                return Err("KV stage Result did not match the offered value".into());
            }
            namespace
                .client
                .send_byte_transfer(&stage.transfer, &bytes)
                .await?;
            ValueSource::Staged(stage.staging_handle)
        };
        let request = Put {
            namespace_handle: namespace.handle,
            operation_id,
            durable,
            relative_key: key_bytes,
            precondition,
            value: source,
            extensions: Extensions::default(),
        };
        (kv::request_kind::PUT, request.encode().map_err(wire_error)?)
    };
    let body = namespace
        .client
        .request(family::KV, kind, payload, true)
        .await?;
    let result = MutationResult::decode(&body).map_err(wire_error)?;
    let code = match result.status {
        Status::Ok => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "key": key,
                        "hash": encode_hash(&result.content_hash),
                        "size": result.byte_len,
                        "mtime_ns": result.modified_unix_ns,
                        "revision": result.modification_revision,
                    })
                );
            }
            0
        }
        Status::Conflict => {
            eprintln!(
                "yas: {key} changed under us (current hash {})",
                encode_hash(&result.content_hash)
            );
            1
        }
        status => return Err(format!("{key}: KV mutation failed with {status:?}")),
    };
    namespace.close().await?;
    Ok(code)
}

/// `yas kv ls [PREFIX]`: consume one coherent native State snapshot and,
/// with `--watch`, subsequent deltas.
pub async fn cmd_ls(
    on: Option<&str>,
    hub: &str,
    prefix: String,
    watch: bool,
    values: bool,
    json: bool,
) -> Result<i32, String> {
    let mut namespace = Namespace::open(on, hub, prefix.as_bytes()).await?;
    let result: WatchResult = namespace
        .client
        .request_typed(
            family::KV,
            kv::request_kind::WATCH,
            &Watch {
                namespace_handle: namespace.handle,
                inline_max: if values {
                    kv::MAX_INLINE_BYTES as u32
                } else {
                    0
                },
                state: yas_wire::state::Watch {
                    initial_credit: STATE_CREDIT,
                    resume: None,
                    extensions: Extensions::default(),
                },
            },
            true,
        )
        .await?;
    let mut entries = BTreeMap::<Vec<u8>, EntryRecord>::new();
    let mut snapshot_done = false;
    let mut cumulative_credit = STATE_CREDIT;
    loop {
        let frame = namespace
            .client
            .next_matching_event(family::KV, kv::event_kind::STATE)
            .await?;
        if !frame.header.sensitive {
            return Err("KV STATE event was not marked sensitive".into());
        }
        let event = StateEvent::decode(&frame.payload).map_err(wire_error)?;
        if event.subscription_id != result.subscription_id {
            continue;
        }
        match event.phase {
            Phase::SnapshotBegin => entries.clear(),
            Phase::SnapshotRecords | Phase::SnapshotEnd | Phase::Delta => {
                for record in &event.records {
                    match record.kind {
                        RecordKind::Add | RecordKind::Replace => {
                            let entry = kv::entry_from_state_record(record).map_err(wire_error)?;
                            if snapshot_done {
                                print_entry(&entry, values, json, "upsert");
                            }
                            entries.insert(entry.relative_key.clone(), entry);
                        }
                        RecordKind::Remove => {
                            let removed =
                                RemovedEntry::from_state_record(record).map_err(wire_error)?;
                            entries.remove(&removed.relative_key);
                            if snapshot_done {
                                print_removed(&removed.relative_key, json);
                            }
                        }
                        RecordKind::Patch | RecordKind::Family(_) if record.required => {
                            return Err("KV sent an unsupported required State record".into());
                        }
                        _ => {}
                    }
                }
            }
            Phase::Reset => {
                entries.clear();
                snapshot_done = false;
            }
        }
        cumulative_credit = cumulative_credit.saturating_add(frame.payload.len() as u64);
        namespace
            .client
            .send_typed_event(
                family::KV,
                kv::event_kind::STATE_ACK,
                &StateAck {
                    subscription_id: result.subscription_id,
                    applied_revision: event.to_revision,
                    cumulative_byte_limit: cumulative_credit,
                },
                false,
            )
            .await?;
        if event.phase == Phase::Reset {
            continue;
        }
        if event.phase == Phase::SnapshotEnd && !snapshot_done {
            snapshot_done = true;
            for entry in entries.values() {
                print_entry(entry, values, json, "upsert");
            }
            if !watch {
                namespace
                    .client
                    .request(
                        family::KV,
                        kv::request_kind::UNWATCH,
                        Unwatch {
                            subscription_id: result.subscription_id,
                        }
                        .encode()
                        .map_err(wire_error)?,
                        false,
                    )
                    .await?;
                namespace.close().await?;
                return Ok(0);
            }
        }
    }
}

fn mutation_precondition(if_hash: Option<&str>, force: bool) -> Result<Precondition, String> {
    if force || if_hash.is_none() {
        return Ok(Precondition::Any);
    }
    Ok(Precondition::Hash(parse_hash(
        if_hash.expect("checked above"),
    )?))
}

fn parse_hash(text: &str) -> Result<[u8; 32], String> {
    let text = text.strip_prefix("0x").unwrap_or(text);
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("not a 32-byte BLAKE3 hex hash: {text}"));
    }
    let mut hash = [0; 32];
    for (index, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        hash[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(hash)
}

fn hex_nibble(byte: u8) -> Result<u8, String> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err("invalid hex digit".into()),
    }
}

fn encode_hash(hash: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(64);
    for byte in hash {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0xf) as usize] as char);
    }
    output
}

fn print_entry(entry: &EntryRecord, values: bool, json: bool, kind: &str) {
    let key = String::from_utf8_lossy(&entry.relative_key);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "type": kind,
                "key": key,
                "hash": encode_hash(&entry.content_hash),
                "size": entry.byte_len,
                "mtime_ns": entry.modified_unix_ns,
                "revision": entry.modification_revision,
                "value": entry.inline_value.as_deref().map(String::from_utf8_lossy),
            })
        );
    } else if values {
        let value = entry
            .inline_value
            .as_deref()
            .map(String::from_utf8_lossy)
            .unwrap_or_default();
        println!("{key}\t{}\t{value}", entry.byte_len);
    } else {
        println!("{key}\t{}", entry.byte_len);
    }
}

fn print_removed(relative_key: &[u8], json: bool) {
    let key = String::from_utf8_lossy(relative_key);
    if json {
        println!("{}", serde_json::json!({"type": "delete", "key": key}));
    } else {
        println!("- {key}");
    }
}

fn wire_error(error: impl std::fmt::Display) -> String {
    format!("invalid YAS KV payload: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_hash_parser_is_full_width_and_round_trips() {
        let text = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let hash = parse_hash(text).unwrap();
        assert_eq!(encode_hash(&hash), text);
        assert!(parse_hash("0123456789abcdef0123456789abcdef").is_err());
    }

    #[test]
    fn mutation_precondition_preserves_cas() {
        let hash = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        assert_eq!(
            mutation_precondition(Some(hash), false).unwrap(),
            Precondition::Hash([0xff; 32])
        );
        assert_eq!(
            mutation_precondition(Some(hash), true).unwrap(),
            Precondition::Any
        );
        assert_eq!(
            mutation_precondition(None, false).unwrap(),
            Precondition::Any
        );
    }
}
