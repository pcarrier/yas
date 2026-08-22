use std::{
    collections::VecDeque,
    sync::atomic::{AtomicU64, Ordering},
};

use wire::{
    Class, Encode, Extensions, Frame, FrameCodec, FrameHeader, FrameLimits,
    core::{
        FamilyDescriptor, Operation, ReceiveLimits, ResultPrefix, RuntimeState, ServerHello, Status,
    },
    extension::{AttemptContext, Runtime},
    family,
};
use yas_guest::{native_host, yas::wire};

static SEEN_EXTENSION: AtomicU64 = AtomicU64::new(0);

// A displayable error, because that is what the entry contract asks for now:
// an extension that fails has to be able to say why.
fn extension(client: yas_guest::Client) -> Result<(), String> {
    SEEN_EXTENSION.store(client.context().extension_handle, Ordering::Relaxed);
    Ok(())
}

yas_guest::entry!(extension);

struct Host {
    incoming: VecDeque<Vec<u8>>,
}

impl native_host::Host for Host {
    fn send(&mut self, _: &[u8]) -> i32 {
        0
    }

    fn recv(&mut self, buffer: &mut [u8]) -> i32 {
        let Some(packet) = self.incoming.front() else {
            return 0;
        };
        let len = packet.len();
        if len <= buffer.len() {
            buffer[..len].copy_from_slice(packet);
            self.incoming.pop_front();
        }
        len as i32
    }

    fn wait(&mut self, _: i64) -> i32 {
        if self.incoming.is_empty() { 2 } else { 1 }
    }

    fn clock(&mut self, _: i32) -> i64 {
        0
    }

    fn random(&mut self, destination: &mut [u8]) {
        destination.fill(4);
    }
}

fn operations() -> Vec<Operation> {
    vec![
        Operation {
            server_accepts: false,
            server_sends: true,
            class: Class::Event,
            kind: wire::extension::event_kind::ATTEMPT_CONTEXT,
        },
        Operation {
            server_accepts: true,
            server_sends: false,
            class: Class::Event,
            kind: wire::extension::event_kind::ATTEMPT_OUTPUT,
        },
    ]
}

fn family_descriptor(family_id: u16, version: u16) -> FamilyDescriptor {
    FamilyDescriptor {
        family_id,
        version,
        runtime_state: RuntimeState::Available,
        operations: if family_id == family::EXTENSION {
            operations()
        } else {
            Vec::new()
        },
        limits: match family_id {
            family::CHANNEL => wire::channel::Limits::HARD.to_extensions().unwrap(),
            family::EXTENSION => wire::extension::Limits::HARD.to_extensions().unwrap(),
            _ => Extensions::default(),
        },
    }
}

fn bootstrap_frames() -> VecDeque<Vec<u8>> {
    let hello = ServerHello {
        minor: 1,
        boot_id: [1; 16],
        session_id: [2; 16],
        receive: ReceiveLimits::recommended(0),
        server_monotonic_ns: 3,
        catalog_revision: 1,
        server_name: "test".into(),
        server_release: "1".into(),
        families: vec![
            family_descriptor(family::CORE, wire::core::VERSION),
            family_descriptor(family::TRANSFER, wire::transfer::VERSION),
            family_descriptor(family::CHANNEL, wire::channel::VERSION),
            family_descriptor(family::EXTENSION, wire::extension::VERSION),
        ],
        extensions: Extensions::default(),
    };
    let pre = FrameCodec::pre_hello();
    let hello = pre
        .encode_stream(&Frame {
            header: FrameHeader::result(family::CORE, wire::core::request_kind::HELLO, 1),
            payload: ResultPrefix {
                status: Status::Ok,
                detail: Extensions::default(),
                body: hello.encode().unwrap(),
            }
            .encode()
            .unwrap(),
        })
        .unwrap();
    let context = AttemptContext {
        extension_handle: 99,
        generation: 1,
        definition_revision: 1,
        attempt: 1,
        task_id: 1,
        flags: (wire::schema::extension::DEFINITION_ENABLED
            | wire::schema::extension::DEFINITION_DESIRED_RUNNING) as u16,
        runtime: Runtime::Wasmi,
        content_hash: [3; 32],
        name: "entry-test".into(),
        argv: Vec::new(),
        extensions: Extensions::default(),
    };
    let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
    let context = codec
        .encode_stream(&Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(
                    family::EXTENSION,
                    wire::extension::event_kind::ATTEMPT_CONTEXT,
                )
            },
            payload: context.encode().unwrap(),
        })
        .unwrap();
    [hello, context].into()
}

#[test]
fn exported_entry_bootstraps_native_yas_before_user_code() {
    let _guard = native_host::install(Host {
        incoming: bootstrap_frames(),
    });

    assert_eq!(__yas_guest_main(), 0);
    assert_eq!(SEEN_EXTENSION.load(Ordering::Relaxed), 99);
}
