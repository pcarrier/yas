//! Server-owned native YAS Font catalogue.

use std::sync::Arc;
use std::time::Duration;

use yas_fonts::{FontCatalog, FontExportPolicy};

pub(crate) const MAX_CONCURRENT_FETCHES: usize = 4;
pub(crate) const MAX_SCAN_DURATION: Duration = Duration::from_secs(30);

#[derive(Clone)]
pub(crate) struct Service {
    catalog: Arc<FontCatalog>,
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
        Self {
            catalog: Arc::new(catalog),
            fetch_slots: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_FETCHES)),
            advertised,
        }
    }

    pub(crate) fn catalog(&self) -> Option<Arc<FontCatalog>> {
        self.advertised.then(|| Arc::clone(&self.catalog))
    }

    pub(crate) fn fetch_slots(&self) -> Arc<tokio::sync::Semaphore> {
        Arc::clone(&self.fetch_slots)
    }

    #[cfg(test)]
    pub(crate) fn disabled_for_test() -> Self {
        Self {
            catalog: Arc::new(empty_catalog(FontExportPolicy::Deny)),
            fetch_slots: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_FETCHES)),
            advertised: false,
        }
    }
}

fn empty_catalog(export_policy: FontExportPolicy) -> FontCatalog {
    FontCatalog::from_paths(export_policy, std::iter::empty::<&std::path::Path>())
}
