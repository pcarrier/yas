use std::collections::BTreeSet;

use super::*;

#[test]
fn outbound_state_transfer_and_view_windows_share_one_exact_budget() {
    let budget = Arc::new(CreditBudget::new(10));

    // State and Transfer are both FlowControl owners of this session budget.
    let state = FlowControl::new(3, Arc::clone(&budget));
    let transfer = FlowControl::new(4, Arc::clone(&budget));
    let terminal = budget.try_lease_exact(2).expect("Terminal frame window");
    assert_eq!(budget.reserved.load(Ordering::Acquire), 9);
    assert!(
        budget.try_lease_exact(2).is_none(),
        "a Surface view cannot overcommit the shared peer budget"
    );

    drop(transfer);
    let surface = budget
        .try_lease_exact(2)
        .expect("released Transfer window is reusable by a view");
    assert_eq!(budget.reserved.load(Ordering::Acquire), 7);
    drop((surface, terminal, state));
    assert_eq!(
        budget.reserved.load(Ordering::Acquire),
        0,
        "session-owner cleanup releases every outbound reservation"
    );
}

#[test]
fn outbound_view_growth_is_exact_and_shrink_keeps_the_high_water_reservation() {
    let budget = Arc::new(CreditBudget::new(20));
    let state = FlowControl::new(5, Arc::clone(&budget));
    let view = budget.try_lease_exact(8).expect("initial Surface window");
    let transfer = FlowControl::new(7, Arc::clone(&budget));
    assert_eq!(budget.reserved.load(Ordering::Acquire), 20);

    assert!(budget.try_lease_exact(1).is_none());
    assert_eq!(view.bytes(), 8, "failed growth leaves capacity unchanged");
    assert_eq!(budget.reserved.load(Ordering::Acquire), 20);

    drop(transfer);
    let growth = budget.try_lease_exact(4).expect("exact growth delta");
    assert!(growth.transfer_to(&view, 4));
    assert_eq!(view.bytes(), 12);
    assert_eq!(budget.reserved.load(Ordering::Acquire), 17);
    // A CONFIGURE shrink changes decoder_capacity but keeps this 12-byte
    // high-water lease: already-written frames may still be retained by the
    // peer even after their writer receipts have fired.
    assert_eq!(view.bytes(), 12);
    assert!(
        budget.try_lease_exact(4).is_none(),
        "a shrink cannot make old in-flight capacity reusable"
    );
    drop(view);
    let reused = budget
        .try_lease_exact(12)
        .expect("view close releases its high-water reservation");
    drop((reused, state));
}

#[test]
fn canonical_native_views_fit_the_validated_peer_budget_above_server_receive_cap() {
    const MIB: u64 = 1024 * 1024;
    let budget = Arc::new(CreditBudget::new(TEST_PEER_MAX_BUFFERED));
    let watches = (0..NATIVE_APP_BASELINE_WATCHES)
        .map(|_| FlowControl::new(MIB, Arc::clone(&budget)))
        .collect::<Vec<_>>();
    let terminal = budget
        .try_lease_exact(u64::from(SERVER_MAX_DECODED))
        .expect("canonical Terminal window");
    let surface = budget
        .try_lease_exact(u64::from(SERVER_MAX_DECODED))
        .expect("canonical Surface window");
    assert_eq!(
        budget.reserved.load(Ordering::Acquire),
        17 * MIB,
        "nine 1 MiB State windows plus two 4 MiB view windows"
    );
    assert!(budget.reserved.load(Ordering::Acquire) > SERVER_MAX_BUFFERED);
    drop((watches, terminal, surface));
    assert_eq!(budget.reserved.load(Ordering::Acquire), 0);
}

#[tokio::test]
async fn confirmed_view_publication_barrier_precedes_data_activation() {
    let cases = [
        (
            family::TERMINAL,
            yas_wire::schema::terminal::request::OPEN_VIEW,
            yas_wire::schema::terminal::event::FRAME,
        ),
        (
            family::TERMINAL,
            yas_wire::schema::terminal::request::CREATE,
            yas_wire::schema::terminal::event::FRAME,
        ),
        (
            family::SURFACE,
            yas_wire::schema::surface::request::OPEN_VIEW,
            yas_wire::schema::surface::event::FRAME,
        ),
        (
            family::SURFACE,
            yas_wire::schema::surface::request::CONFIGURE_VIEW,
            yas_wire::schema::surface::event::FRAME,
        ),
    ];
    for (family_id, request_kind, frame_kind) in cases {
        let result = Frame {
            header: FrameHeader::result(family_id, request_kind, 7),
            payload: vec![0x70],
        };
        let frame = Frame {
            header: FrameHeader::event(family_id, frame_kind),
            payload: vec![0xda],
        };
        let mut outbound = test_outbound(1, decoded_len(&result), decoded_len(&frame));
        let publication = tokio::spawn({
            let sender = outbound.sender.clone();
            async move {
                sender.send_confirmed(result).await.unwrap();
                sender.send(frame).await.unwrap();
            }
        });
        wait_for_queued(&outbound.receivers, 0, 1, 0).await;
        assert!(
            !publication.is_finished(),
            "{family_id:#06x}/{request_kind:#06x} activated before its Result write"
        );
        let mut queued = outbound.receivers.control.recv().await.unwrap();
        queued.written.take().unwrap().send(()).unwrap();
        drop(queued);
        timeout(TEST_TIMEOUT, publication).await.unwrap().unwrap();
        wait_for_queued(&outbound.receivers, 0, 0, 1).await;
        let queued = outbound.receivers.data.recv().await.unwrap();
        assert_eq!(queued.frame.header.family, family_id);
        assert_eq!(queued.frame.header.kind, frame_kind);
    }
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn surface_view_budget_rejects_without_mutation_then_releases_and_resizes() {
    const LIMIT: u64 = 3 * SERVER_MAX_DECODED as u64;
    let state = super::super::super::tests::process_transport::test_state(
        super::super::super::process::Server::new(false, true),
    );
    let surface_handle = {
        let mut shared = state.session.lock().await;
        add_test_surface(&mut shared, 71)
    };
    let (mut client, codec, _hello, server_task) =
        start_registered_session_with_receive(state.clone(), &[family::SURFACE], LIMIT).await;

    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::WATCH,
        10,
        &Watch {
            initial_credit: 1024 * 1024,
            resume: None,
            extensions: Extensions::default(),
        },
    )
    .await;
    let watched = matching_result(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::WATCH,
        10,
    )
    .await;
    assert_eq!(watched.status, Status::Ok);
    let subscription_id = WatchResult::decode(&watched.body).unwrap().subscription_id;

    let open = |decoder_capacity| yas_surface::OpenView {
        surface_handle,
        width: 320,
        height: 180,
        max_fps: 60,
        decoder_capacity,
        codec_versions: vec![yas_wire::schema::surface::CODEC_H264_V1 as u16],
        extensions: Extensions::default(),
    };
    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::OPEN_VIEW,
        11,
        &open(2),
    )
    .await;
    let first = matching_result(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::OPEN_VIEW,
        11,
    )
    .await;
    assert_eq!(first.status, Status::Ok);
    let first = yas_surface::ViewResult::decode(&first.body).unwrap();

    let configure = |decoder_capacity| yas_surface::ConfigureView {
        view_id: first.view_id,
        width: 400,
        height: 240,
        max_fps: 30,
        decoder_capacity,
        latency_target_ns: 20_000_000,
        extensions: Extensions::default(),
    };
    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::CONFIGURE_VIEW,
        12,
        &configure(3),
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::SURFACE,
            yas_wire::schema::surface::request::CONFIGURE_VIEW,
            12,
        )
        .await
        .status,
        Status::ResourceExhausted,
    );
    assert!(state.session.lock().await.clients.values().any(|client| {
        !client.catalog_visible
            && client
                .surface_subs
                .get(&71)
                .is_some_and(|subscription| subscription.scaled_target == Some((320, 180)))
    }));

    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::UNWATCH,
        13,
        &Unwatch { subscription_id },
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::SURFACE,
            yas_wire::schema::surface::request::UNWATCH,
            13,
        )
        .await
        .status,
        Status::Ok,
    );

    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::CONFIGURE_VIEW,
        14,
        &configure(3),
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::SURFACE,
            yas_wire::schema::surface::request::CONFIGURE_VIEW,
            14,
        )
        .await
        .status,
        Status::Ok,
    );
    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::CONFIGURE_VIEW,
        15,
        &configure(1),
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::SURFACE,
            yas_wire::schema::surface::request::CONFIGURE_VIEW,
            15,
        )
        .await
        .status,
        Status::Ok,
    );

    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::OPEN_VIEW,
        16,
        &open(1),
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::SURFACE,
            yas_wire::schema::surface::request::OPEN_VIEW,
            16,
        )
        .await
        .status,
        Status::ResourceExhausted,
        "a capacity shrink keeps the max-ever reservation",
    );

    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::CLOSE_VIEW,
        17,
        &yas_surface::CloseView {
            view_id: first.view_id,
        },
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::SURFACE,
            yas_wire::schema::surface::request::CLOSE_VIEW,
            17,
        )
        .await
        .status,
        Status::Ok,
    );
    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::OPEN_VIEW,
        18,
        &open(2),
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::SURFACE,
            yas_wire::schema::surface::request::OPEN_VIEW,
            18,
        )
        .await
        .status,
        Status::Ok,
    );
    write_request(
        &mut client,
        &codec,
        family::SURFACE,
        yas_wire::schema::surface::request::OPEN_VIEW,
        19,
        &open(1),
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::SURFACE,
            yas_wire::schema::surface::request::OPEN_VIEW,
            19,
        )
        .await
        .status,
        Status::Ok,
        "view close makes its high-water reservation reusable",
    );

    drop(client);
    timeout(TEST_TIMEOUT, server_task).await.unwrap().unwrap();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReceiveRetention {
    Retaining,
    Ephemeral,
}

#[derive(Clone, Copy, Debug)]
struct FamilyInventory {
    id: u16,
    name: &'static str,
    retention: ReceiveRetention,
}

// This is the server-receive authority. Adding a family to the canonical wire
// registry must make this test fail until its retention behavior is reviewed.
const FAMILY_INVENTORY: &[FamilyInventory] = &[
    FamilyInventory {
        id: family::CORE,
        name: "yas.core",
        retention: ReceiveRetention::Ephemeral,
    },
    FamilyInventory {
        id: family::TRANSFER,
        name: "yas.transfer",
        retention: ReceiveRetention::Ephemeral,
    },
    FamilyInventory {
        id: family::RELAY,
        name: "yas.relay",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::TERMINAL,
        name: "yas.terminal",
        retention: ReceiveRetention::Ephemeral,
    },
    FamilyInventory {
        id: family::CLIENT,
        name: "yas.client",
        retention: ReceiveRetention::Ephemeral,
    },
    FamilyInventory {
        id: family::SURFACE,
        name: "yas.surface",
        retention: ReceiveRetention::Ephemeral,
    },
    FamilyInventory {
        id: family::SELECTION,
        name: "yas.selection",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::DESKTOP,
        name: "yas.desktop",
        retention: ReceiveRetention::Ephemeral,
    },
    FamilyInventory {
        id: family::MEDIA,
        name: "yas.media",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::FONT,
        name: "yas.font",
        retention: ReceiveRetention::Ephemeral,
    },
    FamilyInventory {
        id: family::FS,
        name: "yas.fs",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::GIT,
        name: "yas.git",
        retention: ReceiveRetention::Ephemeral,
    },
    FamilyInventory {
        id: family::LSP,
        name: "yas.lsp",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::KV,
        name: "yas.kv",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::PROCESS,
        name: "yas.process",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::NET,
        name: "yas.net",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::CHANNEL,
        name: "yas.channel",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::EXTENSION,
        name: "yas.extension",
        retention: ReceiveRetention::Retaining,
    },
    FamilyInventory {
        id: family::EVENTS,
        name: "yas.events",
        retention: ReceiveRetention::Ephemeral,
    },
    FamilyInventory {
        id: family::ENV,
        name: "yas.env",
        retention: ReceiveRetention::Ephemeral,
    },
];

#[derive(Clone, Copy, Debug)]
struct RetainerPath {
    owner: Option<u16>,
    source: &'static str,
    lifecycle: &'static str,
}

// Every production object that can hold server-received bytes after its
// decoder call returns belongs here. The two ownerless entries are transport
// queues shared by all families; all other entries name the semantic owner.
const RETAINER_PATHS: &[RetainerPath] = &[
    RetainerPath {
        owner: None,
        source: "InboundFrame._credit",
        lifecycle: "ordinary reliable decoded queue",
    },
    RetainerPath {
        owner: None,
        source: "composite_link::InboundDatagram._credit",
        lifecycle: "optional composite datagram queue",
    },
    RetainerPath {
        owner: Some(family::PROCESS),
        source: "ProcessInputTransfer.receive_credit",
        lifecycle: "process stdin window",
    },
    RetainerPath {
        owner: Some(family::LSP),
        source: "LspStageTransfer.receive_credit",
        lifecycle: "LSP buffer upload window",
    },
    RetainerPath {
        owner: Some(family::LSP),
        source: "LspStageTransfer.retained_credit",
        lifecycle: "sealed LSP upload stage before commit",
    },
    RetainerPath {
        owner: Some(family::LSP),
        source: "LspRuntime.buffer_credits",
        lifecycle: "committed inline/staged LSP buffer through replacement, buffer/workspace close, or session drop",
    },
    RetainerPath {
        owner: Some(family::EXTENSION),
        source: "ExtensionStageTransfer.receive_credit",
        lifecycle: "Extension object upload window",
    },
    RetainerPath {
        owner: Some(family::EXTENSION),
        source: "ExtensionStageTransfer.retained_credit",
        lifecycle: "sealed Extension object",
    },
    RetainerPath {
        owner: Some(family::FS),
        source: "FsStageTransfer.receive_credit",
        lifecycle: "FS upload rolling window",
    },
    RetainerPath {
        owner: Some(family::FS),
        source: "FS semantic task captured inbound_credit",
        lifecycle: "original OPEN/WATCH/FETCH/READ/SEARCH/INDEX/GREP/COMMIT/APPLY frame through backend completion",
    },
    RetainerPath {
        owner: Some(family::NET),
        source: "NetTcpFlow._receive_credit",
        lifecycle: "Net TCP byte ingress window",
    },
    RetainerPath {
        owner: Some(family::NET),
        source: "NetMessageFlow._receive_credit",
        lifecycle: "Net seqpacket/message ingress window",
    },
    RetainerPath {
        owner: Some(family::NET),
        source: "net::QueuedDatagram._credit",
        lifecycle: "Net UDP ingress queue",
    },
    RetainerPath {
        owner: Some(family::SELECTION),
        source: "SelectionStage.retained_credit",
        lifecycle: "Selection SET retained stage",
    },
    RetainerPath {
        owner: Some(family::SELECTION),
        source: "SelectionStageItem._receive_credit",
        lifecycle: "Selection SET upload window",
    },
    RetainerPath {
        owner: Some(family::SELECTION),
        source: "PendingDragGet.receive_credit",
        lifecycle: "Selection drag GET reservation",
    },
    RetainerPath {
        owner: Some(family::SELECTION),
        source: "DragFetchTransfer._receive_credit",
        lifecycle: "Selection drag fetch",
    },
    RetainerPath {
        owner: Some(family::SELECTION),
        source: "RetainedDragItem._credit",
        lifecycle: "Selection retained drop data",
    },
    RetainerPath {
        owner: Some(family::CHANNEL),
        source: "ChannelTransfer.receive_credit",
        lifecycle: "Channel message ingress window",
    },
    RetainerPath {
        owner: Some(family::KV),
        source: "KvStage._receive_credit",
        lifecycle: "KV stage upload window",
    },
    RetainerPath {
        owner: Some(family::KV),
        source: "KvStage.retained_credit",
        lifecycle: "sealed KV stage",
    },
    RetainerPath {
        owner: Some(family::MEDIA),
        source: "MediaReassembly._credit",
        lifecycle: "Media input frame reassembly",
    },
    RetainerPath {
        owner: Some(family::RELAY),
        source: "RelayTransfer._receive_credit",
        lifecycle: "Relay input window",
    },
];

#[derive(Clone, Copy, Debug)]
struct BoundedLifecycleRetention {
    owner: u16,
    source: &'static str,
    cap: usize,
    lifecycle: &'static str,
}

// These retain asynchronous semantic work, pending state, or replay
// tombstones rather than aggregate receive bytes. Keep them separate from
// RETAINER_PATHS so SESSION_INFO's byte total remains exact while
// admission/lifecycle review still has one authority.
const BOUNDED_LIFECYCLE_RETENTION: &[BoundedLifecycleRetention] = &[
    BoundedLifecycleRetention {
        owner: family::PROCESS,
        source: "PendingOperation::Process::{Attach,Control}",
        cap: super::super::MAX_PENDING_PROCESS_SEMANTIC_OPERATIONS,
        lifecycle: "permit survives wire cancellation through backend completion",
    },
    BoundedLifecycleRetention {
        owner: family::PROCESS,
        source: "PendingOperation::Process::Wait",
        cap: super::super::MAX_PENDING_PROCESS_WAITS,
        lifecycle: "permit survives wire CANCEL until detached process wait cleanup completes",
    },
    BoundedLifecycleRetention {
        owner: family::FS,
        source: "PendingOperation::FsSemantic",
        cap: super::super::MAX_PENDING_FS_SEMANTIC_OPERATIONS,
        lifecycle: "OPEN/WATCH/FETCH/READ/SEARCH/INDEX/GREP/COMMIT/APPLY permit and request lease survive wire CANCEL until backend completion",
    },
    BoundedLifecycleRetention {
        owner: family::SELECTION,
        source: "SelectionRuntime.pending_drops",
        cap: yas_wire::schema::selection::MAX_ACTIVE_DRAGS_PER_SESSION as usize,
        lifecycle: "one pending drop per drag; total survives materialization until completion, cancellation, or session cleanup",
    },
    BoundedLifecycleRetention {
        owner: family::KV,
        source: "kv::WriteJob",
        cap: super::super::super::kv::MAX_PENDING_WRITES,
        lifecycle: "process-global writer permit is acquired before mutation and held through redb commit plus durable reply settlement",
    },
    BoundedLifecycleRetention {
        owner: family::KV,
        source: "kv durable operation replay horizon",
        cap: super::super::super::kv::MAX_RECENT_OPERATION_REPLAYS,
        lifecycle: "recent durable results and live-revision anchors are pruned by settlement sequence",
    },
    BoundedLifecycleRetention {
        owner: family::KV,
        source: "KvConsumedStages",
        cap: super::super::super::kv::MAX_RECENT_OPERATION_REPLAYS,
        lifecycle: "per-session consumed-stage tombstones retain only the recent operation replay horizon",
    },
    BoundedLifecycleRetention {
        owner: family::EVENTS,
        source: "PendingOperation::EventsDump",
        cap: yas_wire::schema::events::MAX_PENDING_DUMPS as usize,
        lifecycle: "generation-tagged permit survives wire cancellation until the dump backend completion",
    },
    BoundedLifecycleRetention {
        owner: family::EVENTS,
        source: "PendingOperation::EventsStreamStart",
        cap: yas_wire::schema::events::MAX_STREAMS_PER_SESSION as usize,
        lifecycle: "generation-tagged start permit survives wire cancellation until stream admission settles",
    },
];

#[test]
fn receive_retention_inventory_exactly_covers_the_canonical_family_registry() {
    let canonical = yas_wire::schema::FAMILIES
        .iter()
        .map(|metadata| (metadata.id, metadata.name))
        .collect::<BTreeSet<_>>();
    let classified = FAMILY_INVENTORY
        .iter()
        .map(|entry| (entry.id, entry.name))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        classified.len(),
        FAMILY_INVENTORY.len(),
        "duplicate family inventory entry"
    );
    assert_eq!(
        classified, canonical,
        "new/renamed wire families require an explicit receive-retention review"
    );

    let retaining = FAMILY_INVENTORY
        .iter()
        .filter(|entry| entry.retention == ReceiveRetention::Retaining)
        .map(|entry| entry.id)
        .collect::<BTreeSet<_>>();
    let path_owners = RETAINER_PATHS
        .iter()
        .filter_map(|path| path.owner)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        path_owners, retaining,
        "every retaining family needs a concrete charged owner path"
    );
    assert!(
        RETAINER_PATHS
            .iter()
            .all(|path| !path.source.is_empty() && !path.lifecycle.is_empty())
    );
    assert!(BOUNDED_LIFECYCLE_RETENTION.iter().all(|entry| {
        canonical.iter().any(|(id, _)| *id == entry.owner)
            && entry.cap != 0
            && !entry.source.is_empty()
            && !entry.lifecycle.is_empty()
    }));
    assert_eq!(
        BOUNDED_LIFECYCLE_RETENTION
            .iter()
            .map(|entry| entry.source)
            .collect::<BTreeSet<_>>()
            .len(),
        BOUNDED_LIFECYCLE_RETENTION.len(),
        "bounded semantic task retainers need unique inventory entries"
    );
}

async fn matching_result(
    client: &mut tokio::io::DuplexStream,
    codec: &FrameCodec,
    family_id: u16,
    kind: u16,
    request_id: u32,
) -> ResultPrefix {
    loop {
        let frame = next_frame(client, codec).await;
        if frame.header.family == family_id
            && frame.header.class == Class::Result
            && frame.header.kind == kind
            && frame.header.request_id == Some(request_id)
        {
            return ResultPrefix::decode(&frame.payload).expect("qualified Result prefix");
        }
        assert_eq!(
            frame.header.class,
            Class::Event,
            "unexpected Result while awaiting {family_id:#06x}/{kind:#06x}/{request_id}: {:?}",
            frame.header
        );
    }
}

async fn diagnostics_while_events_are_live(
    client: &mut tokio::io::DuplexStream,
    codec: &FrameCodec,
    request_id: u32,
) -> yas_wire::core::ServerDiagnostics {
    client
        .write_all(
            &codec
                .encode_stream(&Frame {
                    header: FrameHeader::request(
                        family::CORE,
                        yas_wire::core::request_kind::SESSION_INFO,
                        request_id,
                    ),
                    payload: Vec::new(),
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let result = matching_result(
        client,
        codec,
        family::CORE,
        yas_wire::core::request_kind::SESSION_INFO,
        request_id,
    )
    .await;
    assert_eq!(result.status, Status::Ok);
    SessionInfo::decode(&result.body)
        .unwrap()
        .server_diagnostics()
        .unwrap()
        .expect("SESSION_INFO carries server diagnostics")
}

async fn assert_family_admitted(
    client: &mut tokio::io::DuplexStream,
    codec: &FrameCodec,
    request_id: u32,
    prior: u64,
    family_name: &str,
) -> u64 {
    let diagnostics = diagnostics_while_events_are_live(client, codec, request_id).await;
    assert!(diagnostics.aggregate_receive_limit >= 2 * 1024 * 1024);
    assert!(
        diagnostics.aggregate_receive_buffered > prior,
        "{family_name} did not add a live aggregate receive reservation: {prior} -> {}",
        diagnostics.aggregate_receive_buffered
    );
    assert!(
        diagnostics.aggregate_receive_buffered <= diagnostics.aggregate_receive_limit,
        "{family_name} exceeded the aggregate receive limit"
    );
    diagnostics.aggregate_receive_buffered
}

async fn assert_owner_released(
    client: &mut tokio::io::DuplexStream,
    codec: &FrameCodec,
    request_id: u32,
    prior: u64,
    owner: &str,
) -> u64 {
    let diagnostics = diagnostics_while_events_are_live(client, codec, request_id).await;
    assert!(
        diagnostics.aggregate_receive_buffered < prior,
        "releasing {owner} did not return aggregate receive credit: {prior} -> {}",
        diagnostics.aggregate_receive_buffered
    );
    assert!(diagnostics.aggregate_receive_buffered <= diagnostics.aggregate_receive_limit);
    diagnostics.aggregate_receive_buffered
}

#[cfg(all(unix, target_os = "linux"))]
#[tokio::test(flavor = "multi_thread")]
async fn every_retaining_family_shares_one_live_session_budget() {
    use std::os::unix::ffi::OsStrExt;

    const LIMIT: u64 = 2 * 1024 * 1024;
    const PEER_RECEIVE_LIMIT: u64 =
        MAX_WATCHES as u64 * yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME as u64;
    const STAGE_BYTES: u64 = 32 * 1024;

    let root = tempfile::tempdir().unwrap();
    let cat_executable =
        std::env::split_paths(&std::env::var_os("PATH").expect("test PATH is set"))
            .map(|directory| directory.join("cat"))
            .find(|path| path.is_file())
            .expect("cat is on PATH")
            .as_os_str()
            .as_bytes()
            .to_vec();
    let extension_root = root.path().join("extension-service");
    std::fs::create_dir(&extension_root).unwrap();
    let extension_service =
        super::super::super::extension::ExtensionService::persistent_for_test(&extension_root);
    let initial_state = super::super::super::tests::process_transport::test_state(
        super::super::super::process::Server::new(false, true),
    );
    let mut state_inner = Arc::try_unwrap(initial_state)
        .ok()
        .expect("fresh aggregate test state is uniquely owned");
    state_inner.config.allow_persistent_extensions = true;
    state_inner.extensions = extension_service;
    let state = Arc::new(state_inner);
    {
        let mut shared = state.session.lock().await;
        shared.ensure_compositor(false, Arc::new(|| {}), "");
        shared
            .compositor
            .as_mut()
            .unwrap()
            .native_media_state_override = Some(super::super::super::MediaBackendState {
            pipewire_available: true,
            microphone_available: true,
            camera_available: false,
            screencasts: Vec::new(),
        });
    }

    let relay_catalogue = Arc::new(RelayRouteCatalog::new());
    relay_catalogue
        .replace_snapshot([("qualification", "socket:qualification")])
        .unwrap();
    let relay_route = relay_catalogue.snapshot().routes[0].clone();
    let (relay_server, _relay_peer) = tokio::io::duplex(1024 * 1024);
    let relay_server = Arc::new(StdMutex::new(Some(relay_server)));
    let relay_connector: Arc<dyn RelayConnector> = {
        let relay_server = Arc::clone(&relay_server);
        Arc::new(move |_uri: String| {
            let stream = relay_server.lock().unwrap().take();
            async move {
                let stream = stream.ok_or_else(|| "qualification relay reused".to_owned())?;
                let (reader, writer) = tokio::io::split(stream);
                Ok((
                    Box::new(reader) as RelayRead,
                    Box::new(writer) as RelayWrite,
                ))
            }
        })
    };

    let mut services = Services::from_state(&state);
    services.receive_max_buffered_override = Some(LIMIT);
    services.relay_catalogue = Some(Arc::clone(&relay_catalogue));
    services.relay_connector = relay_connector;
    services.font_catalogue = Some(Arc::new(FontCatalog::from_paths(
        FontExportPolicy::Deny,
        std::iter::empty::<&std::path::Path>(),
    )));
    services.env_enabled = true;
    services.kv_enabled = true;
    services.channel_enabled = true;
    services.net_enabled = true;
    services.env_snapshot_override = Some(Vec::new());
    services.media_input_lease_override = Some(Arc::new(|start| {
        super::super::super::media_input::InputLease {
            nonce: start.nonce,
            status: super::super::super::media_input::InputStatus::Ok,
            kind: start.kind,
            lease_id: 1,
            codec: start.codec,
            width: start.width,
            height: start.height,
            fps: start.fps,
            initial_credit: 256 * 1024,
        }
    }));

    let offered = FAMILY_INVENTORY
        .iter()
        .filter_map(|entry| (entry.id != family::CORE).then_some(entry.id))
        .collect::<Vec<_>>();
    let (mut client, server) = tokio::io::duplex(4 * 1024 * 1024);
    let (side_server, mut side_peer) = tokio::io::duplex(4 * 1024 * 1024);
    let cancellation = ConnectionCancellation::default();
    let datagram = DatagramLink::open(side_server, 64 * 1024, cancellation.clone());
    let datagram_budget_drops = datagram.inbound_budget_drop_counter();
    let registration = state
        .connections
        .register(cancellation.clone())
        .expect("qualification session registers");
    let server_task = tokio::spawn(serve_registered(
        server,
        services,
        cancellation,
        Some(registration),
        None,
        Some(datagram),
        crate::ConnectionOrigin::Network,
    ));
    let (codec, hello) = handshake_with_receive(&mut client, &offered, PEER_RECEIVE_LIMIT).await;
    assert_eq!(
        hello
            .families
            .iter()
            .map(|descriptor| descriptor.family_id)
            .collect::<BTreeSet<_>>(),
        FAMILY_INVENTORY
            .iter()
            .map(|entry| entry.id)
            .collect::<BTreeSet<_>>(),
        "the all-family pressure session must advertise the full canonical registry"
    );

    let baseline = diagnostics_while_events_are_live(&mut client, &codec, 100).await;
    assert_eq!(baseline.aggregate_receive_limit, LIMIT);
    assert!(baseline.aggregate_receive_buffered > 0);
    let mut buffered = baseline.aggregate_receive_buffered;
    let mut upload_transfers = Vec::new();

    write_request(
        &mut client,
        &codec,
        family::SELECTION,
        yas_wire::schema::selection::request::SET_BEGIN,
        101,
        &yas_selection::SetBegin {
            slot: yas_wire::schema::selection::SLOT_CLIPBOARD as u8,
            operation_id: [0x11; 16],
            items: vec![yas_selection::UploadItem {
                mime: "application/octet-stream".to_owned(),
                byte_len: STAGE_BYTES,
                content_hash: [0x11; 32],
                initial_receive_credit: STAGE_BYTES,
            }],
            extensions: Extensions::default(),
        },
    )
    .await;
    let selection = matching_result(
        &mut client,
        &codec,
        family::SELECTION,
        yas_wire::schema::selection::request::SET_BEGIN,
        101,
    )
    .await;
    assert_eq!(selection.status, Status::Ok);
    let selection = yas_selection::SetBeginResult::decode(&selection.body).unwrap();
    upload_transfers.push(("Selection", selection.descriptors[0].transfer_id));
    buffered = assert_family_admitted(&mut client, &codec, 102, buffered, "Selection").await;

    write_request(
        &mut client,
        &codec,
        family::KV,
        yas_wire::schema::kv::request::STAGE_VALUE,
        103,
        &yas_kv::StageValue {
            byte_len: STAGE_BYTES,
            content_hash: [0x22; 32],
            extensions: Extensions::default(),
        },
    )
    .await;
    let kv = matching_result(
        &mut client,
        &codec,
        family::KV,
        yas_wire::schema::kv::request::STAGE_VALUE,
        103,
    )
    .await;
    assert_eq!(kv.status, Status::Ok);
    let kv = yas_kv::StageValueResult::decode(&kv.body).unwrap();
    upload_transfers.push(("KV", kv.transfer.transfer_id));
    buffered = assert_family_admitted(&mut client, &codec, 104, buffered, "KV").await;

    write_request(
        &mut client,
        &codec,
        family::FS,
        yas_wire::schema::fs::request::OPEN,
        105,
        &yas_fs_wire::Open {
            flags: 0,
            source: yas_fs_wire::RootSource::PlatformPath(
                root.path().as_os_str().as_bytes().to_vec(),
            ),
            extensions: Extensions::default(),
        },
    )
    .await;
    let fs_open = matching_result(
        &mut client,
        &codec,
        family::FS,
        yas_wire::schema::fs::request::OPEN,
        105,
    )
    .await;
    assert_eq!(fs_open.status, Status::Ok);
    let fs_open = yas_fs_wire::OpenResult::decode(&fs_open.body).unwrap();
    write_request(
        &mut client,
        &codec,
        family::FS,
        yas_wire::schema::fs::request::STAGE_WRITE,
        106,
        &yas_fs_wire::StageWrite {
            root_handle: fs_open.root_handle,
            path: yas_fs_wire::Path {
                components: vec![b"qualification.bin".to_vec()],
            },
            precondition: yas_fs_wire::Precondition::Absent,
            flags: 0,
            mode: 0o600,
            byte_len: STAGE_BYTES,
            content_hash: [0x33; 32],
            initial_receive_credit: STAGE_BYTES,
            extensions: Extensions::default(),
        },
    )
    .await;
    let fs = matching_result(
        &mut client,
        &codec,
        family::FS,
        yas_wire::schema::fs::request::STAGE_WRITE,
        106,
    )
    .await;
    assert_eq!(fs.status, Status::Ok);
    let fs = yas_fs_wire::StageWriteResult::decode(&fs.body).unwrap();
    upload_transfers.push(("FS", fs.descriptor.transfer_id));
    buffered = assert_family_admitted(&mut client, &codec, 107, buffered, "FS").await;

    write_request(
        &mut client,
        &codec,
        family::LSP,
        yas_lsp_wire::request_kind::OPEN,
        108,
        &yas_lsp_wire::Open {
            source: yas_lsp_wire::WorkspaceSource::PlatformPath(
                root.path().as_os_str().as_bytes().to_vec(),
            ),
            open_mode: yas_wire::schema::lsp::OPEN_AUTO_DISCOVER as u8,
            diagnostics_settle_ms: 0,
            language: String::new(),
            profile: String::new(),
            initialization_options: Vec::new(),
            extensions: Extensions::default(),
        },
    )
    .await;
    let lsp_open = matching_result(
        &mut client,
        &codec,
        family::LSP,
        yas_lsp_wire::request_kind::OPEN,
        108,
    )
    .await;
    assert_eq!(lsp_open.status, Status::Ok);
    let lsp_open = yas_lsp_wire::OpenResult::decode(&lsp_open.body).unwrap();
    write_request(
        &mut client,
        &codec,
        family::LSP,
        yas_lsp_wire::request_kind::BUFFER_BEGIN,
        109,
        &yas_lsp_wire::BufferBegin {
            workspace_handle: lsp_open.workspace_handle,
            expected_revision: 0,
            path: yas_fs_wire::Path {
                components: vec![b"qualification.txt".to_vec()],
            },
            byte_len: STAGE_BYTES,
            content_hash: [0x44; 32],
            initial_send_credit: STAGE_BYTES,
            extensions: Extensions::default(),
        },
    )
    .await;
    let lsp = matching_result(
        &mut client,
        &codec,
        family::LSP,
        yas_lsp_wire::request_kind::BUFFER_BEGIN,
        109,
    )
    .await;
    assert_eq!(lsp.status, Status::Ok);
    let lsp = yas_lsp_wire::BufferBeginResult::decode(&lsp.body).unwrap();
    upload_transfers.push(("LSP", lsp.descriptor.transfer_id));
    buffered = assert_family_admitted(&mut client, &codec, 110, buffered, "LSP").await;

    write_request(
        &mut client,
        &codec,
        family::EXTENSION,
        yas_extension_wire::request_kind::OBJECT_BEGIN,
        111,
        &yas_extension_wire::ObjectBegin {
            operation_id: [0x55; 16],
            content_hash: [0x55; 32],
            byte_len: STAGE_BYTES,
            extensions: Extensions::default(),
        },
    )
    .await;
    let extension = matching_result(
        &mut client,
        &codec,
        family::EXTENSION,
        yas_extension_wire::request_kind::OBJECT_BEGIN,
        111,
    )
    .await;
    assert_eq!(extension.status, Status::Ok);
    let extension = yas_extension_wire::ObjectBeginResult::decode(&extension.body).unwrap();
    let extension_transfer = extension
        .descriptor
        .as_ref()
        .expect("fresh qualification object uploads")
        .transfer_id;
    upload_transfers.push(("Extension", extension_transfer));
    buffered = assert_family_admitted(&mut client, &codec, 112, buffered, "Extension").await;

    write_request(
        &mut client,
        &codec,
        family::PROCESS,
        yas_process_wire::request_kind::SPAWN,
        113,
        &yas_process_wire::Spawn {
            operation_id: [0x66; 16],
            flags: 0,
            environment_kind: yas_process_wire::EnvironmentKind::Empty,
            cwd: yas_process_wire::Cwd::ServerDefault,
            argv: vec![cat_executable],
            env: Vec::new(),
            stdout_receive_credit: STAGE_BYTES,
            stderr_receive_credit: STAGE_BYTES,
            extensions: Extensions::default(),
        },
    )
    .await;
    let process = matching_result(
        &mut client,
        &codec,
        family::PROCESS,
        yas_process_wire::request_kind::SPAWN,
        113,
    )
    .await;
    assert_eq!(process.status, Status::Ok);
    let process = yas_process_wire::StreamBundle::decode(&process.body).unwrap();
    let process_stdin = process
        .stdin
        .as_ref()
        .expect("cat keeps a stdin receive window")
        .transfer_id;
    buffered = assert_family_admitted(&mut client, &codec, 114, buffered, "Process").await;

    let net_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let net_address = net_listener.local_addr().unwrap();
    let net_peer = tokio::spawn(async move {
        let (stream, _) = net_listener.accept().await.unwrap();
        std::future::pending::<()>().await;
        drop(stream);
    });
    write_request(
        &mut client,
        &codec,
        family::NET,
        yas_wire::schema::net::request::OPEN,
        115,
        &yas_net::Open {
            operation_id: [0x77; 16],
            address: yas_net::Address::Tcp {
                host: net_address.ip().to_string(),
                port: net_address.port(),
            },
            delivery_preference: yas_net::DeliveryPreference::NotApplicable,
            drop_policy: yas_net::DropPolicy::NotApplicable,
            initial_receive_credit: STAGE_BYTES,
            early_data: Vec::new(),
            tls_options: None,
            extensions: Extensions::default(),
        },
    )
    .await;
    let net = matching_result(
        &mut client,
        &codec,
        family::NET,
        yas_wire::schema::net::request::OPEN,
        115,
    )
    .await;
    assert_eq!(net.status, Status::Ok);
    let net = yas_net::Endpoint::decode(&net.body).unwrap();
    let net_transfer = net
        .descriptor
        .as_ref()
        .expect("TCP endpoint carries a receive Transfer")
        .transfer_id;
    buffered = assert_family_admitted(&mut client, &codec, 116, buffered, "Net").await;

    write_request(
        &mut client,
        &codec,
        family::CHANNEL,
        yas_wire::schema::channel::request::LISTEN,
        117,
        &yas_channel::Listen {
            operation_id: [0x88; 16],
            name: "qualification.aggregate".to_owned(),
            metadata: Vec::new(),
            extensions: Extensions::default(),
        },
    )
    .await;
    let listened = matching_result(
        &mut client,
        &codec,
        family::CHANNEL,
        yas_wire::schema::channel::request::LISTEN,
        117,
    )
    .await;
    assert_eq!(listened.status, Status::Ok);
    let listener = yas_channel::ListenerIdentity::decode(&listened.body).unwrap();

    let (mut channel_peer, channel_peer_codec, _, channel_peer_task) =
        start_registered_session_with_receive(
            state.clone(),
            &[family::TRANSFER, family::CHANNEL],
            LIMIT,
        )
        .await;
    write_request(
        &mut channel_peer,
        &channel_peer_codec,
        family::CHANNEL,
        yas_wire::schema::channel::request::CONNECT,
        1,
        &yas_channel::Connect {
            listener_handle: listener.listener_handle,
            generation: listener.generation,
            initial_receive_credit: STAGE_BYTES,
            metadata: Vec::new(),
            extensions: Extensions::default(),
        },
    )
    .await;
    let connected = matching_result(
        &mut channel_peer,
        &channel_peer_codec,
        family::CHANNEL,
        yas_wire::schema::channel::request::CONNECT,
        1,
    )
    .await;
    assert_eq!(connected.status, Status::Ok);
    let connector_endpoint = yas_channel::ChannelEndpoint::decode(&connected.body).unwrap();
    let accepted_frame = next_frame(&mut client, &codec).await;
    assert_eq!(
        accepted_frame.header,
        FrameHeader {
            sensitive: true,
            ..FrameHeader::event(family::CHANNEL, yas_wire::schema::channel::event::ACCEPT,)
        }
    );
    let accepted = yas_channel::Accept::decode(&accepted_frame.payload).unwrap();
    assert!(accepted.endpoint.descriptor.receiver_send_credit > 0);
    let channel_transfer = accepted.endpoint.descriptor.transfer_id;
    buffered = assert_family_admitted(&mut client, &codec, 118, buffered, "Channel").await;

    write_request(
        &mut client,
        &codec,
        family::MEDIA,
        yas_media::request_kind::WATCH,
        119,
        &Watch {
            initial_credit: yas_wire::schema::transport::RECOMMENDED_WIRE_FRAME as u64,
            resume: None,
            extensions: Extensions::default(),
        },
    )
    .await;
    let media_watch = matching_result(
        &mut client,
        &codec,
        family::MEDIA,
        yas_media::request_kind::WATCH,
        119,
    )
    .await;
    assert_eq!(media_watch.status, Status::Ok);
    let media_watch = WatchResult::decode(&media_watch.body).unwrap();
    let mut microphone = None;
    loop {
        let frame = next_frame(&mut client, &codec).await;
        assert_eq!(frame.header.family, family::MEDIA);
        assert_eq!(frame.header.kind, yas_media::event_kind::STATE);
        let event = StateEvent::decode(&frame.payload).unwrap();
        assert_eq!(event.subscription_id, media_watch.subscription_id);
        for record in &event.records {
            if let Ok(yas_media::StateMutation::Complete(yas_media::CompleteEntity::Device(device))) =
                yas_media::decode_state_record(record)
                && device.kind == yas_wire::schema::media::KIND_MICROPHONE as u8
            {
                microphone = Some(device);
            }
        }
        if event.phase == Phase::SnapshotEnd {
            break;
        }
    }
    let microphone = microphone.expect("synthetic microphone is advertised");
    write_request(
        &mut client,
        &codec,
        family::MEDIA,
        yas_media::request_kind::ACQUIRE_DEVICE,
        120,
        &yas_media::AcquireDevice {
            device_handle: microphone.device_handle,
            operation_id: [0x99; 16],
            kind: microphone.kind,
            lease_duration_ns: 60_000_000_000,
            formats: vec![microphone.formats[0].clone()],
            extensions: Extensions::default(),
        },
    )
    .await;
    let acquired = matching_result(
        &mut client,
        &codec,
        family::MEDIA,
        yas_media::request_kind::ACQUIRE_DEVICE,
        120,
    )
    .await;
    assert_eq!(acquired.status, Status::Ok);
    let acquired = yas_media::AcquireDeviceResult::decode(&acquired.body).unwrap();
    write_sensitive_event(
        &mut client,
        &codec,
        family::MEDIA,
        yas_media::event_kind::FRAME,
        &yas_media::MediaFrame {
            stream_handle: acquired.stream_handle,
            sequence: 1,
            capture_time: 0,
            presentation_time: 0,
            codec_version: acquired.selected_format.codec,
            flags: yas_wire::schema::media::FRAME_DISCARDABLE as u16,
            fragment_index: 0,
            fragment_count: 2,
            complete_len: STAGE_BYTES as u32,
            payload: vec![0x5a; STAGE_BYTES as usize / 2],
        },
    )
    .await;
    buffered = assert_family_admitted(&mut client, &codec, 121, buffered, "Media").await;

    write_request(
        &mut client,
        &codec,
        family::RELAY,
        yas_wire::schema::relay::request::CONNECT,
        122,
        &yas_wire::relay::Connect {
            route_handle: relay_route.route_handle,
            generation: relay_route.generation,
            initial_receive_credit: STAGE_BYTES,
            extensions: Extensions::default(),
        },
    )
    .await;
    let relay = matching_result(
        &mut client,
        &codec,
        family::RELAY,
        yas_wire::schema::relay::request::CONNECT,
        122,
    )
    .await;
    assert_eq!(relay.status, Status::Ok);
    let relay = yas_wire::relay::ConnectResult::decode(&relay.body).unwrap();
    buffered = assert_family_admitted(&mut client, &codec, 123, buffered, "Relay").await;

    // Every Ephemeral family still has to dispatch a valid operation while
    // all ten retaining owners are live. Idempotent UNWATCH is the smallest
    // state-neutral operation for the catalogue families.
    for (request_id, family_id, kind) in [
        (
            124,
            family::TERMINAL,
            yas_wire::schema::terminal::request::UNWATCH,
        ),
        (
            125,
            family::CLIENT,
            yas_wire::schema::client::request::UNWATCH,
        ),
        (
            126,
            family::SURFACE,
            yas_wire::schema::surface::request::UNWATCH,
        ),
        (127, family::DESKTOP, yas_desktop::request_kind::UNWATCH),
        (128, family::FONT, yas_wire::schema::font::request::UNWATCH),
        (129, family::GIT, yas_git_wire::request_kind::UNWATCH),
    ] {
        write_request(
            &mut client,
            &codec,
            family_id,
            kind,
            request_id,
            &Unwatch {
                subscription_id: u32::MAX,
            },
        )
        .await;
        assert_eq!(
            matching_result(&mut client, &codec, family_id, kind, request_id)
                .await
                .status,
            Status::Ok
        );
    }
    write_request(
        &mut client,
        &codec,
        family::EVENTS,
        yas_wire::schema::events::request::GET_CONFIG,
        130,
        &yas_events_wire::GetConfig {
            extensions: Extensions::default(),
        },
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::EVENTS,
            yas_wire::schema::events::request::GET_CONFIG,
            130,
        )
        .await
        .status,
        Status::Ok
    );
    write_request(
        &mut client,
        &codec,
        family::ENV,
        yas_wire::schema::env::request::GET,
        131,
        &yas_wire::env::Get {
            initial_receive_credit: STAGE_BYTES,
            extensions: Extensions::default(),
        },
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::ENV,
            yas_wire::schema::env::request::GET,
            131,
        )
        .await
        .status,
        Status::Ok
    );
    write_event(
        &mut client,
        &codec,
        family::TRANSFER,
        yas_wire::schema::transfer::event::CREDIT,
        &Credit {
            transfer_id: u32::MAX,
            cumulative_limit: 1,
        },
    )
    .await;
    write_request(
        &mut client,
        &codec,
        family::CORE,
        yas_wire::core::request_kind::PING,
        132,
        &Ping {
            sender_monotonic_ns: 1,
        },
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::CORE,
            yas_wire::core::request_kind::PING,
            132,
        )
        .await
        .status,
        Status::Ok
    );
    let after_ephemeral = diagnostics_while_events_are_live(&mut client, &codec, 133).await;
    assert_eq!(after_ephemeral.aggregate_receive_buffered, buffered);

    write_request(
        &mut client,
        &codec,
        family::KV,
        yas_wire::schema::kv::request::STAGE_VALUE,
        134,
        &yas_kv::StageValue {
            byte_len: LIMIT,
            content_hash: [0xaa; 32],
            extensions: Extensions::default(),
        },
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::KV,
            yas_wire::schema::kv::request::STAGE_VALUE,
            134,
        )
        .await
        .status,
        Status::ResourceExhausted,
        "the shared aggregate boundary rejects a valid new family reservation"
    );
    assert_eq!(
        diagnostics_while_events_are_live(&mut client, &codec, 135)
            .await
            .aggregate_receive_buffered,
        buffered,
        "a rejected owner must not leak aggregate receive credit"
    );

    let retaining_family_count = FAMILY_INVENTORY
        .iter()
        .filter(|entry| entry.retention == ReceiveRetention::Retaining)
        .count() as u64;
    assert_eq!(retaining_family_count, 10);
    assert!(
        buffered >= baseline.aggregate_receive_buffered + retaining_family_count * STAGE_BYTES,
        "all ten retaining family representatives must remain live together before queue pressure"
    );

    // Leave less than one deliberately large ordinary frame free in the main
    // session budget. The extra KV stage is only a deterministic pressure
    // source; the ten family representatives above remain live throughout.
    const QUEUE_HEADROOM: u64 = 4 * 1024;
    const QUEUE_FRAME_BYTES: usize = 16 * 1024;
    let main_receive_capacity = LIMIT - super::super::INBOUND_FLOW_HEADROOM.min(LIMIT);
    let semantic_buffered = buffered - baseline.aggregate_receive_buffered;
    let filler_bytes = main_receive_capacity
        .checked_sub(semantic_buffered + QUEUE_HEADROOM)
        .expect("family reservations leave room for qualification pressure");
    write_request(
        &mut client,
        &codec,
        family::KV,
        yas_wire::schema::kv::request::STAGE_VALUE,
        136,
        &yas_kv::StageValue {
            byte_len: filler_bytes,
            content_hash: [0xab; 32],
            extensions: Extensions::default(),
        },
    )
    .await;
    let filler = matching_result(
        &mut client,
        &codec,
        family::KV,
        yas_wire::schema::kv::request::STAGE_VALUE,
        136,
    )
    .await;
    assert_eq!(filler.status, Status::Ok);
    let filler = yas_kv::StageValueResult::decode(&filler.body).unwrap();
    buffered = assert_family_admitted(&mut client, &codec, 137, buffered, "pressure filler").await;
    assert_eq!(
        buffered - baseline.aggregate_receive_buffered,
        main_receive_capacity - QUEUE_HEADROOM,
        "pressure must leave a known sub-frame amount of receive budget"
    );

    // This is a valid, ordinary (non-Transfer) Request. Its optional unknown
    // extension makes the decoded frame larger than the remaining budget, so
    // the reliable reader must wait without blocking the independent sideband.
    write_request(
        &mut client,
        &codec,
        family::ENV,
        yas_wire::schema::env::request::GET,
        138,
        &yas_wire::env::Get {
            initial_receive_credit: STAGE_BYTES,
            extensions: Extensions(vec![yas_wire::codec::Extension {
                tag: 0x7fff,
                required: false,
                value: vec![0xbc; QUEUE_FRAME_BYTES],
            }]),
        },
    )
    .await;
    let mut media_header = FrameHeader::event(family::MEDIA, yas_media::event_kind::FRAME);
    media_header.sensitive = true;
    let media_completion = Frame {
        header: media_header,
        payload: yas_media::MediaFrame {
            stream_handle: acquired.stream_handle,
            sequence: 1,
            capture_time: 0,
            presentation_time: 0,
            codec_version: acquired.selected_format.codec,
            flags: yas_wire::schema::media::FRAME_DISCARDABLE as u16,
            fragment_index: 1,
            fragment_count: 2,
            complete_len: STAGE_BYTES as u32,
            payload: vec![0x5a; STAGE_BYTES as usize / 2],
        }
        .encode()
        .unwrap(),
    };
    let encoded_completion = codec
        .encode_datagram(&media_completion, 64 * 1024, DatagramContext::MediaFrame)
        .unwrap();
    assert!(encoded_completion.len() > QUEUE_HEADROOM as usize);
    {
        let reliable_wait = matching_result(
            &mut client,
            &codec,
            family::ENV,
            yas_wire::schema::env::request::GET,
            138,
        );
        tokio::pin!(reliable_wait);
        assert!(
            timeout(Duration::from_millis(25), &mut reliable_wait)
                .await
                .is_err(),
            "ordinary reliable admission must wait at the shared aggregate boundary"
        );

        let drops_before = datagram_budget_drops.load(std::sync::atomic::Ordering::Relaxed);
        yas_composite_transport::write_datagram(&mut side_peer, &encoded_completion, 64 * 1024)
            .await
            .unwrap();
        timeout(TEST_TIMEOUT, async {
            while datagram_budget_drops.load(std::sync::atomic::Ordering::Relaxed) == drops_before {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("composite budget drop is observed deterministically");
        assert!(
            timeout(Duration::from_millis(25), &mut reliable_wait)
                .await
                .is_err(),
            "a dropped optional datagram must not release or bypass reliable backpressure"
        );

        // Disconnecting the paired Channel owner releases one live semantic
        // reservation without requiring the blocked reliable reader. That
        // makes the ordinary ENV frame admissible and proves the stream
        // resumes.
        drop(channel_peer);
        let reliable_result = timeout(TEST_TIMEOUT, &mut reliable_wait)
            .await
            .expect("reliable frame resumes when a semantic owner releases");
        assert_eq!(reliable_result.status, Status::Ok);
        timeout(TEST_TIMEOUT, channel_peer_task)
            .await
            .unwrap()
            .unwrap();
    }
    buffered = assert_owner_released(&mut client, &codec, 139, buffered, "Channel").await;

    // The exact same optional frame is now admitted. Completing the Media
    // reassembly releases its full-frame reservation; no second budget drop
    // is allowed.
    let drops_after_pressure = datagram_budget_drops.load(std::sync::atomic::Ordering::Relaxed);
    yas_composite_transport::write_datagram(&mut side_peer, &encoded_completion, 64 * 1024)
        .await
        .unwrap();
    let media_buffered = timeout(TEST_TIMEOUT, async {
        let mut request_id = 140;
        loop {
            let current = diagnostics_while_events_are_live(&mut client, &codec, request_id).await;
            request_id += 1;
            if current.aggregate_receive_buffered < buffered {
                break current.aggregate_receive_buffered;
            }
            assert!(
                current.aggregate_receive_buffered <= buffered + encoded_completion.len() as u64,
                "the admitted datagram envelope is the only transient increase"
            );
            assert!(
                current.aggregate_receive_buffered <= current.aggregate_receive_limit,
                "retry admission remains inside the process-wide aggregate ceiling"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("accepted composite retry completes Media reassembly");
    assert_eq!(
        datagram_budget_drops.load(std::sync::atomic::Ordering::Relaxed),
        drops_after_pressure,
        "the retry has enough aggregate credit"
    );
    buffered = media_buffered;

    write_sensitive_transfer_event(
        &mut client,
        &codec,
        yas_wire::schema::transfer::event::RESET,
        &Reset {
            transfer_id: filler.transfer.transfer_id,
            status: Status::Cancelled.code(),
            detail: Vec::new(),
        },
    )
    .await;
    buffered = assert_owner_released(&mut client, &codec, 160, buffered, "pressure filler").await;

    let mut release_request = 1_000;
    for (owner, transfer_id) in &upload_transfers {
        write_sensitive_transfer_event(
            &mut client,
            &codec,
            yas_wire::schema::transfer::event::RESET,
            &Reset {
                transfer_id: *transfer_id,
                status: Status::Cancelled.code(),
                detail: Vec::new(),
            },
        )
        .await;
        buffered =
            assert_owner_released(&mut client, &codec, release_request, buffered, owner).await;
        release_request += 1;
    }

    write_sensitive_transfer_event(
        &mut client,
        &codec,
        yas_wire::schema::transfer::event::RESET,
        &Reset {
            transfer_id: process_stdin,
            status: Status::Cancelled.code(),
            detail: Vec::new(),
        },
    )
    .await;
    buffered =
        assert_owner_released(&mut client, &codec, release_request, buffered, "Process").await;
    release_request += 1;

    write_request(
        &mut client,
        &codec,
        family::NET,
        yas_wire::schema::net::request::CLOSE,
        release_request,
        &yas_net::Close {
            flow_handle: net.flow_handle,
            operation_id: [0xa1; 16],
            extensions: Extensions::default(),
        },
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::NET,
            yas_wire::schema::net::request::CLOSE,
            release_request,
        )
        .await
        .status,
        Status::Ok
    );
    release_request += 1;
    buffered = assert_owner_released(&mut client, &codec, release_request, buffered, "Net").await;
    release_request += 1;

    write_request(
        &mut client,
        &codec,
        family::RELAY,
        yas_wire::schema::relay::request::DISCONNECT,
        release_request,
        &yas_wire::relay::Disconnect {
            relay_handle: relay.relay_handle,
            reason: "qualification release".to_owned(),
        },
    )
    .await;
    assert_eq!(
        matching_result(
            &mut client,
            &codec,
            family::RELAY,
            yas_wire::schema::relay::request::DISCONNECT,
            release_request,
        )
        .await
        .status,
        Status::Ok
    );
    release_request += 1;
    buffered = assert_owner_released(&mut client, &codec, release_request, buffered, "Relay").await;
    assert_eq!(
        buffered, baseline.aggregate_receive_buffered,
        "all semantic owners drain back to the reliable envelope baseline"
    );

    let _ = (
        &mut side_peer,
        upload_transfers,
        process_stdin,
        net_transfer,
        connector_endpoint,
        channel_transfer,
        acquired,
        relay,
        buffered,
    );
    net_peer.abort();
    drop(client);
    timeout(TEST_TIMEOUT, server_task).await.unwrap().unwrap();
    assert_eq!(
        state.diagnostics.snapshot(),
        yas_wire::core::ServerDiagnostics {
            active_sessions: 0,
            relay_active: 0,
            relay_pending: 0,
            aggregate_receive_limit: 0,
            aggregate_receive_buffered: 0,
        },
        "all session-owned receive credit drains to zero"
    );
}
