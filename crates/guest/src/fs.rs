//! Native filesystem roots and bounded query/content delivery.

use alloc::{string::String, vec::Vec};
use core::fmt;

use yas_wire::{
    Class, Decode, Encode, Extensions, Frame, family, fs as wire,
    state::{Cursor, Phase, StateAck, StateEvent, Unwatch, Watch as StateWatch, WatchResult},
};

use crate::{
    receive::{DEFAULT_STATE_WINDOW as SHARED_STATE_WINDOW, Lease as ReceiveLease},
    transfer,
    yas::{Client, Error as ClientError},
};

/// Complete encoded query-record bytes plus one worst-case MESSAGE batch
/// envelope per record. Query Results do not declare their eventual Transfer
/// length, so this window must be reserved exactly before the Request.
pub const DEFAULT_QUERY_WINDOW: u64 =
    wire::MAX_QUERY_BYTES as u64 + (wire::MAX_QUERY_RECORDS as u64 * 12);
pub const DEFAULT_CONTENT_WINDOW: u64 = 16 * 1024 * 1024;
pub const DEFAULT_STATE_WINDOW: u64 = SHARED_STATE_WINDOW;

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    Transfer(transfer::Error),
    FeatureMissing,
    CounterOverflow,
    Protocol(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Wire(error) => write!(formatter, "invalid native FS value: {error}"),
            Self::Transfer(error) => write!(formatter, "native FS Transfer failed: {error}"),
            Self::FeatureMissing => formatter.write_str("native FS operation is unavailable"),
            Self::CounterOverflow => formatter.write_str("native FS state credit overflow"),
            Self::Protocol(detail) => write!(formatter, "native FS protocol error: {detail}"),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Content {
    pub bytes: Vec<u8>,
    pub content_hash: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueryPage {
    pub records: Vec<wire::QueryRecord>,
    pub next_cursor: Vec<u8>,
    pub total_hint: u64,
    pub truncated: bool,
}

pub struct Root {
    handle: u64,
    revision: u64,
    path_model: u8,
    case_behavior: u8,
    canonical_path: Vec<u8>,
    closed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateUpdate {
    pub phase: Phase,
    pub from_revision: u64,
    pub to_revision: u64,
    pub changes: Vec<wire::StateMutation>,
}

/// One bounded native FS root subscription.
pub struct Watch {
    subscription_id: u32,
    cumulative_credit: u64,
    closed: bool,
    receive_lease: ReceiveLease,
}

impl Watch {
    pub fn subscription_id(&self) -> u32 {
        self.subscription_id
    }

    pub fn offer_frame(
        &mut self,
        client: &mut Client,
        frame: &Frame,
    ) -> Result<Option<StateUpdate>, Error> {
        if self.closed
            || frame.header.class != Class::Event
            || frame.header.family != family::FS
            || frame.header.kind != wire::event_kind::STATE
        {
            return Ok(None);
        }
        let event = StateEvent::decode(&frame.payload)?;
        if event.subscription_id != self.subscription_id {
            return Ok(None);
        }
        let changes = event
            .records
            .iter()
            .map(wire::StateMutation::decode_record)
            .collect::<Result<Vec<_>, _>>()?;
        self.cumulative_credit = self
            .cumulative_credit
            .checked_add(frame.payload.len() as u64)
            .ok_or(Error::CounterOverflow)?;
        client.send_typed_event(
            family::FS,
            wire::event_kind::STATE_ACK,
            &StateAck {
                subscription_id: self.subscription_id,
                applied_revision: event.to_revision,
                cumulative_byte_limit: self.cumulative_credit,
            },
            false,
        )?;
        Ok(Some(StateUpdate {
            phase: event.phase,
            from_revision: event.from_revision,
            to_revision: event.to_revision,
            changes,
        }))
    }

    pub fn close(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        client.request(
            family::FS,
            wire::request_kind::UNWATCH,
            Unwatch {
                subscription_id: self.subscription_id,
            }
            .encode()?,
            true,
        )?;
        self.closed = true;
        self.receive_lease.release();
        Ok(())
    }
}

pub struct StagedWrite {
    staging_handle: u64,
    committed: bool,
}

impl StagedWrite {
    pub fn handle(&self) -> u64 {
        self.staging_handle
    }

    pub fn commit(&mut self, client: &mut Client, flags: u16) -> Result<wire::CommitResult, Error> {
        if self.committed {
            return Err(Error::Protocol("FS staged write already committed"));
        }
        let operation_id = operation_id(client)?;
        let result = client.request_typed(
            family::FS,
            wire::request_kind::COMMIT,
            &wire::Commit {
                staging_handle: self.staging_handle,
                operation_id,
                flags,
                extensions: Extensions::default(),
            },
            true,
        )?;
        self.committed = true;
        Ok(result)
    }
}

impl Root {
    pub fn handle(&self) -> u64 {
        self.handle
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn path_model(&self) -> u8 {
        self.path_model
    }

    pub fn case_behavior(&self) -> u8 {
        self.case_behavior
    }

    pub fn canonical_path(&self) -> &[u8] {
        &self.canonical_path
    }

    pub fn watch(
        &self,
        client: &mut Client,
        flags: u16,
        settle_ms: u16,
        inline_max: u32,
        ignore_patterns: String,
        resume: Option<Cursor>,
    ) -> Result<Watch, Error> {
        let mut receive_lease = client.receive_credit_exact(DEFAULT_STATE_WINDOW)?;
        let initial_credit = receive_lease.bytes();
        let result: WatchResult = client.request_typed_with_receive_lease(
            family::FS,
            wire::request_kind::WATCH,
            &wire::Watch {
                root_handle: self.handle,
                flags,
                settle_ms,
                inline_max,
                ignore_patterns,
                state: StateWatch {
                    initial_credit,
                    resume,
                    extensions: Extensions::default(),
                },
            },
            true,
            &mut receive_lease,
        )?;
        Ok(Watch {
            subscription_id: result.subscription_id,
            cumulative_credit: initial_credit,
            closed: false,
            receive_lease,
        })
    }

    pub fn fetch(
        &self,
        client: &mut Client,
        path: wire::Path,
        expected_hash: Option<[u8; 32]>,
        maximum_bytes: u64,
    ) -> Result<Content, Error> {
        let mut receive_lease = client.receive_credit_up_to(maximum_bytes)?;
        let initial_receive_credit = receive_lease.bytes();
        let result: wire::ContentResult = client.request_typed_with_receive_lease(
            family::FS,
            wire::request_kind::FETCH,
            &wire::Fetch {
                root_handle: self.handle,
                path,
                expected_hash,
                initial_receive_credit,
                extensions: Extensions::default(),
            },
            true,
            &mut receive_lease,
        )?;
        let content_hash = result.content.content_hash;
        let bytes = transfer::receive_inline_or_transfer_with_lease(
            client,
            result.content,
            initial_receive_credit,
            receive_lease,
        )?;
        Ok(Content {
            bytes,
            content_hash,
        })
    }

    pub fn read(
        &self,
        client: &mut Client,
        questions: Vec<wire::ReadQuestion>,
    ) -> Result<QueryPage, Error> {
        let mut receive_lease = client.receive_credit_exact(DEFAULT_QUERY_WINDOW)?;
        let initial_receive_credit = receive_lease.bytes();
        let page: wire::QueryPage = client.request_typed_with_receive_lease(
            family::FS,
            wire::request_kind::READ,
            &wire::Read {
                root_handle: self.handle,
                initial_receive_credit,
                questions,
                extensions: Extensions::default(),
            },
            true,
            &mut receive_lease,
        )?;
        collect_page(client, page, initial_receive_credit, receive_lease)
    }

    pub fn index(
        &self,
        client: &mut Client,
        flags: u16,
        max_results: u16,
        cursor: Vec<u8>,
    ) -> Result<QueryPage, Error> {
        let mut receive_lease = client.receive_credit_exact(DEFAULT_QUERY_WINDOW)?;
        let initial_receive_credit = receive_lease.bytes();
        let page: wire::QueryPage = client.request_typed_with_receive_lease(
            family::FS,
            wire::request_kind::INDEX,
            &wire::Index {
                root_handle: self.handle,
                flags,
                max_results,
                cursor,
                initial_receive_credit,
                extensions: Extensions::default(),
            },
            true,
            &mut receive_lease,
        )?;
        collect_page(client, page, initial_receive_credit, receive_lease)
    }

    pub fn stage_write(
        &self,
        client: &mut Client,
        path: wire::Path,
        precondition: wire::Precondition,
        flags: u16,
        mode: u32,
        content: &[u8],
    ) -> Result<StagedWrite, Error> {
        let content_hash = *blake3::hash(content).as_bytes();
        let result: wire::StageWriteResult = client.request_typed(
            family::FS,
            wire::request_kind::STAGE_WRITE,
            &wire::StageWrite {
                root_handle: self.handle,
                path,
                precondition,
                flags,
                mode,
                byte_len: content.len() as u64,
                content_hash,
                initial_receive_credit: (content.len() as u64).max(1),
                extensions: Extensions::default(),
            },
            true,
        )?;
        transfer::send_byte_transfer(client, &result.descriptor, content)?;
        Ok(StagedWrite {
            staging_handle: result.staging_handle,
            committed: false,
        })
    }

    pub fn apply(
        &mut self,
        client: &mut Client,
        flags: u16,
        items: Vec<wire::ApplyItem>,
    ) -> Result<wire::ApplyResult, Error> {
        let operation_id = operation_id(client)?;
        let result: wire::ApplyResult = client.request_typed(
            family::FS,
            wire::request_kind::APPLY,
            &wire::Apply {
                root_handle: self.handle,
                operation_id,
                flags,
                items,
                extensions: Extensions::default(),
            },
            true,
        )?;
        self.revision = result.root_revision;
        Ok(result)
    }

    pub fn close(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        client.request(
            family::FS,
            wire::request_kind::CLOSE,
            wire::Close {
                root_handle: self.handle,
                extensions: Extensions::default(),
            }
            .encode()?,
            true,
        )?;
        self.closed = true;
        Ok(())
    }
}

fn operation_id(client: &Client) -> Result<[u8; 16], Error> {
    let mut operation_id = [0; 16];
    client.random(&mut operation_id)?;
    if operation_id == [0; 16] {
        operation_id[15] = 1;
    }
    Ok(operation_id)
}

fn collect_page(
    client: &mut Client,
    page: wire::QueryPage,
    receive_window: u64,
    mut receive_lease: ReceiveLease,
) -> Result<QueryPage, Error> {
    let records = match page.delivery {
        wire::PageDelivery::Inline(records) => {
            receive_lease.release();
            records
        }
        wire::PageDelivery::Transfer(descriptor) => {
            let messages = transfer::receive_message_transfer_with_lease(
                client,
                &descriptor,
                receive_window,
                wire::MAX_QUERY_RECORDS,
                receive_lease,
            )?;
            let mut batches = messages
                .iter()
                .map(|message| wire::QueryRecordBatch::decode(message))
                .collect::<Result<Vec<_>, _>>()?;
            batches.sort_by_key(|batch| batch.first_record_index);
            let mut expected = 0u32;
            let mut records = Vec::new();
            for batch in batches {
                if batch.first_record_index != expected {
                    return Err(Error::Protocol("noncontiguous FS query batches"));
                }
                expected = expected
                    .checked_add(batch.records.len() as u32)
                    .ok_or(Error::Protocol("FS query record index overflow"))?;
                records.extend(batch.records);
            }
            records
        }
    };
    let records = records
        .iter()
        .map(wire::QueryRecord::from_typed_record)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(QueryPage {
        records,
        next_cursor: page.next_cursor,
        total_hint: page.total_hint,
        truncated: page.flags & yas_wire::schema::fs::PAGE_TRUNCATED as u16 != 0,
    })
}

impl Client {
    pub fn open_fs(&mut self, source: wire::RootSource, flags: u16) -> Result<Root, Error> {
        if !self.supports(family::FS, Class::Request, wire::request_kind::OPEN) {
            return Err(Error::FeatureMissing);
        }
        let opened: wire::OpenResult = self.request_typed(
            family::FS,
            wire::request_kind::OPEN,
            &wire::Open {
                flags,
                source,
                extensions: Extensions::default(),
            },
            true,
        )?;
        Ok(Root {
            handle: opened.root_handle,
            revision: opened.root_revision,
            path_model: opened.path_model,
            case_behavior: opened.case_behavior,
            canonical_path: opened.canonical_path,
            closed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;
    use yas_wire::{
        Extension, FrameCodec, FrameHeader, FrameLimits,
        core::{ResultPrefix, Status},
        transfer::{
            Credit, Delivery, Descriptor, Direction, InlineOrTransfer, Mode, Reset, UploadStage,
        },
    };

    use crate::test_support::bootstrap_client;

    fn root() -> Root {
        Root {
            handle: 1,
            revision: 1,
            path_model: 1,
            case_behavior: 1,
            canonical_path: b"/".to_vec(),
            closed: false,
        }
    }

    fn content_descriptor(receive_credit: u64) -> Descriptor {
        Descriptor {
            transfer_id: 111,
            mode: Mode::Byte,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: receive_credit,
            max_item_bytes: 0,
            max_chunk_bytes: 64 * 1024,
            content_family: family::FS,
            content_kind: yas_wire::schema::fs::FILE_CONTENT_KIND as u16,
            content_version: wire::VERSION,
            extensions: Extensions(vec![Extension {
                tag: yas_wire::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }]),
        }
    }

    fn collector_descriptor(transfer_id: u32, mode: Mode) -> Descriptor {
        Descriptor {
            transfer_id,
            mode,
            direction: Direction::SENDER_TO_RECEIVER,
            receiver_send_credit: 0,
            sender_send_credit: 0,
            max_item_bytes: if mode == Mode::Message {
                1024 * 1024
            } else {
                0
            },
            max_chunk_bytes: 64 * 1024,
            content_family: family::FS,
            content_kind: yas_wire::schema::fs::FILE_CONTENT_KIND as u16,
            content_version: wire::VERSION,
            extensions: Extensions::default(),
        }
    }

    fn staged_write_descriptor(staging_handle: u64, receive_credit: u64) -> Descriptor {
        Descriptor {
            transfer_id: 112,
            mode: Mode::Byte,
            direction: Direction::RECEIVER_TO_SENDER,
            receiver_send_credit: receive_credit,
            sender_send_credit: 0,
            max_item_bytes: 0,
            max_chunk_bytes: 64 * 1024,
            content_family: family::FS,
            content_kind: yas_wire::schema::fs::STAGED_WRITE_CONTENT_KIND as u16,
            content_version: wire::VERSION,
            extensions: Extensions(vec![
                Extension {
                    tag: yas_wire::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                    required: true,
                    value: Vec::new(),
                },
                UploadStage {
                    staging_handle,
                    expires_server_ns: 1,
                }
                .extension()
                .unwrap(),
            ]),
        }
    }

    #[test]
    fn staged_write_sender_failure_resets_allocated_upload_stage() {
        let (mut client, state, _guard) = bootstrap_client();
        let staging_handle = 17;
        let descriptor = staged_write_descriptor(staging_handle, 1);
        let result = wire::StageWriteResult {
            staging_handle,
            descriptor: descriptor.clone(),
            extensions: Extensions::default(),
        };
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.extend([
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::result(family::FS, wire::request_kind::STAGE_WRITE, 3)
                    },
                    payload: ResultPrefix {
                        status: Status::Ok,
                        detail: Extensions::default(),
                        body: result.encode().unwrap(),
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
            codec
                .encode_stream(&Frame {
                    header: FrameHeader::event(family::TRANSFER, yas_wire::transfer::kind::CREDIT),
                    payload: Credit {
                        transfer_id: descriptor.transfer_id,
                        cumulative_limit: 0,
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
        ]);

        assert!(matches!(
            root().stage_write(
                &mut client,
                wire::Path::default(),
                wire::Precondition::Any,
                0,
                0,
                b"ab",
            ),
            Err(Error::Transfer(transfer::Error::NonContiguous))
        ));
        // Request, one initially credited BYTE_DATA fragment, then one RESET.
        // FsRoot must not add a second RESET after the Transfer helper retires
        // the upload authority.
        assert_eq!(state.borrow().sent.len(), 3);
        let (reset_frame, _) = codec
            .decode_stream(state.borrow().sent.last().unwrap())
            .unwrap();
        assert_eq!(reset_frame.header.family, family::TRANSFER);
        assert_eq!(reset_frame.header.kind, yas_wire::transfer::kind::RESET);
        let reset = Reset::decode(&reset_frame.payload).unwrap();
        assert_eq!(reset.transfer_id, descriptor.transfer_id);
        assert_eq!(
            result.stage_discarded_by(&reset).unwrap(),
            descriptor.upload_stage().unwrap()
        );
        assert!(client.receive_credit_exact(1).is_ok());
    }

    #[test]
    fn fetch_rejects_declared_size_over_partial_lease_before_data() {
        const MIB: u64 = 1024 * 1024;
        let (mut client, state, _guard) = bootstrap_client();
        let mut held = client.receive_credit_exact(12 * MIB).unwrap();
        held.commit();
        let request_id = 3;
        let result = wire::ContentResult {
            content: InlineOrTransfer {
                byte_len: 8 * MIB,
                content_hash: [7; 32],
                delivery: Delivery::Transfer(content_descriptor(4 * MIB)),
            },
            extensions: Extensions::default(),
        };
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::result(family::FS, wire::request_kind::FETCH, request_id)
                    },
                    payload: ResultPrefix {
                        status: Status::Ok,
                        detail: Extensions::default(),
                        body: result.encode().unwrap(),
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
        );

        assert!(matches!(
            root().fetch(
                &mut client,
                wire::Path::default(),
                None,
                DEFAULT_CONTENT_WINDOW,
            ),
            Err(Error::Transfer(transfer::Error::LimitExceeded {
                declared,
                maximum,
            })) if declared == 8 * MIB && maximum == 4 * MIB
        ));
        assert_eq!(state.borrow().incoming.len(), 0);
        assert_eq!(state.borrow().sent.len(), 2);
        let (reset, _) = codec
            .decode_stream(state.borrow().sent.last().unwrap())
            .unwrap();
        assert_eq!(reset.header.family, family::TRANSFER);
        assert_eq!(reset.header.kind, yas_wire::transfer::kind::RESET);
        assert_eq!(client.available_receive_credit(), 4 * MIB);
        held.release();
        assert_eq!(client.available_receive_credit(), 16 * MIB);
    }

    #[test]
    fn fetch_small_inline_value_succeeds_with_baseline_credit_held() {
        const MIB: u64 = 1024 * 1024;
        let (mut client, state, _guard) = bootstrap_client();
        let mut held = client.receive_credit_exact(3 * MIB).unwrap();
        held.commit();
        let bytes = b"ok".to_vec();
        let result = wire::ContentResult {
            content: InlineOrTransfer {
                byte_len: bytes.len() as u64,
                content_hash: *blake3::hash(&bytes).as_bytes(),
                delivery: Delivery::Inline(bytes.clone()),
            },
            extensions: Extensions::default(),
        };
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::result(family::FS, wire::request_kind::FETCH, 3)
                    },
                    payload: ResultPrefix {
                        status: Status::Ok,
                        detail: Extensions::default(),
                        body: result.encode().unwrap(),
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
        );

        let content = root()
            .fetch(
                &mut client,
                wire::Path::default(),
                None,
                DEFAULT_CONTENT_WINDOW,
            )
            .unwrap();
        assert_eq!(content.bytes, bytes);
        assert_eq!(client.available_receive_credit(), 13 * MIB);
        held.release();
        assert_eq!(client.available_receive_credit(), 16 * MIB);
    }

    #[test]
    fn live_wrappers_share_and_release_one_client_aggregate_budget() {
        const MIB: u64 = 1024 * 1024;
        let (mut client, state, _guard) = bootstrap_client();

        let mut state_lease = client.receive_credit_exact(MIB).unwrap();
        state_lease.commit();
        let mut watch = Watch {
            subscription_id: 1,
            cumulative_credit: MIB,
            closed: false,
            receive_lease: state_lease,
        };
        let mut channel = crate::channel::accepted_for_test(&mut client, 121, 4 * MIB).unwrap();
        let mut bytes = transfer::ByteReceiver::new(
            &mut client,
            collector_descriptor(122, Mode::Byte),
            None,
            4 * MIB,
        )
        .unwrap();
        let mut messages = transfer::MessageCollector::new(
            &mut client,
            collector_descriptor(123, Mode::Message),
            4 * MIB,
            4,
        )
        .unwrap();
        assert_eq!(client.available_receive_credit(), 3 * MIB);

        let inline = b"small".to_vec();
        let result = wire::ContentResult {
            content: InlineOrTransfer {
                byte_len: inline.len() as u64,
                content_hash: *blake3::hash(&inline).as_bytes(),
                delivery: Delivery::Inline(inline.clone()),
            },
            extensions: Extensions::default(),
        };
        let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::result(family::FS, wire::request_kind::FETCH, 3)
                    },
                    payload: ResultPrefix {
                        status: Status::Ok,
                        detail: Extensions::default(),
                        body: result.encode().unwrap(),
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
        );
        assert_eq!(
            root()
                .fetch(
                    &mut client,
                    wire::Path::default(),
                    None,
                    DEFAULT_CONTENT_WINDOW,
                )
                .unwrap()
                .bytes,
            inline
        );
        assert_eq!(client.available_receive_credit(), 3 * MIB);
        assert!(matches!(
            crate::channel::accepted_for_test(&mut client, 124, 4 * MIB),
            Err(crate::channel::Error::Client(
                ClientError::ReceiveBudgetExhausted { .. }
            ))
        ));

        bytes.cancel(&mut client).unwrap();
        messages.cancel(&mut client).unwrap();
        channel
            .close(&mut client, crate::channel::CloseReason::Normal)
            .unwrap();
        assert_eq!(client.available_receive_credit(), 15 * MIB);

        state.borrow_mut().incoming.push_back(
            codec
                .encode_stream(&Frame {
                    header: FrameHeader {
                        sensitive: true,
                        ..FrameHeader::result(family::FS, wire::request_kind::UNWATCH, 5)
                    },
                    payload: ResultPrefix {
                        status: Status::Ok,
                        detail: Extensions::default(),
                        body: Vec::new(),
                    }
                    .encode()
                    .unwrap(),
                })
                .unwrap(),
        );
        watch.close(&mut client).unwrap();
        assert_eq!(client.available_receive_credit(), 16 * MIB);
    }
}
