//! Native YAS Core command helpers.

use yas_wire::{Encode, core, family};

use crate::yas_native::NativeClient;

pub(crate) async fn cmd_quit(on: Option<&str>, hub: &str) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let request = core::Shutdown {
        operation_id: operation_id(),
        grace_ns: 0,
        reason: "yas quit".to_string(),
    };
    let body = client
        .request(
            family::CORE,
            core::request_kind::SHUTDOWN,
            request
                .encode()
                .map_err(|error| format!("cannot encode Core SHUTDOWN: {error}"))?,
            true,
        )
        .await?;
    if !body.is_empty() {
        return Err("Core SHUTDOWN returned an unexpected response body".into());
    }
    Ok(())
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
    fn operation_ids_are_nonzero() {
        assert_ne!(operation_id(), [0; 16]);
    }
}
