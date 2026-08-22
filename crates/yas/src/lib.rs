#![no_std]

//! Native YAS v1 wire protocol.
//!
//! This crate implements the transport-independent bytes described by
//! `docs/design/yas.md`. It does not select a transport, authenticate a peer,
//! allocate handles, or apply family policy. Runtime code is `no_std` and uses
//! only `alloc`, so the same codecs are usable by native servers and guests.

#[macro_use]
extern crate alloc;
#[cfg(test)]
extern crate std;

mod prelude {
    pub(crate) use alloc::borrow::ToOwned;
    pub(crate) use alloc::boxed::Box;
    pub(crate) use alloc::collections::{BTreeMap, BTreeSet};
    pub(crate) use alloc::string::String;
    pub(crate) use alloc::vec::Vec;
}

pub mod channel;
pub mod client;
pub mod codec;
pub mod core;
pub mod desktop;
pub mod env;
pub mod events;
pub mod extension;
pub mod font;
pub mod frame;
pub mod fs;
pub mod git;
pub mod kv;
pub mod lsp;
pub mod media;
pub mod net;
pub mod packed;
pub mod process;
pub mod relay;
pub mod selection;
pub mod state;
pub mod surface;
pub mod terminal;
pub mod transfer;

/// Registry, kind, status, layout metadata, and vectors generated from the
/// canonical TOML files under `protocol/yas`.
pub mod schema {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../protocol/yas/generated.rs"
    ));

    /// Look up the canonical metadata for one exact family version.
    pub fn family_metadata(family_id: u16, version: u16) -> Option<&'static FamilyMetadata> {
        FAMILIES
            .iter()
            .find(|family| family.id == family_id && family.version == version)
    }

    /// Look up one canonical Request or Event operation. Result flag policy is
    /// derived conservatively from its matching Request by `FrameCodec` and is
    /// therefore not duplicated in generated operation metadata.
    pub fn operation(family_id: u16, class: u8, kind: u16) -> Option<&'static OperationMetadata> {
        FAMILIES
            .iter()
            .find(|family| family.id == family_id)?
            .operations
            .iter()
            .find(|operation| operation.class == class && operation.kind == kind)
    }
}

pub use codec::{Decode, Encode, Error, Extension, Extensions, Result};
pub use frame::{Class, Frame, FrameCodec, FrameHeader, FrameLimits, PREFACE};
pub use schema::family;

/// YAS protocol major selected by [`PREFACE`].
pub const PROTOCOL_MAJOR: u16 = schema::transport::PROTOCOL_MAJOR;

#[cfg(test)]
mod generated_artifact_tests {
    use alloc::vec::Vec;

    use super::*;

    fn vector(name: &str) -> Vec<u8> {
        let value = schema::GOLDEN_VECTORS
            .iter()
            .find(|vector| vector.name == name)
            .unwrap()
            .hex
            .as_bytes();
        value
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| {
                let digit = |byte: u8| match byte {
                    b'0'..=b'9' => byte - b'0',
                    b'a'..=b'f' => byte - b'a' + 10,
                    _ => panic!("invalid generated hex"),
                };
                digit(pair[0]) << 4 | digit(pair[1])
            })
            .collect()
    }

    fn truncations(bytes: &[u8], decode: impl Fn(&[u8]) -> Result<()>) {
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err(), "accepted prefix {end}");
        }
        decode(bytes).unwrap();
    }

    #[test]
    fn checked_artifacts_match_build_output() {
        assert_eq!(
            include_str!(concat!(env!("OUT_DIR"), "/yas_schema.json")),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../protocol/yas/schema.json"
            ))
        );
        assert_eq!(
            include_str!(concat!(env!("OUT_DIR"), "/yas_vectors.json")),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../protocol/yas/vectors.json"
            ))
        );
        assert_eq!(
            include_str!(concat!(env!("OUT_DIR"), "/yas_schema.rs")),
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../protocol/yas/generated.rs"
            ))
        );
        assert_eq!(PREFACE, [0x59, 0x41, 0x53, 0, 1, 0, 0x0d, 0x0a]);
        assert_eq!(schema::core::request::HELLO, 0);
        assert_eq!(schema::transfer::event::RESET, 4);
        assert_eq!(schema::relay::request::CONNECT, 2);
        assert_eq!(schema::font::request::FETCH, 3);
    }

    #[test]
    fn generated_frame_header_codec_round_trips_and_rejects_boundaries() {
        let headers = [
            schema::GeneratedFrameHeader {
                family: family::CORE,
                kind: core::event_kind::GOAWAY,
                class: schema::transport::class::EVENT,
                request_id: None,
                compressed: false,
                sensitive: true,
            },
            schema::GeneratedFrameHeader {
                family: family::CORE,
                kind: core::request_kind::PING,
                class: schema::transport::class::REQUEST,
                request_id: Some(7),
                compressed: true,
                sensitive: false,
            },
        ];
        for header in headers {
            let (encoded, len) = header.encode().unwrap();
            for end in 0..len {
                assert_eq!(
                    schema::GeneratedFrameHeader::decode(&encoded[..end]),
                    Err(schema::GeneratedHeaderError::Truncated)
                );
            }
            assert_eq!(
                schema::GeneratedFrameHeader::decode(&encoded[..len]),
                Ok((header, len))
            );
        }

        let invalid = schema::GeneratedFrameHeader {
            family: 0,
            kind: 0,
            class: schema::transport::class::REQUEST,
            request_id: Some(0),
            compressed: false,
            sensitive: false,
        };
        assert_eq!(
            invalid.encode(),
            Err(schema::GeneratedHeaderError::ZeroRequestId)
        );
    }

    #[test]
    fn generated_full_payload_vectors_decode_and_reject_truncation() {
        use crate::{
            channel, client, core, desktop, env, events, extension, font, fs, git, kv, lsp, media,
            net, process, relay, selection, state, surface, terminal, transfer,
        };

        type VectorCase = (&'static str, bool, fn(&[u8]) -> Result<()>);
        let cases: &[VectorCase] = &[
            ("core.client_hello.payload", true, |bytes| {
                core::ClientHello::decode(bytes).map(|_| ())
            }),
            ("core.result.ok_empty.payload", true, |bytes| {
                core::ResultPrefix::decode(bytes).map(|_| ())
            }),
            ("core.negotiated_codecs.payload", true, |bytes| {
                core::NegotiatedCodecs::decode(bytes).map(|_| ())
            }),
            ("core.server_hello.payload", true, |bytes| {
                core::ServerHello::decode(bytes).map(|_| ())
            }),
            ("core.ping.payload", true, |bytes| {
                core::Ping::decode(bytes).map(|_| ())
            }),
            ("core.ping_result.payload", true, |bytes| {
                core::PingResult::decode(bytes).map(|_| ())
            }),
            ("core.cancel.payload", true, |bytes| {
                core::Cancel::decode(bytes).map(|_| ())
            }),
            ("core.shutdown.payload", true, |bytes| {
                core::Shutdown::decode(bytes).map(|_| ())
            }),
            ("core.goaway.payload", true, |bytes| {
                core::GoAway::decode(bytes).map(|_| ())
            }),
            ("core.session_update.payload", true, |bytes| {
                core::SessionUpdate::decode(bytes).map(|_| ())
            }),
            ("core.family_update.payload", true, |bytes| {
                core::FamilyUpdate::decode(bytes).map(|_| ())
            }),
            ("core.session_info.payload", true, |bytes| {
                core::SessionInfo::decode(bytes).map(|_| ())
            }),
            ("transport.ping.frame", true, |bytes| {
                let codec = FrameCodec::new(FrameLimits::recommended(), [])?;
                let frame = codec.decode(bytes)?;
                if frame.header.family != family::CORE
                    || frame.header.kind != core::request_kind::PING
                    || frame.header.class != Class::Request
                    || frame.header.request_id != Some(7)
                {
                    return Err(Error::Invalid("PING frame vector header"));
                }
                core::Ping::decode(&frame.payload).map(|_| ())
            }),
            ("transport.ping.stream", true, |bytes| {
                let codec = FrameCodec::new(FrameLimits::recommended(), [])?;
                let (frame, consumed) = codec.decode_stream(bytes)?;
                if consumed != bytes.len()
                    || frame.header.family != family::CORE
                    || frame.header.kind != core::request_kind::PING
                {
                    return Err(Error::Invalid("PING stream vector"));
                }
                core::Ping::decode(&frame.payload).map(|_| ())
            }),
            ("transport.shutdown.frame", true, |bytes| {
                let codec = FrameCodec::new(FrameLimits::recommended(), [])?;
                let frame = codec.decode(bytes)?;
                if frame.header.family != family::CORE
                    || frame.header.kind != core::request_kind::SHUTDOWN
                    || frame.header.class != Class::Request
                    || frame.header.request_id != Some(8)
                    || !frame.header.sensitive
                {
                    return Err(Error::Invalid("SHUTDOWN frame vector header"));
                }
                core::Shutdown::decode(&frame.payload).map(|_| ())
            }),
            ("transport.goaway.frame", true, |bytes| {
                let codec = FrameCodec::new(FrameLimits::recommended(), [])?;
                let frame = codec.decode(bytes)?;
                if frame.header.family != family::CORE
                    || frame.header.kind != core::event_kind::GOAWAY
                    || frame.header.class != Class::Event
                {
                    return Err(Error::Invalid("GOAWAY frame vector header"));
                }
                core::GoAway::decode(&frame.payload).map(|_| ())
            }),
            ("transfer.descriptor.payload", true, |bytes| {
                transfer::Descriptor::decode(bytes).map(|_| ())
            }),
            // BYTE_DATA consumes its remaining bytes as data, so a shorter
            // prefix after its fixed fields is another valid chunk rather
            // than a truncated encoding.
            ("transfer.byte_data.payload", false, |bytes| {
                transfer::ByteData::decode(bytes).map(|_| ())
            }),
            // MESSAGE_DATA also consumes its remaining bytes as a fragment.
            ("transfer.message_data.payload", false, |bytes| {
                transfer::MessageData::decode(bytes).map(|_| ())
            }),
            ("transfer.credit.payload", true, |bytes| {
                transfer::Credit::decode(bytes).map(|_| ())
            }),
            ("transfer.close.payload", true, |bytes| {
                transfer::Close::decode(bytes).map(|_| ())
            }),
            ("transfer.reset.payload", true, |bytes| {
                transfer::Reset::decode(bytes).map(|_| ())
            }),
            ("state.watch.payload", true, |bytes| {
                state::Watch::decode(bytes).map(|_| ())
            }),
            ("state.unwatch.payload", true, |bytes| {
                state::Unwatch::decode(bytes).map(|_| ())
            }),
            ("state.ack.payload", true, |bytes| {
                state::StateAck::decode(bytes).map(|_| ())
            }),
            ("state.delta_remove.payload", true, |bytes| {
                state::StateEvent::decode(bytes).map(|_| ())
            }),
            ("relay.connect.payload", true, |bytes| {
                relay::Connect::decode(bytes).map(|_| ())
            }),
            ("font.fetch.payload", true, |bytes| {
                font::Fetch::decode(bytes).map(|_| ())
            }),
            ("terminal.create.payload", true, |bytes| {
                terminal::Create::decode(bytes).map(|_| ())
            }),
            ("terminal.frame.byte_budget.payload", true, |bytes| {
                let frame = terminal::TerminalFrame::decode(bytes)?;
                frame.decode_grid_codec1(4096, Some((24, 80))).map(|_| ())
            }),
            ("terminal.close_view.payload", true, |bytes| {
                terminal::CloseView::decode(bytes).map(|_| ())
            }),
            ("terminal.query_inline.payload", true, |bytes| {
                terminal::QueryBody::decode(bytes).map(|_| ())
            }),
            ("terminal.read.payload", true, |bytes| {
                terminal::Read::decode(bytes).map(|_| ())
            }),
            ("terminal.search.payload", true, |bytes| {
                terminal::Search::decode(bytes).map(|_| ())
            }),
            ("terminal.cwd.payload", true, |bytes| {
                terminal::CwdQuery::decode(bytes).map(|_| ())
            }),
            ("terminal.journal.payload", true, |bytes| {
                terminal::Journal::decode(bytes).map(|_| ())
            }),
            ("terminal.output.payload", true, |bytes| {
                terminal::Output::decode(bytes).map(|_| ())
            }),
            ("terminal.wait.payload", true, |bytes| {
                terminal::Wait::decode(bytes).map(|_| ())
            }),
            ("terminal.copy_range.payload", true, |bytes| {
                terminal::CopyRange::decode(bytes).map(|_| ())
            }),
            ("terminal.search_results.payload", true, |bytes| {
                terminal::SearchResults::decode(bytes).map(|_| ())
            }),
            ("terminal.journal_result.payload", true, |bytes| {
                terminal::JournalResult::decode(bytes).map(|_| ())
            }),
            ("terminal.output_result.payload", true, |bytes| {
                terminal::OutputResult::decode(bytes).map(|_| ())
            }),
            ("terminal.styled_lines.payload", true, |bytes| {
                terminal::StyledLines::decode(bytes).map(|_| ())
            }),
            ("terminal.text_and_styled.payload", true, |bytes| {
                terminal::TextAndStyled::decode(bytes).map(|_| ())
            }),
            ("client.disconnect.payload", true, |bytes| {
                client::Disconnect::decode(bytes).map(|_| ())
            }),
            ("client.bandwidth_rates.payload", true, |bytes| {
                client::BandwidthRates::decode(bytes).map(|_| ())
            }),
            ("surface.create_app_endpoint.payload", true, |bytes| {
                surface::CreateAppEndpoint::decode(bytes).map(|_| ())
            }),
            (
                "surface.create_app_endpoint_result.payload",
                true,
                |bytes| surface::CreateAppEndpointResult::decode(bytes).map(|_| ()),
            ),
            ("surface.release_app_endpoint.payload", true, |bytes| {
                surface::ReleaseAppEndpoint::decode(bytes).map(|_| ())
            }),
            ("surface.open_view.payload", true, |bytes| {
                surface::OpenView::decode(bytes).map(|_| ())
            }),
            ("surface.remote_input.payload", true, |bytes| {
                surface::RemoteInput::decode(bytes).map(|_| ())
            }),
            ("selection.drag_get.payload", true, |bytes| {
                selection::Get::decode(bytes).map(|_| ())
            }),
            ("selection.drag_drop.payload", true, |bytes| {
                selection::DragDrop::decode(bytes).map(|_| ())
            }),
            ("desktop.fetch_asset.payload", true, |bytes| {
                desktop::FetchAsset::decode(bytes).map(|_| ())
            }),
            ("desktop.tray_action.payload", true, |bytes| {
                desktop::TrayAction::decode(bytes).map(|_| ())
            }),
            ("desktop.notification_action.payload", true, |bytes| {
                desktop::NotificationAction::decode(bytes).map(|_| ())
            }),
            ("desktop.notification_record.payload", true, |bytes| {
                desktop::NotificationRecord::decode(bytes).map(|_| ())
            }),
            ("desktop.notification_patch.payload", true, |bytes| {
                desktop::decode_state_record(&state::Record {
                    kind: state::RecordKind::Patch,
                    required: false,
                    body: bytes.to_vec(),
                })
                .map(|_| ())
            }),
            ("desktop.notification_remove.payload", true, |bytes| {
                desktop::decode_state_record(&state::Record {
                    kind: state::RecordKind::Remove,
                    required: false,
                    body: bytes.to_vec(),
                })
                .map(|_| ())
            }),
            ("media.fetch_asset.payload", true, |bytes| {
                media::FetchAsset::decode(bytes).map(|_| ())
            }),
            ("media.portal_access_request.payload", true, |bytes| {
                media::PortalRequest::decode(bytes).map(|_| ())
            }),
            ("media.portal_access_reply.payload", true, |bytes| {
                media::PortalReply::decode(bytes).map(|_| ())
            }),
            ("media.portal_screencast_request.payload", true, |bytes| {
                media::PortalRequest::decode(bytes).map(|_| ())
            }),
            ("media.portal_screencast_reply.payload", true, |bytes| {
                media::PortalReply::decode(bytes).map(|_| ())
            }),
            ("media.portal_close.payload", true, |bytes| {
                media::PortalClose::decode(bytes).map(|_| ())
            }),
            ("media.portal_granted.payload", true, |bytes| {
                media::PortalRecord::decode(bytes).map(|_| ())
            }),
            ("env.get.payload", true, |bytes| {
                env::Get::decode(bytes).map(|_| ())
            }),
            ("env.inline.payload", true, |bytes| {
                env::GetResult::decode(bytes).map(|_| ())
            }),
            ("env.transfer.payload", true, |bytes| {
                env::GetResult::decode(bytes).map(|_| ())
            }),
            ("env.batch.payload", true, |bytes| {
                env::SnapshotBatch::decode(bytes).map(|_| ())
            }),
            ("kv.open.payload", true, |bytes| {
                kv::Open::decode(bytes).map(|_| ())
            }),
            ("kv.watch.payload", true, |bytes| {
                kv::Watch::decode(bytes).map(|_| ())
            }),
            ("kv.entry.inline.payload", true, |bytes| {
                kv::EntryRecord::decode(bytes).map(|_| ())
            }),
            ("kv.get.transfer.payload", true, |bytes| {
                kv::GetResult::decode(bytes).map(|_| ())
            }),
            ("kv.stage_value.result.payload", true, |bytes| {
                kv::StageValueResult::decode(bytes).map(|_| ())
            }),
            ("kv.put.inline.payload", true, |bytes| {
                kv::Put::decode(bytes).map(|_| ())
            }),
            ("kv.mutation_result.payload", true, |bytes| {
                kv::MutationResult::decode(bytes).map(|_| ())
            }),
            ("kv.batch.payload", true, |bytes| {
                kv::Batch::decode(bytes).map(|_| ())
            }),
            ("channel.listen.payload", true, |bytes| {
                channel::Listen::decode(bytes).map(|_| ())
            }),
            ("channel.listen.max_metadata.payload", true, |bytes| {
                channel::Listen::decode(bytes).map(|_| ())
            }),
            ("channel.connect.payload", true, |bytes| {
                channel::Connect::decode(bytes).map(|_| ())
            }),
            ("channel.accept.payload", true, |bytes| {
                channel::Accept::decode(bytes).map(|_| ())
            }),
            ("process.spawn.payload", true, |bytes| {
                process::Spawn::decode(bytes).map(|_| ())
            }),
            ("process.stream_bundle.payload", true, |bytes| {
                process::StreamBundle::decode(bytes).map(|_| ())
            }),
            ("process.exit.payload", true, |bytes| {
                process::ExitRecord::decode(bytes).map(|_| ())
            }),
            ("fs.open.payload", true, |bytes| {
                fs::Open::decode(bytes).map(|_| ())
            }),
            ("fs.close.payload", true, |bytes| {
                fs::Close::decode(bytes).map(|_| ())
            }),
            ("fs.watch.payload", true, |bytes| {
                fs::Watch::decode(bytes).map(|_| ())
            }),
            ("fs.unwatch.payload", true, |bytes| {
                state::Unwatch::decode(bytes).map(|_| ())
            }),
            ("fs.fetch.payload", true, |bytes| {
                fs::Fetch::decode(bytes).map(|_| ())
            }),
            ("fs.read.payload", true, |bytes| {
                fs::Read::decode(bytes).map(|_| ())
            }),
            ("fs.search.payload", true, |bytes| {
                fs::Search::decode(bytes).map(|_| ())
            }),
            ("fs.index.payload", true, |bytes| {
                fs::Index::decode(bytes).map(|_| ())
            }),
            ("fs.grep.payload", true, |bytes| {
                fs::Grep::decode(bytes).map(|_| ())
            }),
            ("fs.stage_write.payload", true, |bytes| {
                fs::StageWrite::decode(bytes).map(|_| ())
            }),
            ("fs.commit.payload", true, |bytes| {
                fs::Commit::decode(bytes).map(|_| ())
            }),
            ("fs.apply.payload", true, |bytes| {
                fs::Apply::decode(bytes).map(|_| ())
            }),
            ("fs.entry.inline.payload", true, |bytes| {
                fs::EntryRecord::decode(bytes).map(|_| ())
            }),
            ("fs.query.inline.payload", true, |bytes| {
                fs::QueryPage::decode(bytes).map(|_| ())
            }),
            ("fs.query.batch.payload", true, |bytes| {
                fs::QueryRecordBatch::decode(bytes).map(|_| ())
            }),
            ("fs.query.read_record.payload", true, |bytes| {
                fs::QueryReadRecord::decode(bytes).map(|_| ())
            }),
            ("fs.query.path_record.payload", true, |bytes| {
                fs::QueryPathRecord::decode(bytes).map(|_| ())
            }),
            ("fs.query.grep_file_record.payload", true, |bytes| {
                fs::QueryGrepFileRecord::decode(bytes).map(|_| ())
            }),
            ("fs.query.grep_match_record.payload", true, |bytes| {
                fs::QueryGrepMatchRecord::decode(bytes).map(|_| ())
            }),
            ("fs.conflict_detail.payload", true, |bytes| {
                fs::ConflictDetail::decode(bytes).map(|_| ())
            }),
            ("fs.commit_result.payload", true, |bytes| {
                fs::CommitResult::decode(bytes).map(|_| ())
            }),
            ("fs.apply_result.payload", true, |bytes| {
                fs::ApplyResult::decode(bytes).map(|_| ())
            }),
            ("fs.state.move.payload", true, |bytes| {
                fs::MoveRecord::decode(bytes).map(|_| ())
            }),
            ("git.open.payload", true, |bytes| {
                git::Open::decode(bytes).map(|_| ())
            }),
            ("git.open_terminal.payload", true, |bytes| {
                git::Open::decode(bytes).map(|_| ())
            }),
            ("git.open_result.payload", true, |bytes| {
                git::OpenResult::decode(bytes).map(|_| ())
            }),
            ("git.close.payload", true, |bytes| {
                git::Close::decode(bytes).map(|_| ())
            }),
            ("git.watch.payload", true, |bytes| {
                git::Watch::decode(bytes).map(|_| ())
            }),
            ("git.watch_options.payload", true, |bytes| {
                git::Watch::decode(bytes).map(|_| ())
            }),
            ("git.unwatch.payload", true, |bytes| {
                state::Unwatch::decode(bytes).map(|_| ())
            }),
            ("git.query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.resolve_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.merge_base_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.log_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.tree_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.blob_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.index_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.discover_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.blame_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.reflog_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.worktrees_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.watch_query.payload", true, |bytes| {
                git::WatchQuery::decode(bytes).map(|_| ())
            }),
            ("git.query_state.payload", true, |bytes| {
                git::QueryState::decode(bytes).map(|_| ())
            }),
            ("git.query_state_error.payload", true, |bytes| {
                git::QueryState::decode(bytes).map(|_| ())
            }),
            ("git.unwatch_query.payload", true, |bytes| {
                state::Unwatch::decode(bytes).map(|_| ())
            }),
            ("git.fetch.payload", true, |bytes| {
                git::Fetch::decode(bytes).map(|_| ())
            }),
            ("git.object_id.payload", true, |bytes| {
                git::ObjectId::decode(bytes).map(|_| ())
            }),
            ("git.object_record.payload", true, |bytes| {
                git::ObjectRecord::decode(bytes).map(|_| ())
            }),
            ("git.patch_query.payload", true, |bytes| {
                git::Query::decode(bytes).map(|_| ())
            }),
            ("git.commit.payload", true, |bytes| {
                git::CommitRecord::decode(bytes).map(|_| ())
            }),
            ("git.log_path.payload", true, |bytes| {
                git::LogPathRecord::decode(bytes).map(|_| ())
            }),
            ("git.patch_file.payload", true, |bytes| {
                git::PatchFileRecord::decode(bytes).map(|_| ())
            }),
            ("git.patch_row.payload", true, |bytes| {
                git::PatchRowRecord::decode(bytes).map(|_| ())
            }),
            ("git.patch_gap.payload", true, |bytes| {
                git::PatchGapRecord::decode(bytes).map(|_| ())
            }),
            ("git.patch_base.payload", true, |bytes| {
                git::PatchBaseRecord::decode(bytes).map(|_| ())
            }),
            ("git.query_page.payload", true, |bytes| {
                git::QueryPage::decode(bytes).map(|_| ())
            }),
            ("git.query_cursor.payload", false, |bytes| {
                git::QueryCursor::decode(bytes).map(|_| ())
            }),
            ("git.tree_entry.payload", true, |bytes| {
                git::TreeEntryRecord::decode(bytes).map(|_| ())
            }),
            ("git.blob_content.payload", true, |bytes| {
                git::ContentRecord::decode_blob(bytes).map(|_| ())
            }),
            ("git.diff_record.payload", true, |bytes| {
                git::DiffRecord::decode(bytes).map(|_| ())
            }),
            ("git.index_record.payload", true, |bytes| {
                git::IndexEntryRecord::decode(bytes).map(|_| ())
            }),
            ("git.discovery_record.payload", true, |bytes| {
                git::DiscoveryRecord::decode(bytes).map(|_| ())
            }),
            ("git.blame_record.payload", true, |bytes| {
                git::BlameRecord::decode(bytes).map(|_| ())
            }),
            ("git.reflog_record.payload", true, |bytes| {
                git::ReflogRecord::decode(bytes).map(|_| ())
            }),
            ("git.worktree_record.payload", true, |bytes| {
                git::WorktreeRecord::decode(bytes).map(|_| ())
            }),
            ("git.fetch_result.payload", true, |bytes| {
                git::FetchResult::decode(bytes).map(|_| ())
            }),
            ("git.entity.payload", true, |bytes| {
                git::EntityRecord::decode(bytes).map(|_| ())
            }),
            ("git.entity.head.payload", true, |bytes| {
                git::EntityRecord::decode(bytes).map(|_| ())
            }),
            ("git.entity.ref.payload", true, |bytes| {
                git::EntityRecord::decode(bytes).map(|_| ())
            }),
            ("git.entity.remote.payload", true, |bytes| {
                git::EntityRecord::decode(bytes).map(|_| ())
            }),
            ("git.entity.operation.payload", true, |bytes| {
                git::EntityRecord::decode(bytes).map(|_| ())
            }),
            ("git.entity.status.payload", true, |bytes| {
                git::EntityRecord::decode(bytes).map(|_| ())
            }),
            ("git.entity.upstream.payload", true, |bytes| {
                git::EntityRecord::decode(bytes).map(|_| ())
            }),
            ("git.entity.stash.payload", true, |bytes| {
                git::EntityRecord::decode(bytes).map(|_| ())
            }),
            ("git.entity.worktree_generation.payload", true, |bytes| {
                git::EntityRecord::decode(bytes).map(|_| ())
            }),
            ("git.progress.payload", true, |bytes| {
                git::Progress::decode(bytes).map(|_| ())
            }),
            ("git.closed.payload", true, |bytes| {
                git::Closed::decode(bytes).map(|_| ())
            }),
            ("lsp.open.payload", true, |bytes| {
                lsp::Open::decode(bytes).map(|_| ())
            }),
            ("lsp.open_auto.payload", true, |bytes| {
                lsp::Open::decode(bytes).map(|_| ())
            }),
            ("lsp.open_result.payload", true, |bytes| {
                lsp::OpenResult::decode(bytes).map(|_| ())
            }),
            ("lsp.open_result_no_backend.payload", true, |bytes| {
                lsp::OpenResult::decode(bytes).map(|_| ())
            }),
            ("lsp.workspace_source.platform.payload", true, |bytes| {
                lsp::WorkspaceSource::decode(bytes).map(|_| ())
            }),
            ("lsp.close.payload", true, |bytes| {
                lsp::Close::decode(bytes).map(|_| ())
            }),
            ("lsp.closed.payload", true, |bytes| {
                lsp::Closed::decode(bytes).map(|_| ())
            }),
            ("lsp.watch.payload", true, |bytes| {
                lsp::Watch::decode(bytes).map(|_| ())
            }),
            ("lsp.unwatch.payload", true, |bytes| {
                state::Unwatch::decode(bytes).map(|_| ())
            }),
            ("lsp.query.payload", true, |bytes| {
                lsp::Query::decode(bytes).map(|_| ())
            }),
            ("lsp.signature_query.payload", true, |bytes| {
                lsp::QueryBody::decode(bytes).map(|_| ())
            }),
            ("lsp.buffer_put.payload", true, |bytes| {
                lsp::BufferPut::decode(bytes).map(|_| ())
            }),
            ("lsp.buffer_begin.payload", true, |bytes| {
                lsp::BufferBegin::decode(bytes).map(|_| ())
            }),
            ("lsp.buffer_commit.payload", true, |bytes| {
                lsp::BufferCommit::decode(bytes).map(|_| ())
            }),
            ("lsp.buffer_close.payload", true, |bytes| {
                lsp::BufferClose::decode(bytes).map(|_| ())
            }),
            ("lsp.list_servers.payload", true, |bytes| {
                lsp::ListServers::decode(bytes).map(|_| ())
            }),
            ("lsp.stop_server.payload", true, |bytes| {
                lsp::StopServer::decode(bytes).map(|_| ())
            }),
            ("lsp.buffer_begin_result.payload", true, |bytes| {
                lsp::BufferBeginResult::decode(bytes).map(|_| ())
            }),
            ("lsp.query_page.payload", true, |bytes| {
                lsp::QueryPage::decode(bytes).map(|_| ())
            }),
            ("lsp.query_page_incomplete.payload", true, |bytes| {
                lsp::QueryPage::decode(bytes).map(|_| ())
            }),
            ("lsp.location.payload", true, |bytes| {
                lsp::LocationRecord::decode(bytes).map(|_| ())
            }),
            ("lsp.hover.payload", true, |bytes| {
                lsp::HoverRecord::decode(bytes).map(|_| ())
            }),
            ("lsp.symbol.payload", true, |bytes| {
                lsp::SymbolRecord::decode(bytes).map(|_| ())
            }),
            ("lsp.edit.payload", true, |bytes| {
                lsp::EditRecord::decode(bytes).map(|_| ())
            }),
            ("lsp.signature.payload", true, |bytes| {
                lsp::SignatureRecord::decode(bytes).map(|_| ())
            }),
            ("lsp.server.payload", true, |bytes| {
                lsp::ServerRecord::decode(bytes).map(|_| ())
            }),
            ("lsp.diagnostics.payload", true, |bytes| {
                lsp::DiagnosticRecord::decode(bytes).map(|_| ())
            }),
            ("lsp.remove.payload", true, |bytes| {
                lsp::RemovedEntity::decode(bytes).map(|_| ())
            }),
            ("events.set_config.payload", true, |bytes| {
                events::SetConfig::decode(bytes).map(|_| ())
            }),
            ("events.dump_result.payload", true, |bytes| {
                events::DumpResult::decode(bytes).map(|_| ())
            }),
            ("events.record.payload", true, |bytes| {
                events::RecordEvent::decode(bytes).map(|_| ())
            }),
            ("events.recording_info.payload", true, |bytes| {
                events::RecordingInfo::decode(bytes).map(|_| ())
            }),
            ("extension.object_begin_result.payload", true, |bytes| {
                extension::ObjectBeginResult::decode(bytes).map(|_| ())
            }),
            ("extension.deploy.payload", true, |bytes| {
                extension::Deploy::decode(bytes).map(|_| ())
            }),
            ("extension.state.payload", true, |bytes| {
                extension::ExtensionRecord::decode(bytes).map(|_| ())
            }),
            ("extension.follow_result.payload", true, |bytes| {
                extension::FollowResult::decode(bytes).map(|_| ())
            }),
            ("extension.output_batch.payload", true, |bytes| {
                extension::OutputBatch::decode(bytes).map(|_| ())
            }),
            ("extension.command_page.payload", true, |bytes| {
                extension::CommandPage::decode(bytes).map(|_| ())
            }),
            ("extension.attempt_context.payload", true, |bytes| {
                extension::AttemptContext::decode(bytes).map(|_| ())
            }),
            ("net.open.payload", true, |bytes| {
                net::Open::decode(bytes).map(|_| ())
            }),
            ("net.endpoint.payload", true, |bytes| {
                net::Endpoint::decode(bytes).map(|_| ())
            }),
            ("net.datagram.payload", false, |bytes| {
                net::Datagram::decode(bytes).map(|_| ())
            }),
            ("net.datagram_stats.payload", true, |bytes| {
                net::DatagramStats::decode(bytes).map(|_| ())
            }),
        ];
        for generated in schema::GOLDEN_VECTORS.iter().filter(|vector| {
            vector.name.ends_with(".payload") && !vector.name.starts_with("packed_codec.")
        }) {
            assert!(
                cases.iter().any(|(name, _, _)| *name == generated.name),
                "generated payload vector {} lacks a Rust decode gate",
                generated.name
            );
        }
        for family in schema::FAMILIES {
            let short_name = family.name.strip_prefix("yas.").unwrap();
            let prefix = format!("{short_name}.");
            assert!(
                schema::GOLDEN_VECTORS.iter().any(|vector| {
                    vector.name.starts_with(&prefix) && vector.name.ends_with(".payload")
                }),
                "family {} lacks a full payload vector",
                family.name
            );
        }
        for (name, all_truncations, decode) in cases {
            let bytes = vector(name);
            if *all_truncations {
                truncations(&bytes, decode);
            } else {
                decode(&bytes).unwrap();
            }
        }
    }
}
