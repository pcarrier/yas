//! Native YAS Selection implementations for the clipboard CLI.

use std::io::{Read, Write};

use yas_wire::{Extensions, family, selection};

use crate::yas_native::{MAX_COLLECTED_TRANSFER_BYTES, NativeClient};

const INITIAL_RECEIVE_CREDIT: u64 = 1024 * 1024;
const MAX_SELECTION_BYTES: u64 = MAX_COLLECTED_TRANSFER_BYTES;

pub(crate) async fn cmd_list(on: Option<&str>, hub: &str) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let record = clipboard_record(&mut client).await?;
    for mime in record.mime_types {
        println!("{mime}");
    }
    Ok(())
}

pub(crate) async fn cmd_get(on: Option<&str>, hub: &str, mime: &str) -> Result<(), String> {
    let mut client = NativeClient::connect(on, hub).await?;
    let bytes = get_with_client(&mut client, mime).await?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(&bytes)
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("cannot write clipboard content: {error}"))
}

async fn get_with_client(client: &mut NativeClient, mime: &str) -> Result<Vec<u8>, String> {
    let record = clipboard_record(client).await?;
    let result: selection::GetResult = client
        .request_typed(
            family::SELECTION,
            selection::request_kind::GET,
            &selection::Get {
                target: selection::GetTarget::Slot {
                    slot: yas_wire::schema::selection::SLOT_CLIPBOARD as u8,
                    revision: record.revision,
                },
                initial_receive_credit: INITIAL_RECEIVE_CREDIT,
                mime: mime.to_owned(),
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    client
        .receive_inline_or_transfer(result.0, MAX_SELECTION_BYTES)
        .await
}

pub(crate) async fn cmd_set(
    on: Option<&str>,
    hub: &str,
    mime: &str,
    primary: bool,
    text: Option<String>,
) -> Result<(), String> {
    let bytes = match text {
        Some(text) => {
            let bytes = text.into_bytes();
            check_collection_limit(bytes.len() as u64)?;
            bytes
        }
        None => read_stdin_bounded()?,
    };
    let slot = if primary {
        yas_wire::schema::selection::SLOT_PRIMARY as u8
    } else {
        yas_wire::schema::selection::SLOT_CLIPBOARD as u8
    };
    let operation_id = operation_id();
    let mut client = NativeClient::connect(on, hub).await?;
    set_with_client(&mut client, slot, mime, bytes, operation_id).await
}

async fn set_with_client(
    client: &mut NativeClient,
    slot: u8,
    mime: &str,
    bytes: Vec<u8>,
    operation_id: [u8; 16],
) -> Result<(), String> {
    if fits_inline(mime, bytes.len())? {
        let _: selection::RevisionResult = client
            .request_typed(
                family::SELECTION,
                selection::request_kind::SET,
                &selection::Set {
                    slot,
                    operation_id,
                    items: vec![selection::InlineItem {
                        mime: mime.to_owned(),
                        data: bytes,
                    }],
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        return Ok(());
    }

    let begin: selection::SetBeginResult = client
        .request_typed(
            family::SELECTION,
            selection::request_kind::SET_BEGIN,
            &selection::SetBegin {
                slot,
                operation_id,
                items: vec![selection::UploadItem {
                    mime: mime.to_owned(),
                    byte_len: bytes.len() as u64,
                    content_hash: *blake3::hash(&bytes).as_bytes(),
                    initial_receive_credit: bytes.len() as u64,
                }],
                extensions: Extensions::default(),
            },
            true,
        )
        .await?;
    let descriptor = begin
        .descriptors
        .first()
        .ok_or_else(|| "YAS Selection SET_BEGIN returned no upload Transfer".to_string())?;
    if begin.descriptors.len() != 1 {
        return Err(format!(
            "YAS Selection SET_BEGIN returned {} Transfers for one item",
            begin.descriptors.len()
        ));
    }
    client.send_byte_transfer(descriptor, &bytes).await?;
    let _: selection::RevisionResult = client
        .request_typed(
            family::SELECTION,
            selection::request_kind::SET_COMMIT,
            &selection::SetCommit {
                staging_handle: begin.staging_handle,
                operation_id,
                extensions: Extensions::default(),
            },
            false,
        )
        .await?;
    Ok(())
}

async fn clipboard_record(client: &mut NativeClient) -> Result<selection::SelectionRecord, String> {
    let records = client
        .snapshot(family::SELECTION)
        .await?
        .ok_or_else(|| "server did not negotiate the YAS Selection family".to_string())?;
    slot_from_records(&records, yas_wire::schema::selection::SLOT_CLIPBOARD as u8)
}

fn slot_from_records(
    records: &[yas_wire::state::Record],
    wanted_slot: u8,
) -> Result<selection::SelectionRecord, String> {
    let mut found = None;
    for record in records {
        let mutation = selection::decode_state_record(record)
            .map_err(|error| format!("invalid YAS Selection state: {error}"))?;
        if let selection::StateMutation::Complete(selection::CompleteEntity::Slot(slot)) = mutation
            && slot.slot == wanted_slot
            && found.replace(slot).is_some()
        {
            return Err("YAS Selection snapshot repeated a slot".to_string());
        }
    }
    found.ok_or_else(|| "YAS Selection snapshot omitted the clipboard slot".to_string())
}

fn fits_inline(mime: &str, data_len: usize) -> Result<bool, String> {
    let encoded = 2usize
        .checked_add(mime.len())
        .and_then(|value| value.checked_add(4))
        .and_then(|value| value.checked_add(data_len))
        .ok_or_else(|| "clipboard item length overflow".to_string())?;
    Ok(encoded <= selection::MAX_INLINE_BYTES)
}

fn read_stdin_bounded() -> Result<Vec<u8>, String> {
    read_bounded(&mut std::io::stdin().lock(), MAX_SELECTION_BYTES)
        .map_err(|error| format!("failed to read stdin: {error}"))
}

fn read_bounded(reader: &mut impl Read, maximum: u64) -> std::io::Result<Vec<u8>> {
    let limit = maximum
        .checked_add(1)
        .ok_or_else(|| std::io::Error::other("clipboard collection limit overflow"))?;
    let mut bytes = Vec::new();
    reader.take(limit).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(std::io::Error::other(format!(
            "clipboard content exceeds the {maximum}-byte CLI limit"
        )));
    }
    Ok(bytes)
}

fn check_collection_limit(length: u64) -> Result<(), String> {
    if length > MAX_SELECTION_BYTES {
        Err(format!(
            "clipboard content is {length} bytes; CLI limit is {MAX_SELECTION_BYTES}"
        ))
    } else {
        Ok(())
    }
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use yas_wire::{
        Class, Decode, Encode, Extension, Frame, FrameCodec, FrameHeader, FrameLimits,
        core::{
            ClientHello, FamilyDescriptor, Operation, ReceiveLimits, ResultPrefix, RuntimeState,
            ServerHello, Status,
        },
        state::RecordKind,
        transfer::{ByteData, Close, Descriptor, Direction, Mode, UploadStage},
    };

    #[test]
    fn inline_boundary_accounts_for_the_typed_item_envelope() {
        let mime = "text/plain";
        let overhead = 2 + mime.len() + 4;
        assert!(fits_inline(mime, selection::MAX_INLINE_BYTES - overhead).unwrap());
        assert!(!fits_inline(mime, selection::MAX_INLINE_BYTES - overhead + 1).unwrap());
    }

    #[test]
    fn clipboard_slot_is_decoded_from_native_selection_state() {
        let clipboard = selection::SelectionRecord {
            slot: yas_wire::schema::selection::SLOT_CLIPBOARD as u8,
            owner_kind: yas_wire::schema::selection::OWNER_SESSION as u8,
            owner_handle: 19,
            revision: 42,
            mime_types: vec!["text/plain".to_string()],
            extensions: Extensions::default(),
        };
        let primary = selection::SelectionRecord {
            slot: yas_wire::schema::selection::SLOT_PRIMARY as u8,
            revision: 43,
            ..clipboard.clone()
        };
        let records = [
            selection::CompleteEntity::Slot(primary)
                .state_record(RecordKind::Add)
                .unwrap(),
            selection::CompleteEntity::Slot(clipboard.clone())
                .state_record(RecordKind::Add)
                .unwrap(),
        ];
        assert_eq!(
            slot_from_records(&records, yas_wire::schema::selection::SLOT_CLIPBOARD as u8).unwrap(),
            clipboard
        );
    }

    #[test]
    fn bounded_reader_rejects_one_byte_over_the_limit() {
        assert_eq!(
            read_bounded(&mut std::io::Cursor::new(b"1234"), 4).unwrap(),
            b"1234"
        );
        assert!(read_bounded(&mut std::io::Cursor::new(b"12345"), 4).is_err());
    }

    #[tokio::test]
    async fn staged_set_uses_native_transfer_then_commit() {
        let bytes = (0..selection::MAX_INLINE_BYTES + 4096)
            .map(|index| (index.wrapping_mul(17) & 0xff) as u8)
            .collect::<Vec<_>>();
        let expected = bytes.clone();
        let operation_id = [0x31; 16];
        let (client_stream, mut server_stream) = tokio::io::duplex(256 * 1024);
        let server = tokio::spawn(async move {
            let codec = fake_handshake(&mut server_stream).await;
            let begin_frame = read_fake_frame(&mut server_stream, &codec).await;
            assert_eq!(begin_frame.header.family, family::SELECTION);
            assert_eq!(begin_frame.header.kind, selection::request_kind::SET_BEGIN);
            assert!(begin_frame.header.sensitive);
            let begin = selection::SetBegin::decode(&begin_frame.payload).unwrap();
            assert_eq!(begin.operation_id, operation_id);
            assert_eq!(begin.items.len(), 1);
            assert_eq!(begin.items[0].byte_len, expected.len() as u64);
            assert_eq!(
                begin.items[0].content_hash,
                *blake3::hash(&expected).as_bytes()
            );

            let staging_handle = 77;
            let mut extensions = vec![Extension {
                tag: yas_wire::schema::transfer::SENSITIVE_CONTENT_EXTENSION as u16,
                required: true,
                value: Vec::new(),
            }];
            extensions.push(
                UploadStage {
                    staging_handle,
                    expires_server_ns: 999,
                }
                .extension()
                .unwrap(),
            );
            let descriptor = Descriptor {
                transfer_id: 2,
                mode: Mode::Byte,
                direction: Direction::RECEIVER_TO_SENDER,
                receiver_send_credit: expected.len() as u64,
                sender_send_credit: 0,
                max_item_bytes: 0,
                max_chunk_bytes: 4096,
                content_family: family::SELECTION,
                content_kind: yas_wire::schema::selection::ITEM_CONTENT_KIND as u16,
                content_version: selection::VERSION,
                extensions: Extensions(extensions),
            };
            send_fake_result(
                &mut server_stream,
                &codec,
                &begin_frame,
                selection::SetBeginResult {
                    staging_handle,
                    descriptors: vec![descriptor],
                    extensions: Extensions::default(),
                }
                .encode()
                .unwrap(),
                true,
            )
            .await;

            let mut uploaded = Vec::new();
            loop {
                let frame = read_fake_frame(&mut server_stream, &codec).await;
                assert_eq!(frame.header.family, family::TRANSFER);
                assert!(frame.header.sensitive);
                match frame.header.kind {
                    yas_wire::transfer::kind::BYTE_DATA => {
                        let chunk = ByteData::decode(&frame.payload).unwrap();
                        assert_eq!(chunk.offset, uploaded.len() as u64);
                        uploaded.extend_from_slice(&chunk.data);
                    }
                    yas_wire::transfer::kind::CLOSE => {
                        let close = Close::decode(&frame.payload).unwrap();
                        assert_eq!(close.final_data_bytes, expected.len() as u64);
                        assert_eq!(close.status, Status::Ok.code());
                        break;
                    }
                    kind => panic!("unexpected Transfer kind {kind:#06x}"),
                }
            }
            assert_eq!(uploaded, expected);

            let commit_frame = read_fake_frame(&mut server_stream, &codec).await;
            assert_eq!(commit_frame.header.family, family::SELECTION);
            assert_eq!(
                commit_frame.header.kind,
                selection::request_kind::SET_COMMIT
            );
            let commit = selection::SetCommit::decode(&commit_frame.payload).unwrap();
            assert_eq!(commit.staging_handle, staging_handle);
            assert_eq!(commit.operation_id, operation_id);
            send_fake_result(
                &mut server_stream,
                &codec,
                &commit_frame,
                selection::RevisionResult { revision: 5 }.encode().unwrap(),
                false,
            )
            .await;
        });

        let mut client = NativeClient::connect_transport(
            crate::transport::Transport::Duplex(client_stream),
            "yas-selection-test",
        )
        .await
        .unwrap();
        set_with_client(
            &mut client,
            yas_wire::schema::selection::SLOT_CLIPBOARD as u8,
            "application/octet-stream",
            bytes,
            operation_id,
        )
        .await
        .unwrap();
        server.await.unwrap();
    }

    async fn fake_handshake(stream: &mut tokio::io::DuplexStream) -> FrameCodec {
        let mut preface = [0; yas_wire::PREFACE.len()];
        stream.read_exact(&mut preface).await.unwrap();
        assert_eq!(preface, yas_wire::PREFACE);
        let pre_hello = FrameCodec::pre_hello();
        let hello_frame = read_fake_frame(stream, &pre_hello).await;
        let client_hello = ClientHello::decode(&hello_frame.payload).unwrap();
        let bidirectional_event = |kind| Operation {
            server_accepts: true,
            server_sends: true,
            class: Class::Event,
            kind,
        };
        let accepts_request = |kind| Operation {
            server_accepts: true,
            server_sends: false,
            class: Class::Request,
            kind,
        };
        let hello = ServerHello {
            minor: 1,
            boot_id: [1; 16],
            session_id: [2; 16],
            receive: ReceiveLimits::recommended(0),
            server_monotonic_ns: 3,
            catalog_revision: 1,
            server_name: "fake".to_string(),
            server_release: "test".to_string(),
            families: vec![
                FamilyDescriptor {
                    family_id: family::CORE,
                    version: yas_wire::core::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: vec![],
                    limits: Extensions::default(),
                },
                FamilyDescriptor {
                    family_id: family::TRANSFER,
                    version: yas_wire::transfer::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: vec![
                        bidirectional_event(yas_wire::transfer::kind::BYTE_DATA),
                        bidirectional_event(yas_wire::transfer::kind::CREDIT),
                        bidirectional_event(yas_wire::transfer::kind::CLOSE),
                        bidirectional_event(yas_wire::transfer::kind::RESET),
                    ],
                    limits: Extensions::default(),
                },
                FamilyDescriptor {
                    family_id: family::SELECTION,
                    version: selection::VERSION,
                    runtime_state: RuntimeState::Available,
                    operations: vec![
                        accepts_request(selection::request_kind::SET_BEGIN),
                        accepts_request(selection::request_kind::SET_COMMIT),
                    ],
                    limits: selection::Limits::HARD.to_extensions().unwrap(),
                },
            ],
            extensions: Extensions::default(),
        };
        let response = Frame {
            header: FrameHeader::result(
                family::CORE,
                yas_wire::core::request_kind::HELLO,
                hello_frame.header.request_id.unwrap(),
            ),
            payload: ResultPrefix {
                status: Status::Ok,
                detail: Extensions::default(),
                body: hello.encode().unwrap(),
            }
            .encode()
            .unwrap(),
        };
        write_fake_frame(stream, &pre_hello, &response).await;
        FrameCodec::new(
            FrameLimits {
                max_wire_frame: client_hello.receive.max_frame,
                max_decoded_frame: client_hello.receive.max_decoded,
            },
            [],
        )
        .unwrap()
    }

    async fn send_fake_result(
        stream: &mut tokio::io::DuplexStream,
        codec: &FrameCodec,
        request: &Frame,
        body: Vec<u8>,
        sensitive: bool,
    ) {
        let mut header = FrameHeader::result(
            request.header.family,
            request.header.kind,
            request.header.request_id.unwrap(),
        );
        header.sensitive = sensitive;
        write_fake_frame(
            stream,
            codec,
            &Frame {
                header,
                payload: ResultPrefix {
                    status: Status::Ok,
                    detail: Extensions::default(),
                    body,
                }
                .encode()
                .unwrap(),
            },
        )
        .await;
    }

    async fn read_fake_frame(stream: &mut tokio::io::DuplexStream, codec: &FrameCodec) -> Frame {
        let mut length = [0; 4];
        stream.read_exact(&mut length).await.unwrap();
        let length = u32::from_le_bytes(length) as usize;
        let mut bytes = vec![0; length + 4];
        bytes[..4].copy_from_slice(&(length as u32).to_le_bytes());
        stream.read_exact(&mut bytes[4..]).await.unwrap();
        let (frame, consumed) = codec.decode_stream(&bytes).unwrap();
        assert_eq!(consumed, bytes.len());
        frame
    }

    async fn write_fake_frame(
        stream: &mut tokio::io::DuplexStream,
        codec: &FrameCodec,
        frame: &Frame,
    ) {
        stream
            .write_all(&codec.encode_stream(frame).unwrap())
            .await
            .unwrap();
    }
}
