//! Native, sensitive environment snapshot access.

use alloc::vec::Vec;
use core::fmt;

use yas_wire::{Class, Decode, Extensions, Frame, core::Status, env as wire, family};

use crate::{
    receive::Lease as ReceiveLease,
    transfer::{self, MessageCollector},
    yas::{Client, Error as ClientError, RequestToken},
};

/// Includes raw entry length prefixes and worst-case one batch header per
/// entry in addition to the protocol's raw key/value data bound.
pub const DEFAULT_SNAPSHOT_WINDOW: u64 =
    wire::MAX_TOTAL_DATA_BYTES as u64 + (wire::MAX_ENTRIES as u64 * 14);

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    Transfer(transfer::Error),
    FeatureMissing,
}

enum RequestState {
    Result {
        token: RequestToken,
        receive_lease: Option<ReceiveLease>,
    },
    Transfer {
        collector: MessageCollector,
        entry_count: u32,
        total_data_bytes: u64,
    },
    Done,
}

/// A multiplexable native Env snapshot request.
pub struct PendingEnvironment {
    state: RequestState,
}

impl PendingEnvironment {
    pub fn owns_frame(&self, frame: &Frame) -> bool {
        match &self.state {
            RequestState::Result { token, .. } => token.matches(frame),
            RequestState::Transfer { collector, .. } => collector.owns_frame(frame),
            RequestState::Done => false,
        }
    }

    pub fn offer_frame(
        &mut self,
        client: &mut Client,
        frame: &Frame,
    ) -> Result<Option<Vec<wire::Entry>>, Error> {
        match &mut self.state {
            RequestState::Result {
                token,
                receive_lease,
            } => {
                let prefix = match client.offer_result(token, frame) {
                    Ok(Some(prefix)) => prefix,
                    Ok(None) => return Ok(None),
                    Err(error) => {
                        self.state = RequestState::Done;
                        return Err(error.into());
                    }
                };
                if prefix.status != Status::Ok {
                    if let Some(mut lease) = receive_lease.take() {
                        lease.release();
                    }
                    let family = token.family();
                    let kind = token.kind();
                    self.state = RequestState::Done;
                    return Err(ClientError::RequestFailed {
                        family,
                        kind,
                        status: prefix.status,
                        detail: prefix.detail,
                    }
                    .into());
                }
                let result = match client.decode_result_body::<wire::GetResult>(&prefix.body) {
                    Ok(result) => result,
                    Err(error) => {
                        self.state = RequestState::Done;
                        return Err(error.into());
                    }
                };
                match result.delivery {
                    wire::Delivery::Inline(entries) => {
                        if let Some(mut lease) = receive_lease.take() {
                            lease.release();
                        }
                        self.state = RequestState::Done;
                        Ok(Some(entries))
                    }
                    wire::Delivery::Transfer(descriptor) => {
                        let lease = receive_lease
                            .take()
                            .ok_or(ClientError::Protocol("Env receive lease disappeared"))?;
                        let receive_window = lease.bytes();
                        let required = result
                            .total_data_bytes
                            .checked_add(u64::from(result.entry_count).saturating_mul(14))
                            .ok_or(ClientError::Protocol("Env receive size overflow"))?;
                        if required > receive_window {
                            self.state = RequestState::Done;
                            transfer::reject_receive_transfer_with_lease(
                                client,
                                &descriptor,
                                lease,
                            )?;
                            return Err(transfer::Error::LimitExceeded {
                                declared: required,
                                maximum: receive_window,
                            }
                            .into());
                        }
                        // The correlated Result has been consumed and the
                        // lease moves into the collector. Do not leave a
                        // resumable Result state if collector validation or
                        // its best-effort RESET fails.
                        self.state = RequestState::Done;
                        let collector = MessageCollector::new_with_lease(
                            client,
                            descriptor,
                            receive_window,
                            wire::MAX_ENTRIES,
                            lease,
                        )?;
                        self.state = RequestState::Transfer {
                            collector,
                            entry_count: result.entry_count,
                            total_data_bytes: result.total_data_bytes,
                        };
                        Ok(None)
                    }
                }
            }
            RequestState::Transfer {
                collector,
                entry_count,
                total_data_bytes,
            } => {
                let Some(messages) = collector.offer_frame(frame)? else {
                    return Ok(None);
                };
                let entry_count = *entry_count;
                let total_data_bytes = *total_data_bytes;
                // A complete collector has retired its receive authority.
                // Terminalize before validating the assembled value so an
                // invalid snapshot cannot leave a spent collector live.
                self.state = RequestState::Done;
                let entries = assemble_snapshot(messages, entry_count, total_data_bytes)?;
                Ok(Some(entries))
            }
            RequestState::Done => Ok(None),
        }
    }

    pub fn cancel(&mut self, client: &mut Client) -> Result<bool, Error> {
        match &mut self.state {
            RequestState::Result {
                token,
                receive_lease,
            } => {
                let prefix = match client.cancel_request_and_wait(token) {
                    Ok(prefix) => prefix,
                    Err(error) => {
                        if !client.request_is_resumable(token) {
                            self.state = RequestState::Done;
                        }
                        return Err(error.into());
                    }
                };
                let result = if prefix.status == Status::Ok {
                    match client.decode_result_body::<wire::GetResult>(&prefix.body) {
                        Ok(result) => Some(result),
                        Err(error) => {
                            self.state = RequestState::Done;
                            return Err(error.into());
                        }
                    }
                } else {
                    None
                };
                let mut lease = receive_lease
                    .take()
                    .ok_or(ClientError::Protocol("Env receive lease disappeared"))?;
                let settled = if let Some(result) = result {
                    match result.delivery {
                        wire::Delivery::Inline(_) => {
                            lease.release();
                            Ok(())
                        }
                        wire::Delivery::Transfer(descriptor) => {
                            transfer::reject_receive_transfer_with_lease(client, &descriptor, lease)
                                .map_err(Error::from)
                        }
                    }
                } else {
                    lease.release();
                    Ok(())
                };
                self.state = RequestState::Done;
                settled?;
                Ok(true)
            }
            RequestState::Transfer { collector, .. } => {
                let cancelled = collector.cancel(client)?;
                self.state = RequestState::Done;
                Ok(cancelled)
            }
            RequestState::Done => Ok(false),
        }
    }
}

fn assemble_snapshot(
    messages: Vec<Vec<u8>>,
    entry_count: u32,
    total_data_bytes: u64,
) -> Result<Vec<wire::Entry>, Error> {
    let mut batches = messages
        .iter()
        .map(|message| wire::SnapshotBatch::decode(message))
        .collect::<Result<Vec<_>, _>>()?;
    batches.sort_by_key(|batch| batch.first_index);
    let mut assembler = wire::SnapshotAssembler::new(entry_count, total_data_bytes)?;
    for batch in batches {
        assembler.push(batch)?;
    }
    assembler.finish().map_err(Into::into)
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Wire(error) => write!(formatter, "invalid native Env value: {error}"),
            Self::Transfer(error) => write!(formatter, "native Env Transfer failed: {error}"),
            Self::FeatureMissing => formatter.write_str("native Env GET is unavailable"),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl std::error::Error for Error {}

impl From<ClientError> for Error {
    fn from(value: ClientError) -> Self {
        Self::Client(value)
    }
}

impl From<yas_wire::Error> for Error {
    fn from(value: yas_wire::Error) -> Self {
        Self::Wire(value)
    }
}

impl From<transfer::Error> for Error {
    fn from(value: transfer::Error) -> Self {
        Self::Transfer(value)
    }
}

impl Client {
    pub fn begin_environment(&mut self) -> Result<PendingEnvironment, Error> {
        if !self.supports(family::ENV, Class::Request, wire::request_kind::GET) {
            return Err(Error::FeatureMissing);
        }
        let mut receive_lease = self.receive_credit_up_to(DEFAULT_SNAPSHOT_WINDOW)?;
        let initial_receive_credit = receive_lease.bytes();
        let token = self.begin_typed_request(
            family::ENV,
            wire::request_kind::GET,
            &wire::Get {
                initial_receive_credit,
                extensions: Extensions::default(),
            },
            true,
        )?;
        receive_lease.commit();
        Ok(PendingEnvironment {
            state: RequestState::Result {
                token,
                receive_lease: Some(receive_lease),
            },
        })
    }

    /// Fetch the complete boot-scoped process environment. Keys and values stay
    /// raw bytes, and both inline and transferred results remain sensitive.
    pub fn get_environment(&mut self) -> Result<Vec<wire::Entry>, Error> {
        let mut request = self.begin_environment()?;
        loop {
            let frame = match self.next_matching_frame(|frame| request.owns_frame(frame)) {
                Ok(frame) => frame,
                Err(error) => {
                    if request.cancel(self).is_err() {
                        self.poison();
                    }
                    return Err(error.into());
                }
            };
            match request.offer_frame(self, &frame) {
                Ok(Some(entries)) => return Ok(entries),
                Ok(None) => {}
                Err(error) => {
                    if request.cancel(self).is_err() {
                        self.poison();
                    }
                    return Err(error);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use yas_wire::{
        Encode, Extension, FrameHeader,
        core::ResultPrefix,
        transfer::{Close, Descriptor, Direction, Mode},
    };

    use crate::test_support::{bootstrap_client, pending_headroom_burst};

    const WINDOW: u64 = 4 * 1024 * 1024;

    fn pending(client: &mut Client) -> (PendingEnvironment, RequestToken) {
        let mut lease = client.receive_credit_exact(WINDOW).unwrap();
        let token = client
            .begin_typed_request(
                family::ENV,
                wire::request_kind::GET,
                &wire::Get {
                    initial_receive_credit: WINDOW,
                    extensions: Extensions::default(),
                },
                true,
            )
            .unwrap();
        lease.commit();
        (
            PendingEnvironment {
                state: RequestState::Result {
                    token,
                    receive_lease: Some(lease),
                },
            },
            token,
        )
    }

    fn result(token: RequestToken, status: Status, body: Vec<u8>) -> Frame {
        Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::result(token.family(), token.kind(), token.request_id())
            },
            payload: ResultPrefix {
                status,
                detail: Extensions::default(),
                body,
            }
            .encode()
            .unwrap(),
        }
    }

    fn transfer_descriptor() -> Descriptor {
        Descriptor {
            transfer_id: 91,
            mode: Mode::Message,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: WINDOW,
            max_item_bytes: wire::MAX_BATCH_BYTES as u64,
            max_chunk_bytes: 64 * 1024,
            content_family: family::ENV,
            content_kind: wire::SNAPSHOT_CONTENT_KIND,
            content_version: wire::VERSION,
            extensions: Extensions(vec![Extension {
                tag: yas_wire::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        }
    }

    fn transfer_close(final_data_bytes: u64) -> Frame {
        Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CLOSE)
            },
            payload: Close {
                transfer_id: 91,
                final_data_bytes,
                status: Status::Ok.code(),
                detail: Vec::new(),
            }
            .encode()
            .unwrap(),
        }
    }

    #[test]
    fn rejected_transfer_result_terminalizes_consumed_request() {
        let (mut client, _state, _guard) = bootstrap_client();
        let (mut request, token) = pending(&mut client);
        let body =
            wire::GetResult::transfer(1, WINDOW, transfer_descriptor(), Extensions::default())
                .unwrap()
                .encode()
                .unwrap();

        assert!(matches!(
            request.offer_frame(&mut client, &result(token, Status::Ok, body)),
            Err(Error::Transfer(transfer::Error::LimitExceeded { .. }))
        ));
        assert!(!request.owns_frame(&result(token, Status::Ok, Vec::new())));
        assert!(!request.cancel(&mut client).unwrap());
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);

        let (mut request, token) = pending(&mut client);
        let mut descriptor = transfer_descriptor();
        descriptor.sender_send_credit = WINDOW + 1;
        let body = wire::GetResult::transfer(1, 1, descriptor, Extensions::default())
            .unwrap()
            .encode()
            .unwrap();
        assert!(matches!(
            request.offer_frame(&mut client, &result(token, Status::Ok, body)),
            Err(Error::Transfer(transfer::Error::LimitExceeded { .. }))
        ));
        assert!(!request.owns_frame(&result(token, Status::Ok, Vec::new())));
        assert!(!request.cancel(&mut client).unwrap());
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn invalid_assembled_transfer_terminalizes_after_authority_retirement() {
        let (mut client, _state, _guard) = bootstrap_client();
        let lease = client.receive_credit_exact(WINDOW).unwrap();
        let collector = MessageCollector::new_with_lease(
            &mut client,
            transfer_descriptor(),
            WINDOW,
            wire::MAX_ENTRIES,
            lease,
        )
        .unwrap();
        let mut pending = PendingEnvironment {
            state: RequestState::Transfer {
                collector,
                entry_count: 1,
                total_data_bytes: 1,
            },
        };

        assert!(matches!(
            pending.offer_frame(&mut client, &transfer_close(0)),
            Err(Error::Wire(_))
        ));
        assert!(!pending.owns_frame(&transfer_close(0)));
        assert!(!pending.cancel(&mut client).unwrap());
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn cancel_preserves_live_result_on_preflight_block_then_settles_pending_non_ok() {
        let (mut client, state, _guard) = bootstrap_client();
        let (mut pending, token) = pending(&mut client);
        client
            .defer_for_test(Frame {
                header: FrameHeader::event(family::EXTENSION, 0x7fff),
                payload: vec![
                    0;
                    yas_wire::schema::transport::RECOMMENDED_BUFFERED as usize
                        - yas_wire::schema::transport::RECOMMENDED_DECODED_FRAME as usize
                        + 1
                ],
            })
            .unwrap();
        let sent_before = state.borrow().sent.len();

        assert!(matches!(
            pending.cancel(&mut client),
            Err(Error::Client(ClientError::PendingReadBlocked))
        ));
        assert_eq!(state.borrow().sent.len(), sent_before);
        assert!(client.request_is_resumable(&token));
        assert!(pending.owns_frame(&result(token, Status::ResourceExhausted, Vec::new())));
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);

        client
            .defer_for_test(result(token, Status::ResourceExhausted, Vec::new()))
            .unwrap();
        assert!(pending.cancel(&mut client).unwrap());
        assert_eq!(state.borrow().sent.len(), sent_before);
        assert!(!pending.owns_frame(&result(token, Status::Ok, Vec::new())));
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn cancel_settles_pending_inline_or_transfer_without_orphaning_credit() {
        let (mut client, state, _guard) = bootstrap_client();
        let (mut inline, inline_token) = pending(&mut client);
        let inline_body = wire::GetResult::inline(
            vec![wire::Entry {
                key: b"A".to_vec(),
                value: b"B".to_vec(),
            }],
            Extensions::default(),
        )
        .unwrap()
        .encode()
        .unwrap();
        client
            .defer_for_test(result(inline_token, Status::Ok, inline_body))
            .unwrap();
        let sent_before = state.borrow().sent.len();
        assert!(inline.cancel(&mut client).unwrap());
        assert_eq!(state.borrow().sent.len(), sent_before);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);

        let (mut transferred, transfer_token) = pending(&mut client);
        let transfer_body =
            wire::GetResult::transfer(1, 2, transfer_descriptor(), Extensions::default())
                .unwrap()
                .encode()
                .unwrap();
        client
            .defer_for_test(result(transfer_token, Status::Ok, transfer_body))
            .unwrap();
        let sent_before = state.borrow().sent.len();
        assert!(transferred.cancel(&mut client).unwrap());
        assert_eq!(state.borrow().sent.len(), sent_before + 1);
        assert_eq!(client.available_receive_credit(), 16 * 1024 * 1024);
    }

    #[test]
    fn cancel_malformed_pending_result_terminalizes_and_poisons() {
        let (mut client, _state, _guard) = bootstrap_client();
        let (mut pending, token) = pending(&mut client);
        client
            .defer_for_test(result(token, Status::Ok, vec![0]))
            .unwrap();

        assert!(matches!(
            pending.cancel(&mut client),
            Err(Error::Client(ClientError::Wire(_)))
        ));
        assert!(!pending.owns_frame(&result(token, Status::Ok, Vec::new())));
        assert_eq!(client.available_receive_credit(), 12 * 1024 * 1024);
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
        assert!(matches!(
            client.receive_credit_exact(1),
            Err(ClientError::Poisoned)
        ));
    }

    #[test]
    fn blocking_environment_owner_poisons_and_pins_when_cancel_cannot_preflight() {
        let (mut client, state, _guard) = bootstrap_client();
        state
            .borrow_mut()
            .responses_after_send
            .push_back((1, pending_headroom_burst()));

        assert!(matches!(
            client.get_environment(),
            Err(Error::Client(ClientError::PendingReadBlocked))
        ));
        assert_eq!(state.borrow().sent.len(), 1);
        assert_eq!(
            client.available_receive_credit(),
            16 * 1024 * 1024 - DEFAULT_SNAPSHOT_WINDOW
        );
        assert!(matches!(client.next_event(), Err(ClientError::Poisoned)));
    }
}
