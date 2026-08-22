//! The native YAS Relay catalogue and connector.
//!
//! The catalogue lives in this instance's KV store, under the `remotes` key,
//! as the same `name = uri` document `roots` uses (a leading `#` disables an
//! entry). KV is why there is no bespoke transport for editing it: watching,
//! compare-and-swap and per-instance storage already exist there, so the
//! browser and the CLI edit remotes the same way they edit anything else, and
//! the store is the instance's rather than a file in a shared home directory.
//!
//! Relay's own state convention still publishes only boot-scoped opaque
//! handles, so a route *snapshot* carries no URI. The stored document does,
//! and any client that can read this KV store can read it — including the
//! passphrases in `share:` URIs and the identities in `ssh:` ones. That is a
//! deliberate consequence of putting the catalogue somewhere clients can edit
//! it: a client that can reach this store already holds full authority over
//! this server.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::watch;
use yas_webserver::config::parse_remotes_str;
use yas_wire::relay::{Availability, TransportHint};

pub(crate) const MAX_ROUTES: usize = 1024;
const MAX_ROUTE_NAME_BYTES: usize = u16::MAX as usize;
const MAX_ROUTE_URI_BYTES: usize = 16 * 1024;

/// Where the catalogue lives in this instance's KV store.
pub(crate) const REMOTES_KEY: &[u8] = b"remotes";

pub(crate) struct Service {
    enabled: bool,
    catalogue: Arc<RelayRouteCatalog>,
    reconcile_task: Option<tokio::task::JoinHandle<()>>,
}

/// Carry a pre-KV `yas.remotes` file into the store, once.
///
/// Only when the key is *absent*: a present-but-empty document means someone
/// removed their last remote, and re-importing a file they have stopped
/// editing would resurrect it every start. Absent means this store has never
/// held a catalogue, which is exactly the upgrade case.
///
/// The file is left where it is. Deleting a user's configuration to prove a
/// migration ran is not this function's business, and reading it twice is
/// harmless.
/// A one-shot operation id. The dedup window keys on it, and this runs once
/// per process start, so anything unique will do.
fn import_operation_id() -> [u8; 16] {
    let mut id = [0u8; 16];
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos() as u64);
    id[..8].copy_from_slice(&nanos.to_le_bytes());
    id[8..].copy_from_slice(&std::process::id().to_le_bytes().repeat(2)[..8]);
    id
}

fn import_legacy_remotes_file() {
    if crate::kv::native_get(REMOTES_KEY).is_some() {
        return;
    }
    let path = yas_webserver::config::remotes_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    let entries = yas_webserver::config::parse_remotes_full(&contents);
    if entries.is_empty() {
        return;
    }
    let document = yas_webserver::config::serialize_remotes(&entries).into_bytes();
    let content_hash = *blake3::hash(&document).as_bytes();
    let mutation = crate::kv::NativeMutation::Put {
        key: REMOTES_KEY.to_vec(),
        // Absent, not Any: two servers starting at once must not both import,
        // and the loser doing nothing is the right outcome.
        precondition: yas_wire::kv::Precondition::Absent,
        value: std::sync::Arc::new(document),
        content_hash,
    };
    match crate::kv::native_mutate(
        import_operation_id(),
        content_hash,
        true,
        Vec::new(),
        vec![mutation],
    ) {
        Ok(_) => eprintln!(
            "yas: imported {} remotes from {} into this instance's KV store",
            entries.len(),
            path.display()
        ),
        Err(error) => {
            eprintln!("yas: could not import {}: {error:?}", path.display())
        }
    }
}

/// The stored document, or an empty one when the key is absent — which is
/// what a server with no remotes configured looks like, not an error.
fn remotes_document(entries: &[crate::kv::NativeEntry]) -> String {
    entries
        .iter()
        .find(|entry| entry.key == REMOTES_KEY)
        .and_then(|entry| std::str::from_utf8(entry.value.as_slice()).ok())
        .map(str::to_owned)
        .unwrap_or_default()
}

impl Service {
    pub(crate) fn from_env() -> Self {
        let enabled = !std::env::var("YAS_RELAY").is_ok_and(|value| value == "0");
        if !enabled {
            return Self::disabled();
        }
        Self::from_kv()
    }

    /// Follow the `remotes` key for as long as this service lives.
    ///
    /// The initial snapshot and the change stream come from one `native_watch`
    /// call, so an edit landing between "read it" and "subscribe" cannot be
    /// missed — the gap a read-then-subscribe pair would leave is the whole
    /// reason KV hands both back together.
    fn from_kv() -> Self {
        let catalogue = Arc::new(RelayRouteCatalog::new());
        import_legacy_remotes_file();
        let watch = crate::kv::native_watch(REMOTES_KEY);
        reconcile_remotes(&catalogue, &remotes_document(&watch.entries));
        let task_catalogue = Arc::clone(&catalogue);
        let mut changes = watch.changes;
        let reconcile_task = tokio::spawn(async move {
            loop {
                match changes.recv().await {
                    Ok(change) => {
                        // One key in a store the whole product writes to:
                        // ignoring changes that are not ours is what keeps a
                        // busy KV from re-parsing the catalogue constantly.
                        let ours = change.records.iter().any(|record| match record {
                            crate::kv::NativeChangeRecord::Upsert { entry, .. } => {
                                entry.key == REMOTES_KEY
                            }
                            crate::kv::NativeChangeRecord::Remove { key, .. } => key == REMOTES_KEY,
                        });
                        if !ours {
                            continue;
                        }
                    }
                    // Lagged past the change this catalogue needed: re-read
                    // rather than carry on from an unknown state.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                }
                let entries = crate::kv::native_watch(REMOTES_KEY).entries;
                reconcile_remotes(&task_catalogue, &remotes_document(&entries));
            }
        });
        Self {
            enabled: true,
            catalogue,
            reconcile_task: Some(reconcile_task),
        }
    }

    fn disabled() -> Self {
        Self {
            enabled: false,
            catalogue: Arc::new(RelayRouteCatalog::new()),
            reconcile_task: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn disabled_for_test() -> Self {
        Self::disabled()
    }

    pub(crate) fn catalogue(&self) -> Option<Arc<RelayRouteCatalog>> {
        self.enabled.then(|| Arc::clone(&self.catalogue))
    }

    pub(crate) fn connector(&self) -> Arc<dyn RelayConnector> {
        Arc::new(|uri: String| async move { yas_proxy::connect_yas_upstream_split(&uri).await })
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        if let Some(task) = self.reconcile_task.take() {
            task.abort();
        }
    }
}

fn reconcile_remotes(catalogue: &RelayRouteCatalog, contents: &str) {
    let routes = parse_remotes_str(contents)
        .into_iter()
        .filter(|(name, uri)| !(name == "local" && uri == "local"))
        .filter_map(|(name, uri)| match normalize_yas_uri(&uri) {
            Some(uri) => Some((name, uri)),
            None => {
                // Route names are public topology. URI values can contain
                // credentials and must never enter diagnostics.
                eprintln!(
                    "yas-server: relay route '{name}' is not published over YAS: \
                     connector has no native YAS selector"
                );
                None
            }
        })
        .collect::<Vec<_>>();
    if let Err(error) = catalogue.replace_snapshot(routes) {
        eprintln!("yas-server: YAS relay catalogue update refused: {error}");
    }
}

fn normalize_yas_uri(uri: &str) -> Option<String> {
    let uri = uri.strip_prefix("proxy:").unwrap_or(uri);
    if uri == "local" {
        return Some(format!(
            "socket:{}",
            yas_webserver::config::default_yas_socket()
        ));
    }
    if let Some(name) = uri.strip_prefix("local:") {
        if !yas_webserver::config::valid_server_name(name) {
            return None;
        }
        return Some(format!(
            "socket:{}",
            yas_webserver::config::yas_socket_for_name(name)
        ));
    }
    ["socket:", "tcp:", "ssh:", "ws://", "wss://"]
        .iter()
        .any(|scheme| uri.starts_with(scheme))
        .then(|| uri.to_owned())
}

pub type RelayRead = Box<dyn AsyncRead + Unpin + Send + 'static>;
pub type RelayWrite = Box<dyn AsyncWrite + Unpin + Send + 'static>;
pub type RelayConnectResult = Result<(RelayRead, RelayWrite), String>;
pub type RelayConnectFuture = Pin<Box<dyn Future<Output = RelayConnectResult> + Send + 'static>>;

pub trait RelayConnector: Send + Sync + 'static {
    fn connect(&self, uri: String) -> RelayConnectFuture;
}

impl<F, Fut> RelayConnector for F
where
    F: Fn(String) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = RelayConnectResult> + Send + 'static,
{
    fn connect(&self, uri: String) -> RelayConnectFuture {
        Box::pin((self)(uri))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayRouteDescriptor {
    pub route_handle: u64,
    pub generation: u64,
    pub availability: Availability,
    pub transport_hint: TransportHint,
    pub is_default: bool,
    pub name: String,
    pub label: String,
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayRouteSnapshot {
    pub revision: u64,
    pub routes: Vec<RelayRouteDescriptor>,
}

#[derive(Clone)]
struct RouteSlot {
    handle: u64,
    generation: u64,
    uri: String,
    present: bool,
}

#[derive(Clone)]
struct CatalogInner {
    revision: u64,
    next_handle: u64,
    slots: BTreeMap<String, RouteSlot>,
    names_by_handle: HashMap<u64, String>,
}

pub struct RelayRouteCatalog {
    inner: StdMutex<CatalogInner>,
    revisions: watch::Sender<u64>,
}

impl Default for RelayRouteCatalog {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayRouteCatalog {
    pub fn new() -> Self {
        let (revisions, _) = watch::channel(1);
        Self {
            inner: StdMutex::new(CatalogInner {
                revision: 1,
                next_handle: 1,
                slots: BTreeMap::new(),
                names_by_handle: HashMap::new(),
            }),
            revisions,
        }
    }

    pub fn replace_snapshot<I, N, U>(&self, routes: I) -> Result<u64, RelayCatalogError>
    where
        I: IntoIterator<Item = (N, U)>,
        N: Into<String>,
        U: Into<String>,
    {
        let mut wanted = BTreeMap::new();
        for (name, uri) in routes {
            if wanted.len() >= MAX_ROUTES {
                return Err(RelayCatalogError::TooManyRoutes);
            }
            let name = name.into();
            let uri = uri.into();
            if name.is_empty() {
                return Err(RelayCatalogError::EmptyName);
            }
            if name.len() > MAX_ROUTE_NAME_BYTES {
                return Err(RelayCatalogError::NameTooLong);
            }
            if uri.is_empty() {
                return Err(RelayCatalogError::EmptyUri(name));
            }
            if uri.len() > MAX_ROUTE_URI_BYTES {
                return Err(RelayCatalogError::UriTooLong(name));
            }
            if wanted.insert(name.clone(), uri).is_some() {
                return Err(RelayCatalogError::DuplicateName(name));
            }
        }

        let mut guard = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let unchanged = guard.slots.iter().all(|(name, slot)| {
            !slot.present || wanted.get(name).is_some_and(|uri| uri == &slot.uri)
        }) && wanted.len()
            == guard.slots.values().filter(|slot| slot.present).count();
        if unchanged {
            return Ok(guard.revision);
        }

        let mut next = guard.clone();
        for (name, slot) in &mut next.slots {
            if slot.present && !wanted.contains_key(name) {
                slot.generation = slot
                    .generation
                    .checked_add(1)
                    .ok_or_else(|| RelayCatalogError::GenerationExhausted(name.clone()))?;
                slot.present = false;
            }
        }
        for (name, uri) in wanted {
            if let Some(slot) = next.slots.get_mut(&name) {
                if !slot.present || slot.uri != uri {
                    slot.generation = slot
                        .generation
                        .checked_add(1)
                        .ok_or_else(|| RelayCatalogError::GenerationExhausted(name.clone()))?;
                    slot.uri = uri;
                    slot.present = true;
                }
            } else {
                let handle = next.next_handle;
                next.next_handle = next
                    .next_handle
                    .checked_add(1)
                    .ok_or(RelayCatalogError::HandleExhausted)?;
                next.names_by_handle.insert(handle, name.clone());
                next.slots.insert(
                    name,
                    RouteSlot {
                        handle,
                        generation: 1,
                        uri,
                        present: true,
                    },
                );
            }
        }
        next.revision = next
            .revision
            .checked_add(1)
            .ok_or(RelayCatalogError::RevisionExhausted)?;
        let revision = next.revision;
        *guard = next;
        drop(guard);
        self.revisions.send_replace(revision);
        Ok(revision)
    }

    pub fn snapshot(&self) -> RelayRouteSnapshot {
        let guard = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let routes = guard
            .slots
            .iter()
            .filter(|(_, slot)| slot.present)
            .map(|(name, slot)| RelayRouteDescriptor {
                route_handle: slot.handle,
                generation: slot.generation,
                availability: Availability::Unknown,
                transport_hint: transport_hint(&slot.uri),
                is_default: false,
                name: name.clone(),
                label: name.clone(),
                description: String::new(),
            })
            .collect();
        RelayRouteSnapshot {
            revision: guard.revision,
            routes,
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.revisions.subscribe()
    }

    pub(crate) fn resolve(&self, handle: u64, generation: u64) -> Result<String, ResolveError> {
        let guard = self.inner.lock().unwrap_or_else(|error| error.into_inner());
        let name = guard
            .names_by_handle
            .get(&handle)
            .ok_or(ResolveError::NotFound)?;
        let slot = guard.slots.get(name).ok_or(ResolveError::NotFound)?;
        if slot.generation != generation {
            return Err(ResolveError::Stale);
        }
        if !slot.present {
            return Err(ResolveError::Unavailable);
        }
        Ok(slot.uri.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ResolveError {
    NotFound,
    Stale,
    Unavailable,
}

fn transport_hint(uri: &str) -> TransportHint {
    if uri.starts_with("socket:") {
        TransportHint::Local
    } else if uri.starts_with("ssh:") {
        TransportHint::Ssh
    } else if uri.starts_with("tcp:") || uri.starts_with("ws://") || uri.starts_with("wss://") {
        TransportHint::Tcp
    } else {
        TransportHint::Other
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RelayCatalogError {
    TooManyRoutes,
    EmptyName,
    NameTooLong,
    EmptyUri(String),
    UriTooLong(String),
    DuplicateName(String),
    HandleExhausted,
    GenerationExhausted(String),
    RevisionExhausted,
}

impl fmt::Display for RelayCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyRoutes => formatter.write_str("too many routes"),
            Self::EmptyName => formatter.write_str("empty route name"),
            Self::NameTooLong => formatter.write_str("route name is too long"),
            Self::EmptyUri(name) => write!(formatter, "route '{name}' has an empty connector"),
            Self::UriTooLong(name) => write!(formatter, "route '{name}' connector is too long"),
            Self::DuplicateName(name) => write!(formatter, "duplicate route '{name}'"),
            Self::HandleExhausted => formatter.write_str("route handle space exhausted"),
            Self::GenerationExhausted(name) => {
                write!(formatter, "route '{name}' generation exhausted")
            }
            Self::RevisionExhausted => formatter.write_str("route revision space exhausted"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_invalidates_the_observed_generation() {
        let catalogue = RelayRouteCatalog::new();
        catalogue
            .replace_snapshot([("work", "socket:/first")])
            .unwrap();
        let first = catalogue.snapshot().routes[0].clone();
        catalogue
            .replace_snapshot([("work", "socket:/second")])
            .unwrap();
        let second = catalogue.snapshot().routes[0].clone();
        assert_eq!(first.route_handle, second.route_handle);
        assert!(second.generation > first.generation);
        assert!(matches!(
            catalogue.resolve(first.route_handle, first.generation),
            Err(ResolveError::Stale)
        ));
        assert_eq!(
            catalogue
                .resolve(second.route_handle, second.generation)
                .unwrap(),
            "socket:/second"
        );
    }
}
