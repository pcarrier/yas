//! `yas remote`: the Relay catalogue, edited where the server keeps it.
//!
//! The catalogue is the `remotes` key in the target server's KV store (see
//! `crates/server/src/relay.rs`), not a file in this machine's home directory.
//! That is what makes `yas --on dev remote add` mean anything: the edit lands
//! on the server that will dial the route, so administering a remote server no
//! longer requires a shell on it.
//!
//! Every mutation is read-modify-write under a revision precondition, so two
//! editors cannot silently overwrite each other — the loser retries against
//! what the winner wrote rather than clobbering it.

use yas_wire::{
    Class, Decode, Encode, Extensions,
    core::Status,
    family,
    kv::{self, Get, GetResult, Open, OpenResult, Precondition, Put, ValueSource},
};

use yas_webserver::config::{RemoteEntry, parse_remotes_full, serialize_remotes};

use crate::yas_native::NativeClient;

/// Where the catalogue lives. Must match `yas_server::relay::REMOTES_KEY`.
const REMOTES_KEY: &[u8] = b"remotes";

/// One read-modify-write against a contended key deserves one retry: the
/// second attempt sees the winner's document, and a third round of the same
/// race is a queue, not a conflict.
const ATTEMPTS: usize = 2;

fn wire_error(error: yas_wire::Error) -> String {
    format!("YAS wire error: {error}")
}

struct Catalogue {
    client: NativeClient,
    handle: u64,
}

impl Catalogue {
    async fn open(on: Option<&str>, hub: &str) -> Result<Self, String> {
        let mut client = NativeClient::connect(on, hub).await?;
        if !client.supports(family::KV, Class::Request, kv::request_kind::OPEN) {
            return Err(
                "this server does not offer the KV family, which is where remotes live".to_owned(),
            );
        }
        let result: OpenResult = client
            .request_typed(
                family::KV,
                kv::request_kind::OPEN,
                &Open {
                    prefix: Vec::new(),
                    extensions: Extensions::default(),
                },
                true,
            )
            .await?;
        Ok(Self {
            client,
            handle: result.namespace_handle,
        })
    }

    /// The stored document and the revision it was read at. A missing key is
    /// an empty catalogue, not an error: a server with no remotes has never
    /// written one.
    async fn read(&mut self) -> Result<(Vec<RemoteEntry>, Option<u64>), String> {
        let prefix = self
            .client
            .request_result(
                family::KV,
                kv::request_kind::GET,
                Get {
                    namespace_handle: self.handle,
                    relative_key: REMOTES_KEY.to_vec(),
                    initial_receive_credit: kv::MAX_VALUE_BYTES as u64,
                    extensions: Extensions::default(),
                }
                .encode()
                .map_err(wire_error)?,
                true,
            )
            .await?;
        if prefix.status == Status::NotFound {
            return Ok((Vec::new(), None));
        }
        if prefix.status != Status::Ok {
            return Err(format!("cannot read remotes: {:?}", prefix.status));
        }
        let result = GetResult::decode(&prefix.body).map_err(wire_error)?;
        let revision = result.modification_revision;
        let bytes = self
            .client
            .receive_inline_or_transfer(result.value, kv::MAX_VALUE_BYTES as u64)
            .await?;
        let text = String::from_utf8(bytes)
            .map_err(|_| "the stored remotes document is not UTF-8".to_owned())?;
        Ok((parse_remotes_full(&text), Some(revision)))
    }

    async fn write(
        &mut self,
        entries: &[RemoteEntry],
        revision: Option<u64>,
    ) -> Result<Status, String> {
        let prefix = self
            .client
            .request_result(
                family::KV,
                kv::request_kind::PUT,
                Put {
                    namespace_handle: self.handle,
                    operation_id: rand::random(),
                    durable: true,
                    relative_key: REMOTES_KEY.to_vec(),
                    precondition: match revision {
                        Some(revision) => Precondition::Revision(revision),
                        None => Precondition::Absent,
                    },
                    value: ValueSource::Inline(serialize_remotes(entries).into_bytes()),
                    extensions: Extensions::default(),
                }
                .encode()
                .map_err(wire_error)?,
                true,
            )
            .await?;
        Ok(prefix.status)
    }
}

/// Read the catalogue, apply `edit`, write it back.
///
/// `edit` returns the message to print, or an error to report — it runs again
/// on a losing race, so it must decide from the entries it is handed rather
/// than from anything it captured earlier.
pub(crate) async fn modify(
    on: Option<&str>,
    hub: &str,
    mut edit: impl FnMut(&mut Vec<RemoteEntry>) -> Result<String, String>,
) -> Result<(), String> {
    let mut catalogue = Catalogue::open(on, hub).await?;
    for attempt in 0..ATTEMPTS {
        let (mut entries, revision) = catalogue.read().await?;
        let message = edit(&mut entries)?;
        match catalogue.write(&entries, revision).await? {
            Status::Ok => {
                eprintln!("{message}");
                return Ok(());
            }
            Status::Conflict if attempt + 1 < ATTEMPTS => continue,
            status => return Err(format!("cannot write remotes: {status:?}")),
        }
    }
    Err("remotes changed under this edit twice; try again".to_owned())
}

/// The catalogue as stored, for `yas remote list`.
pub(crate) async fn read(on: Option<&str>, hub: &str) -> Result<Vec<RemoteEntry>, String> {
    let mut catalogue = Catalogue::open(on, hub).await?;
    Ok(catalogue.read().await?.0)
}
