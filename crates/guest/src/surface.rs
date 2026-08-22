//! Native Surface catalogue and application-endpoint helpers.

use alloc::{string::String, vec::Vec};
use core::fmt;

use yas_wire::{
    Class, Decode, Encode, Extensions, Frame, family,
    state::{
        Cursor, Phase, RecordKind, StateAck, StateEvent, Unwatch, Watch as StateWatch, WatchResult,
    },
    surface as wire,
};

use crate::{
    receive::{DEFAULT_STATE_WINDOW as SHARED_STATE_WINDOW, Lease as ReceiveLease},
    yas::{Client, Error as ClientError},
};

pub const DEFAULT_STATE_WINDOW: u64 = SHARED_STATE_WINDOW;

#[derive(Debug)]
pub enum Error {
    Client(ClientError),
    Wire(yas_wire::Error),
    FeatureMissing,
    CounterOverflow,
    Protocol(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Client(error) => write!(formatter, "guest client error: {error}"),
            Self::Wire(error) => write!(formatter, "invalid native Surface value: {error}"),
            Self::FeatureMissing => formatter.write_str("native Surface operation is unavailable"),
            Self::CounterOverflow => formatter.write_str("native Surface credit overflow"),
            Self::Protocol(detail) => write!(formatter, "native Surface protocol error: {detail}"),
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StateChange {
    Upsert(wire::SurfaceRecord),
    Patch(wire::SurfacePatch),
    Remove(wire::RemovedSurface),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateUpdate {
    pub phase: Phase,
    pub from_revision: u64,
    pub to_revision: u64,
    pub changes: Vec<StateChange>,
}

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
            || frame.header.family != family::SURFACE
            || frame.header.kind != wire::event_kind::STATE
        {
            return Ok(None);
        }
        let event = StateEvent::decode(&frame.payload)?;
        if event.subscription_id != self.subscription_id {
            return Ok(None);
        }
        let mut changes = Vec::with_capacity(event.records.len());
        for record in &event.records {
            match record.kind {
                RecordKind::Add | RecordKind::Replace => {
                    changes.push(StateChange::Upsert(wire::surface_from_state_record(
                        record,
                    )?));
                }
                RecordKind::Patch => {
                    changes.push(StateChange::Patch(wire::SurfacePatch::decode(
                        &record.body,
                    )?));
                }
                RecordKind::Remove => {
                    changes.push(StateChange::Remove(wire::RemovedSurface::decode(
                        &record.body,
                    )?));
                }
                RecordKind::Family(_) => {
                    return Err(Error::Protocol("unexpected Surface state record kind"));
                }
            }
        }
        self.cumulative_credit = self
            .cumulative_credit
            .checked_add(frame.payload.len() as u64)
            .ok_or(Error::CounterOverflow)?;
        client.send_typed_event(
            family::SURFACE,
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
            family::SURFACE,
            wire::request_kind::UNWATCH,
            Unwatch {
                subscription_id: self.subscription_id,
            }
            .encode()?,
            false,
        )?;
        self.closed = true;
        self.receive_lease.release();
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppEndpoint {
    pub app_handle: u64,
    pub expires_server_ns: u64,
    pub environment: Vec<wire::EnvironmentOverride>,
}

impl Client {
    pub fn watch_surfaces(&mut self, resume: Option<Cursor>) -> Result<Watch, Error> {
        if !self.supports(family::SURFACE, Class::Request, wire::request_kind::WATCH) {
            return Err(Error::FeatureMissing);
        }
        let mut receive_lease = self.receive_credit_exact(DEFAULT_STATE_WINDOW)?;
        let initial_credit = receive_lease.bytes();
        let result: WatchResult = self.request_typed_with_receive_lease(
            family::SURFACE,
            wire::request_kind::WATCH,
            &StateWatch {
                initial_credit,
                resume,
                extensions: Extensions::default(),
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

    pub fn create_app_endpoint(&mut self, application_id: String) -> Result<AppEndpoint, Error> {
        if !self.supports(
            family::SURFACE,
            Class::Request,
            wire::request_kind::CREATE_APP_ENDPOINT,
        ) {
            return Err(Error::FeatureMissing);
        }
        let mut operation_id = [0; 16];
        self.random(&mut operation_id)?;
        let result: wire::CreateAppEndpointResult = self.request_typed(
            family::SURFACE,
            wire::request_kind::CREATE_APP_ENDPOINT,
            &wire::CreateAppEndpoint {
                operation_id,
                application_id,
                extensions: Extensions::default(),
            },
            true,
        )?;
        Ok(AppEndpoint {
            app_handle: result.app_handle,
            expires_server_ns: result.expires_server_ns,
            environment: result.environment,
        })
    }

    pub fn release_app_endpoint(&mut self, app_handle: u64) -> Result<(), Error> {
        if !self.supports(
            family::SURFACE,
            Class::Request,
            wire::request_kind::RELEASE_APP_ENDPOINT,
        ) {
            return Err(Error::FeatureMissing);
        }
        let mut operation_id = [0; 16];
        self.random(&mut operation_id)?;
        self.request(
            family::SURFACE,
            wire::request_kind::RELEASE_APP_ENDPOINT,
            wire::ReleaseAppEndpoint {
                app_handle,
                operation_id,
                extensions: Extensions::default(),
            }
            .encode()?,
            true,
        )?;
        Ok(())
    }
}
