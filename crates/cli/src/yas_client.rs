//! Native YAS Client catalogue and disconnect commands.

use yas_wire::{Encode, client, family};

use crate::cli::{ClientCommand, SessionId};
use crate::yas_native::NativeClient;

pub(crate) async fn dispatch(
    on: Option<&str>,
    hub: &str,
    command: Option<ClientCommand>,
) -> Result<(), String> {
    match command.unwrap_or(ClientCommand::List) {
        ClientCommand::List => list(on, hub).await,
        ClientCommand::Disconnect { id, reason } => {
            disconnect(on, hub, id, reason.unwrap_or_default()).await
        }
    }
}

async fn list(on: Option<&str>, hub: &str) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let self_id = client.hello().session_id;
    let server_now = client.hello().server_monotonic_ns;
    let records = client
        .snapshot(family::CLIENT)
        .await?
        .ok_or_else(|| "server did not negotiate the YAS Client family".to_string())?;

    // Name the far end before listing who is talking to it: a server's own
    // OS, architecture and platform flavour is the thing a reader of this list
    // most often wants and cannot otherwise see.
    if let Some(platform) = yas_wire::core::Platform::from_extensions(
        &client.hello().extensions,
        yas_wire::schema::core::SERVER_HELLO_PLATFORM_EXTENSION as u16,
    ) {
        println!(
            "# server {} {} on {platform}",
            client.hello().server_name,
            client.hello().server_release,
        );
    }
    println!("ID\tAGE_S\tOUT_BYTES_S\tIN_BYTES_S\tSUBSCRIPTIONS\tTERMINALS\tSURFACES\tORIGIN");
    for state in records {
        let record = client::client_from_state_record(&state)
            .map_err(|error| format!("invalid Client state record: {error}"))?;
        if record.session_id == self_id {
            continue;
        }
        let subscriptions = record
            .active_subscriptions()
            .map_err(|error| format!("invalid Client subscription snapshot: {error}"))?
            .unwrap_or_default();
        let rates = record
            .bandwidth_rates()
            .map_err(|error| format!("invalid Client bandwidth snapshot: {error}"))?;
        let (out_rate, in_rate) = rates
            .map(|rates| (rates.sent_bytes_per_second, rates.received_bytes_per_second))
            .unwrap_or((0, 0));
        let auxiliary = subscriptions
            .auxiliary
            .iter()
            .map(|value| {
                let family_name = yas_wire::schema::FAMILIES
                    .iter()
                    .find(|candidate| candidate.id == value.family)
                    .map(|candidate| {
                        candidate
                            .name
                            .strip_prefix("yas.")
                            .unwrap_or(candidate.name)
                    })
                    .unwrap_or("unknown");
                format!(
                    "{family_name}:{}@{}",
                    value.resource_handle, value.subscription_id
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let terminals = subscriptions
            .terminals
            .iter()
            .map(|value| {
                if value.rows == 0 {
                    format!("{}:?:v{}", value.terminal_handle, value.view_id)
                } else {
                    format!(
                        "{}:{}x{}:v{}",
                        value.terminal_handle, value.columns, value.rows, value.view_id
                    )
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        let surfaces = subscriptions
            .surfaces
            .iter()
            .map(|value| {
                if value.width == 0 {
                    format!("{}:?:v{}", value.surface_handle, value.view_id)
                } else {
                    format!(
                        "{}:{}x{}@{}/120:v{}",
                        value.surface_handle,
                        value.width,
                        value.height,
                        value.scale_120,
                        value.view_id
                    )
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{}\t{}\t{out_rate}\t{in_rate}\t{auxiliary}\t{terminals}\t{surfaces}\t{}",
            format_session_id(record.session_id),
            server_now.saturating_sub(record.connected_server_ns) / 1_000_000_000,
            format_origin(&record.origin),
        );
    }
    Ok(())
}

async fn disconnect(
    on: Option<&str>,
    hub: &str,
    id: SessionId,
    reason: String,
) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let request = client::Disconnect {
        session_id: id.into_bytes(),
        operation_id: operation_id(),
        reason,
    };
    let body = client
        .request(
            family::CLIENT,
            client::request_kind::DISCONNECT,
            request
                .encode()
                .map_err(|error| format!("cannot encode Client DISCONNECT: {error}"))?,
            true,
        )
        .await?;
    if !body.is_empty() {
        return Err("Client DISCONNECT returned an unexpected response body".into());
    }
    Ok(())
}

fn format_session_id(value: [u8; 16]) -> String {
    let mut output = String::with_capacity(32);
    for byte in value {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("writing to String");
    }
    output
}

fn format_origin(origin: &client::Origin) -> String {
    match origin {
        client::Origin::Unix {
            peer_pid,
            peer_uid,
            socket_path,
            ..
        } => format!(
            "unix:uid={peer_uid}:pid={peer_pid}:{}",
            escape_field(&String::from_utf8_lossy(socket_path))
        ),
        client::Origin::Ssh {
            remote_address,
            username,
        } => format!(
            "ssh:{}@{}",
            escape_field(username),
            escape_field(remote_address)
        ),
        client::Origin::Edge { subject, issuer } => {
            format!("edge:{}@{}", escape_field(subject), escape_field(issuer))
        }
        client::Origin::Relay {
            route_handle,
            generation,
            depth,
        } => format!("relay:{route_handle}:{generation}:depth={depth}"),
        client::Origin::WebRtc { peer_id } => format!("webrtc:{}", escape_field(peer_id)),
        client::Origin::Extension {
            extension_id, name, ..
        } => {
            if name.is_empty() {
                format!("ext:id:{extension_id:016x}")
            } else {
                format!("ext:{}", escape_field(name))
            }
        }
        client::Origin::UnknownOptional { kind, .. } => format!("kind:{kind}"),
    }
}

fn escape_field(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            '\n' => output.push_str("\\n"),
            '\\' => output.push_str("\\\\"),
            other => output.push(other),
        }
    }
    output
}

fn operation_id() -> [u8; 16] {
    let mut value: [u8; 16] = rand::random();
    if value == [0; 16] {
        value[15] = 1;
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_full_width_lowercase_hex() {
        assert_eq!(
            format_session_id([0x0a; 16]),
            "0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a"
        );
    }

    #[test]
    fn tsv_fields_escape_controls() {
        assert_eq!(escape_field("a\tb\nc\\d"), "a\\tb\\nc\\\\d");
    }
}
