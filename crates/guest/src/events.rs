//! Native Events journal configuration, dump, stream, and recording helpers.

use alloc::vec::Vec;
use core::fmt;

use yas_wire::{Class, Decode, Encode, Extensions, Frame, events as wire, family};

use crate::{
    transfer,
    yas::{Client, Error as ClientError},
};

pub const DEFAULT_DUMP_WINDOW: u64 = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    Transfer(transfer::Error),
    FeatureMissing,
    LimitExceeded { declared: u64, maximum: u64 },
    HashMismatch,
    Protocol(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Wire(error) => write!(formatter, "invalid native Events value: {error}"),
            Self::Transfer(error) => write!(formatter, "native Events Transfer failed: {error}"),
            Self::FeatureMissing => formatter.write_str("native Events operation is unavailable"),
            Self::LimitExceeded { declared, maximum } => write!(
                formatter,
                "native Events dump declares {declared} bytes; limit is {maximum}"
            ),
            Self::HashMismatch => formatter.write_str("native Events dump hash mismatch"),
            Self::Protocol(detail) => write!(formatter, "native Events protocol error: {detail}"),
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
pub enum StreamEvent {
    Records(wire::EventBatch),
    Gap(wire::Gap),
    Stopped(wire::StreamStopped),
}

pub struct Stream {
    handle: u64,
    next_sequence: u64,
    stopped: bool,
}

impl Stream {
    pub fn handle(&self) -> u64 {
        self.handle
    }

    pub fn next_sequence(&self) -> u64 {
        self.next_sequence
    }

    pub fn owns_frame(&self, frame: &Frame) -> bool {
        if self.stopped
            || frame.header.class != Class::Event
            || frame.header.family != family::EVENTS
        {
            return false;
        }
        match frame.header.kind {
            wire::event_kind::RECORD => wire::RecordEvent::decode(&frame.payload)
                .is_ok_and(|event| event.stream_handle == self.handle),
            wire::event_kind::GAP => wire::Gap::decode(&frame.payload)
                .is_ok_and(|event| event.stream_handle == self.handle),
            wire::event_kind::STREAM_STOPPED => wire::StreamStopped::decode(&frame.payload)
                .is_ok_and(|event| event.stream_handle == self.handle),
            _ => false,
        }
    }

    pub fn offer_frame(&mut self, frame: &Frame) -> Result<Option<StreamEvent>, Error> {
        if self.stopped
            || frame.header.class != Class::Event
            || frame.header.family != family::EVENTS
        {
            return Ok(None);
        }
        match frame.header.kind {
            wire::event_kind::RECORD => {
                let event = wire::RecordEvent::decode(&frame.payload)?;
                if event.stream_handle != self.handle {
                    return Ok(None);
                }
                if event.batch.first_sequence != self.next_sequence {
                    return Err(Error::Protocol("noncontiguous Events stream batch"));
                }
                self.next_sequence = self
                    .next_sequence
                    .checked_add(event.batch.records.len() as u64)
                    .ok_or(Error::Protocol("Events stream sequence overflow"))?;
                Ok(Some(StreamEvent::Records(event.batch)))
            }
            wire::event_kind::GAP => {
                let gap = wire::Gap::decode(&frame.payload)?;
                if gap.stream_handle != self.handle {
                    return Ok(None);
                }
                if gap.first_available_sequence < self.next_sequence
                    || gap
                        .first_available_sequence
                        .saturating_sub(self.next_sequence)
                        != gap.lost
                {
                    return Err(Error::Protocol("invalid Events stream gap"));
                }
                self.next_sequence = gap.first_available_sequence;
                Ok(Some(StreamEvent::Gap(gap)))
            }
            wire::event_kind::STREAM_STOPPED => {
                let stopped = wire::StreamStopped::decode(&frame.payload)?;
                if stopped.stream_handle != self.handle {
                    return Ok(None);
                }
                self.stopped = true;
                Ok(Some(StreamEvent::Stopped(stopped)))
            }
            _ => Ok(None),
        }
    }

    pub fn stop(&mut self, client: &mut Client) -> Result<(), Error> {
        if self.stopped {
            return Ok(());
        }
        let operation_id = operation_id(client)?;
        client.request(
            family::EVENTS,
            wire::request_kind::STOP_STREAM,
            wire::StopStream {
                stream_handle: self.handle,
                operation_id,
                extensions: Extensions::default(),
            }
            .encode()?,
            true,
        )?;
        self.stopped = true;
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

impl Client {
    pub fn get_events_config(&mut self) -> Result<wire::Config, Error> {
        if !self.supports(
            family::EVENTS,
            Class::Request,
            wire::request_kind::GET_CONFIG,
        ) {
            return Err(Error::FeatureMissing);
        }
        self.request_typed(
            family::EVENTS,
            wire::request_kind::GET_CONFIG,
            &wire::GetConfig {
                extensions: Extensions::default(),
            },
            true,
        )
        .map_err(Into::into)
    }

    pub fn set_events_config(
        &mut self,
        expected_revision: u64,
        capacity: u64,
        activations: wire::ActivationSet,
    ) -> Result<wire::Config, Error> {
        self.request_typed(
            family::EVENTS,
            wire::request_kind::SET_CONFIG,
            &wire::SetConfig {
                operation_id: operation_id(self)?,
                expected_revision,
                capacity,
                activations,
                extensions: Extensions::default(),
            },
            true,
        )
        .map_err(Into::into)
    }

    pub fn dump_events(&mut self, maximum_bytes: u64) -> Result<Vec<u8>, Error> {
        if maximum_bytes == 0 {
            return Err(Error::LimitExceeded {
                declared: 0,
                maximum: 0,
            });
        }
        let mut receive_lease = self.receive_credit_up_to(maximum_bytes)?;
        let initial_receive_credit = receive_lease.bytes();
        let result: wire::DumpResult = self.request_typed_with_receive_lease(
            family::EVENTS,
            wire::request_kind::DUMP,
            &wire::Dump {
                initial_receive_credit,
                extensions: Extensions::default(),
            },
            true,
            &mut receive_lease,
        )?;
        let bytes = transfer::receive_byte_transfer_with_lease(
            self,
            &result.descriptor,
            Some(result.byte_len),
            initial_receive_credit,
            receive_lease,
        )?;
        if blake3::hash(&bytes).as_bytes() != &result.content_hash {
            return Err(Error::HashMismatch);
        }
        Ok(bytes)
    }

    pub fn start_events_stream(
        &mut self,
        history: bool,
        start_sequence: u64,
        max_batch_bytes: u32,
    ) -> Result<Stream, Error> {
        let started: wire::StreamStarted = self.request_typed(
            family::EVENTS,
            wire::request_kind::START_STREAM,
            &wire::StartStream {
                operation_id: operation_id(self)?,
                history,
                start_sequence,
                max_batch_bytes,
                extensions: Extensions::default(),
            },
            true,
        )?;
        Ok(Stream {
            handle: started.stream_handle,
            next_sequence: started.first_sequence,
            stopped: false,
        })
    }

    pub fn start_events_recording(
        &mut self,
        history: bool,
        append: bool,
        path: Vec<u8>,
    ) -> Result<wire::RecordingInfo, Error> {
        self.request_typed(
            family::EVENTS,
            wire::request_kind::START_RECORDING,
            &wire::StartRecording {
                operation_id: operation_id(self)?,
                history,
                append,
                path,
                extensions: Extensions::default(),
            },
            true,
        )
        .map_err(Into::into)
    }

    pub fn stop_events_recording(
        &mut self,
        recording_handle: u64,
    ) -> Result<wire::RecordingInfo, Error> {
        self.request_typed(
            family::EVENTS,
            wire::request_kind::STOP_RECORDING,
            &wire::StopRecording {
                recording_handle,
                operation_id: operation_id(self)?,
                extensions: Extensions::default(),
            },
            true,
        )
        .map_err(Into::into)
    }

    pub fn list_events_recordings(&mut self) -> Result<Vec<wire::RecordingInfo>, Error> {
        let list: wire::RecordingList = self.request_typed(
            family::EVENTS,
            wire::request_kind::LIST_RECORDINGS,
            &wire::ListRecordings {
                extensions: Extensions::default(),
            },
            true,
        )?;
        Ok(list.recordings)
    }
}
