use alloc::{collections::VecDeque, rc::Rc, string::String, vec, vec::Vec};
use std::cell::RefCell;

use yas_wire::{
    Class, Encode, Extensions, Frame, FrameCodec, FrameHeader, FrameLimits,
    core::{
        FamilyDescriptor, Operation, ReceiveLimits, ResultPrefix, RuntimeState, ServerHello, Status,
    },
    extension::{AttemptContext, Runtime, event_kind},
    family,
};

use crate::{native_host, yas::Client};

#[derive(Default)]
pub(crate) struct HostState {
    pub(crate) incoming: VecDeque<Vec<u8>>,
    pub(crate) sent: Vec<Vec<u8>>,
    pub(crate) received: usize,
    pub(crate) sent_after_receives: Vec<usize>,
    /// Test-only packets made readable after the indexed outbound packet.
    pub(crate) responses_after_send: VecDeque<(usize, Vec<Vec<u8>>)>,
    /// Test-only host sends which fail as if the endpoint had closed.
    pub(crate) fail_sends: usize,
}

struct MockHost(Rc<RefCell<HostState>>);

impl native_host::Host for MockHost {
    fn send(&mut self, packet: &[u8]) -> i32 {
        let mut state = self.0.borrow_mut();
        if state.fail_sends > 0 {
            state.fail_sends -= 1;
            return -1;
        }
        let received = state.received;
        state.sent.push(packet.to_vec());
        state.sent_after_receives.push(received);
        let sent = state.sent.len();
        while state
            .responses_after_send
            .front()
            .is_some_and(|(after, _)| *after == sent)
        {
            let (_, responses) = state
                .responses_after_send
                .pop_front()
                .expect("checked scheduled test response");
            state.incoming.extend(responses);
        }
        0
    }

    fn recv(&mut self, buffer: &mut [u8]) -> i32 {
        let mut state = self.0.borrow_mut();
        let Some(packet) = state.incoming.front() else {
            return 0;
        };
        let length = packet.len();
        if length <= buffer.len() {
            buffer[..length].copy_from_slice(packet);
            state.incoming.pop_front();
            state.received += 1;
        }
        length as i32
    }

    fn wait(&mut self, _deadline: i64) -> i32 {
        i32::from(!self.0.borrow().incoming.is_empty())
    }

    fn clock(&mut self, _kind: i32) -> i64 {
        0
    }

    fn random(&mut self, destination: &mut [u8]) {
        destination.fill(7);
    }
}

fn server_hello() -> ServerHello {
    ServerHello {
        minor: 1,
        boot_id: [1; 16],
        session_id: [2; 16],
        receive: ReceiveLimits::recommended(0),
        server_monotonic_ns: 3,
        catalog_revision: 1,
        server_name: String::from("test"),
        server_release: String::from("1"),
        families: vec![
            FamilyDescriptor {
                family_id: family::CORE,
                version: yas_wire::core::VERSION,
                runtime_state: RuntimeState::Available,
                operations: Vec::new(),
                limits: Extensions::default(),
            },
            FamilyDescriptor {
                family_id: family::TRANSFER,
                version: yas_wire::transfer::VERSION,
                runtime_state: RuntimeState::Available,
                operations: Vec::new(),
                limits: Extensions::default(),
            },
            FamilyDescriptor {
                family_id: family::TERMINAL,
                version: yas_wire::terminal::VERSION,
                runtime_state: RuntimeState::Available,
                operations: vec![
                    Operation {
                        server_accepts: true,
                        server_sends: false,
                        class: Class::Request,
                        kind: yas_wire::terminal::request_kind::CWD,
                    },
                    Operation {
                        server_accepts: true,
                        server_sends: false,
                        class: Class::Request,
                        kind: yas_wire::terminal::request_kind::OUTPUT,
                    },
                    Operation {
                        server_accepts: true,
                        server_sends: false,
                        class: Class::Request,
                        kind: yas_wire::terminal::request_kind::WAIT,
                    },
                ],
                limits: yas_wire::terminal::Limits::HARD.to_extensions().unwrap(),
            },
            FamilyDescriptor {
                family_id: family::NET,
                version: yas_wire::net::VERSION,
                runtime_state: RuntimeState::Available,
                operations: vec![
                    Operation {
                        server_accepts: true,
                        server_sends: false,
                        class: Class::Request,
                        kind: yas_wire::net::request_kind::OPEN,
                    },
                    Operation {
                        server_accepts: true,
                        server_sends: false,
                        class: Class::Request,
                        kind: yas_wire::net::request_kind::CLOSE,
                    },
                ],
                limits: yas_wire::net::Limits::HARD.to_extensions().unwrap(),
            },
            FamilyDescriptor {
                family_id: family::CHANNEL,
                version: yas_wire::channel::VERSION,
                runtime_state: RuntimeState::Available,
                operations: Vec::new(),
                limits: yas_wire::channel::Limits::HARD.to_extensions().unwrap(),
            },
            FamilyDescriptor {
                family_id: family::EXTENSION,
                version: yas_wire::extension::VERSION,
                runtime_state: RuntimeState::Available,
                operations: vec![
                    Operation {
                        server_accepts: false,
                        server_sends: true,
                        class: Class::Event,
                        kind: event_kind::ATTEMPT_CONTEXT,
                    },
                    Operation {
                        server_accepts: true,
                        server_sends: false,
                        class: Class::Event,
                        kind: event_kind::ATTEMPT_OUTPUT,
                    },
                    Operation {
                        server_accepts: true,
                        server_sends: false,
                        class: Class::Request,
                        kind: yas_wire::extension::request_kind::REGISTER_COMMAND,
                    },
                ],
                limits: yas_wire::extension::Limits::HARD.to_extensions().unwrap(),
            },
            FamilyDescriptor {
                family_id: family::ENV,
                version: yas_wire::env::VERSION,
                runtime_state: RuntimeState::Available,
                operations: vec![Operation {
                    server_accepts: true,
                    server_sends: false,
                    class: Class::Request,
                    kind: yas_wire::env::request_kind::GET,
                }],
                limits: yas_wire::env::Limits::HARD.to_extensions().unwrap(),
            },
        ],
        extensions: Extensions::default(),
    }
}

fn attempt_context() -> AttemptContext {
    AttemptContext {
        extension_handle: 21,
        generation: 22,
        definition_revision: 23,
        attempt: 24,
        task_id: 25,
        flags: (yas_wire::schema::extension::DEFINITION_ENABLED
            | yas_wire::schema::extension::DEFINITION_DESIRED_RUNNING
            | yas_wire::schema::extension::DEFINITION_PERSISTENT
            | yas_wire::schema::extension::DEFINITION_DETACHED) as u16,
        runtime: Runtime::Wasmi,
        content_hash: [4; 32],
        name: String::from("stream-test"),
        argv: Vec::new(),
        extensions: Extensions::default(),
    }
}

pub(crate) fn bootstrap_client() -> (Client, Rc<RefCell<HostState>>, native_host::Guard) {
    let hello = ResultPrefix {
        status: Status::Ok,
        detail: Extensions::default(),
        body: server_hello().encode().unwrap(),
    };
    let pre_hello = FrameCodec::pre_hello();
    let hello_frame = pre_hello
        .encode_stream(&Frame {
            header: FrameHeader::result(family::CORE, yas_wire::core::request_kind::HELLO, 1),
            payload: hello.encode().unwrap(),
        })
        .unwrap();
    let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
    let context_frame = codec
        .encode_stream(&Frame {
            header: FrameHeader {
                sensitive: true,
                ..FrameHeader::event(family::EXTENSION, event_kind::ATTEMPT_CONTEXT)
            },
            payload: attempt_context().encode().unwrap(),
        })
        .unwrap();
    let state = Rc::new(RefCell::new(HostState {
        incoming: [hello_frame, context_frame].into(),
        sent: Vec::new(),
        received: 0,
        sent_after_receives: Vec::new(),
        responses_after_send: VecDeque::new(),
        fail_sends: 0,
    }));
    let guard = native_host::install(MockHost(state.clone()));
    let client = Client::bootstrap().unwrap();
    let mut host_state = state.borrow_mut();
    host_state.sent.clear();
    host_state.received = 0;
    host_state.sent_after_receives.clear();
    host_state.responses_after_send.clear();
    drop(host_state);
    (client, state, guard)
}

pub(crate) fn pending_headroom_burst() -> Vec<Vec<u8>> {
    let codec = FrameCodec::new(FrameLimits::recommended(), []).unwrap();
    (0..25)
        .map(|_| 512 * 1024)
        .map(|length| {
            codec
                .encode_stream(&Frame {
                    header: FrameHeader::event(family::EXTENSION, 0x7fff),
                    payload: vec![0; length],
                })
                .unwrap()
        })
        .collect()
}
