//! Server-owned native YAS Font catalogue.

use std::sync::Arc;
use std::time::Duration;

use notify::Watcher;
use yas_fonts::{FontCatalog, FontExportPolicy};

pub(crate) const MAX_CONCURRENT_FETCHES: usize = 4;
pub(crate) const MAX_SCAN_DURATION: Duration = Duration::from_secs(30);
const RESCAN_DEBOUNCE: Duration = Duration::from_millis(250);

#[derive(Clone)]
pub(crate) struct Catalog {
    current: Arc<std::sync::RwLock<CatalogSnapshot>>,
    revisions: tokio::sync::watch::Receiver<u64>,
}

#[derive(Clone)]
pub(crate) struct CatalogSnapshot {
    pub(crate) revision: u64,
    pub(crate) catalog: Arc<FontCatalog>,
}

impl Catalog {
    pub(crate) fn current(&self) -> CatalogSnapshot {
        self.current
            .read()
            .expect("font catalogue lock poisoned")
            .clone()
    }

    pub(crate) fn subscribe(&self) -> tokio::sync::watch::Receiver<u64> {
        self.revisions.clone()
    }

    #[cfg(test)]
    pub(crate) fn fixed(catalog: Arc<FontCatalog>) -> Self {
        let (tx, revisions) = tokio::sync::watch::channel(1);
        let _ = tx;
        Self {
            current: Arc::new(std::sync::RwLock::new(CatalogSnapshot {
                revision: 1,
                catalog,
            })),
            revisions,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Service {
    catalog: Catalog,
    fetch_slots: Arc<tokio::sync::Semaphore>,
    advertised: bool,
}

impl Service {
    pub(crate) async fn from_env() -> Self {
        let advertised = !std::env::var("YAS_FONTS").is_ok_and(|value| value == "0");
        // Font enumeration can fingerprint the host and exporting bytes is a
        // distinct licensing/policy choice. Both decisions remain at the
        // authenticated server, never at the protocol adapter.
        let export_policy = if std::env::var("YAS_FONT_EXPORT").is_ok_and(|value| value == "1") {
            FontExportPolicy::Allow
        } else {
            FontExportPolicy::Deny
        };
        let catalog = if advertised {
            let mut scan = tokio::task::spawn_blocking(move || FontCatalog::scan(export_policy));
            match tokio::time::timeout(MAX_SCAN_DURATION, &mut scan).await {
                Ok(Ok(catalog)) => catalog,
                Ok(Err(error)) => {
                    eprintln!("yas-server: font catalogue task failed: {error}");
                    empty_catalog(export_policy)
                }
                Err(_) => {
                    scan.abort();
                    eprintln!(
                        "yas-server: font catalogue scan exceeded {}s; publishing an empty catalogue",
                        MAX_SCAN_DURATION.as_secs()
                    );
                    empty_catalog(export_policy)
                }
            }
        } else {
            empty_catalog(export_policy)
        };
        let catalog = Arc::new(catalog);
        let (revision_tx, revisions) = tokio::sync::watch::channel(1);
        let current = Arc::new(std::sync::RwLock::new(CatalogSnapshot {
            revision: 1,
            catalog: Arc::clone(&catalog),
        }));
        if advertised {
            let current = Arc::clone(&current);
            tokio::spawn(async move {
                let (event_tx, mut events) = tokio::sync::mpsc::unbounded_channel();
                let Some(mut _watcher) = font_watcher(event_tx.clone()) else {
                    return;
                };
                loop {
                    if events.recv().await.is_none() {
                        return;
                    }
                    tokio::time::sleep(RESCAN_DEBOUNCE).await;
                    while events.try_recv().is_ok() {}
                    let scan =
                        tokio::task::spawn_blocking(move || FontCatalog::scan(export_policy));
                    let Ok(Ok(next)) = tokio::time::timeout(MAX_SCAN_DURATION, scan).await else {
                        continue;
                    };
                    // Recreate the registrations after every event so a font
                    // directory which did not exist at startup replaces its
                    // nearest-parent watch with a recursive watch of itself.
                    if let Some(next_watcher) = font_watcher(event_tx.clone()) {
                        _watcher = next_watcher;
                    }
                    let next = Arc::new(next);
                    let mut guard = current.write().expect("font catalogue lock poisoned");
                    if guard.catalog.families() == next.families() {
                        continue;
                    }
                    guard.revision = guard.revision.saturating_add(1);
                    guard.catalog = next;
                    let revision = guard.revision;
                    drop(guard);
                    revision_tx.send_replace(revision);
                }
            });
        }
        Self {
            catalog: Catalog { current, revisions },
            fetch_slots: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_FETCHES)),
            advertised,
        }
    }

    pub(crate) fn catalog(&self) -> Option<Catalog> {
        self.advertised.then(|| self.catalog.clone())
    }

    pub(crate) fn fetch_slots(&self) -> Arc<tokio::sync::Semaphore> {
        Arc::clone(&self.fetch_slots)
    }

    #[cfg(test)]
    pub(crate) fn disabled_for_test() -> Self {
        Self {
            catalog: Catalog::fixed(Arc::new(empty_catalog(FontExportPolicy::Deny))),
            fetch_slots: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_FETCHES)),
            advertised: false,
        }
    }
}

fn font_watcher(
    events: tokio::sync::mpsc::UnboundedSender<()>,
) -> Option<notify::RecommendedWatcher> {
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if event.is_ok() {
            let _ = events.send(());
        }
    })
    .map_err(|error| eprintln!("yas-server: cannot watch font directories: {error}"))
    .ok()?;

    let roots: Vec<std::path::PathBuf> = yas_fonts::font_dirs()
        .into_iter()
        .map(std::path::PathBuf::from)
        .collect();
    #[cfg(unix)]
    let roots = {
        let mut roots = roots;
        roots.push("/etc/fonts".into());
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(std::path::PathBuf::from(home).join(".config/fontconfig"));
        }
        roots
    };

    let mut watched = std::collections::BTreeMap::new();
    for root in roots {
        let exact = root.is_dir();
        let mut path = root.as_path();
        while !path.exists() {
            let Some(parent) = path.parent() else {
                break;
            };
            path = parent;
        }
        // Missing system roots such as /Library/Fonts on Linux must not turn
        // into a watch of the whole filesystem.
        if path.parent().is_none() {
            continue;
        }
        watched
            .entry(path.to_path_buf())
            .and_modify(|recursive| *recursive |= exact)
            .or_insert(exact);
    }
    for (path, recursive) in watched {
        let mode = if recursive {
            notify::RecursiveMode::Recursive
        } else {
            notify::RecursiveMode::NonRecursive
        };
        if let Err(error) = watcher.watch(&path, mode) {
            eprintln!(
                "yas-server: cannot watch font directory {}: {error}",
                path.display()
            );
        }
    }
    Some(watcher)
}

fn empty_catalog(export_policy: FontExportPolicy) -> FontCatalog {
    FontCatalog::from_paths(export_policy, std::iter::empty::<&std::path::Path>())
}
