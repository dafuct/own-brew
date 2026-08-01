//! Process-wide application state.

use crate::brew::Brew;
use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::history::History;
use crate::model::detail::Info;
use crate::model::Outdated;
use crate::ops::Runner;
use crate::security;
use serde::Serialize;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A value that costs seconds of `brew` to produce, computed at most once
/// until something invalidates it.
///
/// The write lock is held across the load, so concurrent callers wait for one
/// load instead of each starting their own — the Updates tab and the Security
/// tab both want the vulnerability scan, and it must not run twice.
pub struct Cache<T> {
    slot: RwLock<Option<Arc<T>>>,
}

impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            slot: RwLock::new(None),
        }
    }
}

impl<T> Cache<T> {
    pub async fn get_or_load<F, Fut>(&self, load: F) -> Result<Arc<T>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        if let Some(value) = self.slot.read().await.as_ref() {
            return Ok(value.clone());
        }

        let mut slot = self.slot.write().await;
        // Another caller may have filled it while we waited for the lock.
        if let Some(value) = slot.as_ref() {
            return Ok(value.clone());
        }

        let value = Arc::new(load().await?);
        *slot = Some(value.clone());
        Ok(value)
    }

    /// Recompute unconditionally — what an explicit "Rescan" must do.
    pub async fn refresh<F, Fut>(&self, load: F) -> Result<Arc<T>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        let mut slot = self.slot.write().await;
        let value = Arc::new(load().await?);
        *slot = Some(value.clone());
        Ok(value)
    }

    pub async fn invalidate(&self) {
        *self.slot.write().await = None;
    }
}

pub struct App {
    /// `None` when Homebrew isn't installed. The app still starts, so it can
    /// explain how to fix that rather than refusing to open.
    brew: Option<Brew>,
    http: reqwest::Client,
    pub runner: Runner,
    catalog: RwLock<Option<Arc<Catalog>>>,
    /// Everything derived from the local Homebrew installation. Each costs a
    /// `brew` subprocess measured in seconds — `brew outdated` 5.5 s and
    /// `brew vulns` 4.7 s on the machine this was profiled on — and several
    /// views want the same answer, so they are computed once and shared.
    info: Cache<Info>,
    outdated: Cache<Outdated>,
    vulns: Cache<security::Report>,
    /// `None` only if the database could not be opened. History is valuable
    /// but must never stop the app from managing packages.
    history: Option<History>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub brew_installed: bool,
    pub brew_version: Option<String>,
    pub prefix: Option<String>,
}

impl App {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        let brew = match Brew::discover() {
            Ok(brew) => {
                tracing::info!(prefix = %brew.prefix().display(), "found Homebrew");
                Some(brew)
            }
            Err(e) => {
                tracing::warn!(error = %e, "Homebrew not found");
                None
            }
        };

        let history = match History::open(&data_dir.join("history.sqlite3")) {
            Ok(history) => Some(history),
            Err(e) => {
                tracing::error!(error = %e, "could not open the history database");
                None
            }
        };

        Self {
            brew,
            http: reqwest::Client::builder()
                .user_agent(concat!("own-brew/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("HTTP client construction cannot fail with these settings"),
            runner: Runner::new(),
            catalog: RwLock::new(None),
            info: Cache::default(),
            outdated: Cache::default(),
            vulns: Cache::default(),
            history,
        }
    }

    /// `brew info --json=v2 --installed`, shared by the Installed list, the
    /// upgrade impact assessment and the disk view.
    pub async fn info(&self) -> Result<Arc<Info>> {
        let brew = self.brew()?;
        self.info
            .get_or_load(|| async { brew.json(&["info", "--json=v2", "--installed"]).await })
            .await
    }

    pub async fn outdated(&self) -> Result<Arc<Outdated>> {
        let brew = self.brew()?;
        self.outdated
            .get_or_load(|| crate::state::outdated(brew))
            .await
    }

    /// The vulnerability scan. Both the Security tab and the Updates tab's
    /// impact assessment need it; before this cache existed, opening both ran
    /// `brew vulns` twice for the same answer.
    pub async fn vulns(&self) -> Result<Arc<security::Report>> {
        let brew = self.brew()?;
        self.vulns.get_or_load(|| security::scan(brew)).await
    }

    pub async fn rescan_vulns(&self) -> Result<Arc<security::Report>> {
        let brew = self.brew()?;
        self.vulns.refresh(|| security::scan(brew)).await
    }

    /// Forget everything derived from the local installation.
    ///
    /// Called when an operation finishes, because that is the only thing that
    /// can change what is installed from under us.
    pub async fn invalidate_local(&self) {
        self.info.invalidate().await;
        self.outdated.invalidate().await;
        self.vulns.invalidate().await;
    }

    pub fn history(&self) -> Result<&History> {
        self.history.as_ref().ok_or_else(|| {
            Error::Catalog("the history database is unavailable on this machine".to_owned())
        })
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    pub fn brew(&self) -> Result<&Brew> {
        self.brew.as_ref().ok_or(Error::BrewNotFound)
    }

    pub async fn environment(&self) -> Environment {
        match &self.brew {
            Some(brew) => Environment {
                brew_installed: true,
                brew_version: brew.version().await.ok(),
                prefix: Some(brew.prefix().display().to_string()),
            },
            None => Environment {
                brew_installed: false,
                brew_version: None,
                prefix: None,
            },
        }
    }

    /// The catalog, loading it on first use.
    ///
    /// The write lock is held across the load so that concurrent callers wait
    /// for one load rather than each starting their own.
    pub async fn catalog(&self) -> Result<Arc<Catalog>> {
        if let Some(catalog) = self.catalog.read().await.as_ref() {
            return Ok(catalog.clone());
        }

        let mut slot = self.catalog.write().await;
        if let Some(catalog) = slot.as_ref() {
            return Ok(catalog.clone());
        }

        let catalog = Arc::new(self.load_catalog().await?);
        *slot = Some(catalog.clone());
        Ok(catalog)
    }

    /// Discard the cached catalog and read it again.
    pub async fn reload_catalog(&self) -> Result<Arc<Catalog>> {
        let mut slot = self.catalog.write().await;
        let catalog = Arc::new(self.load_catalog().await?);
        *slot = Some(catalog.clone());
        Ok(catalog)
    }

    async fn load_catalog(&self) -> Result<Catalog> {
        let cache = self.brew.as_ref().and_then(Brew::cache_dir);
        let started = std::time::Instant::now();
        let catalog = Catalog::load(&self.http, cache).await?;
        let stats = catalog.stats();
        tracing::info!(
            formulae = stats.formulae,
            casks = stats.casks,
            source = ?stats.origin,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "catalog loaded"
        );
        Ok(catalog)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn a_value_is_computed_once_and_then_reused() {
        let cache: Cache<u32> = Cache::default();
        let calls = AtomicUsize::new(0);

        let load = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(7)
        };

        assert_eq!(*cache.get_or_load(load).await.unwrap(), 7);
        assert_eq!(*cache.get_or_load(load).await.unwrap(), 7);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the second call was not served from cache");
    }

    #[tokio::test]
    async fn concurrent_callers_share_one_load() {
        // The Security tab and the Updates tab both want the vulnerability
        // scan. Asking at the same time must not run `brew vulns` twice.
        let cache: Cache<u32> = Cache::default();
        let calls = AtomicUsize::new(0);

        let load = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            Ok(1)
        };

        let (a, b, c) = tokio::join!(
            cache.get_or_load(load),
            cache.get_or_load(load),
            cache.get_or_load(load),
        );

        assert!(a.is_ok() && b.is_ok() && c.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1, "the load was not single-flighted");
    }

    #[tokio::test]
    async fn invalidating_forces_the_next_read_to_recompute() {
        let cache: Cache<u32> = Cache::default();
        let calls = AtomicUsize::new(0);
        let load = || async {
            Ok(calls.fetch_add(1, Ordering::SeqCst) as u32)
        };

        assert_eq!(*cache.get_or_load(load).await.unwrap(), 0);
        cache.invalidate().await;
        assert_eq!(
            *cache.get_or_load(load).await.unwrap(),
            1,
            "an operation invalidated the cache but a stale value was served"
        );
    }

    #[tokio::test]
    async fn refresh_recomputes_even_when_a_value_is_cached() {
        // What the Security tab's Rescan button depends on.
        let cache: Cache<u32> = Cache::default();
        let calls = AtomicUsize::new(0);
        let load = || async { Ok(calls.fetch_add(1, Ordering::SeqCst) as u32) };

        assert_eq!(*cache.get_or_load(load).await.unwrap(), 0);
        assert_eq!(*cache.refresh(load).await.unwrap(), 1, "Rescan returned the cached report");
        assert_eq!(*cache.get_or_load(load).await.unwrap(), 1, "the refreshed value was not stored");
    }

    #[tokio::test]
    async fn a_failed_load_is_not_cached() {
        let cache: Cache<u32> = Cache::default();
        let calls = AtomicUsize::new(0);

        let failing = || async {
            calls.fetch_add(1, Ordering::SeqCst);
            Err(Error::Catalog("brew is having a moment".to_owned()))
        };
        assert!(cache.get_or_load(failing).await.is_err());

        let succeeding = || async { Ok(42) };
        assert_eq!(
            *cache.get_or_load(succeeding).await.unwrap(),
            42,
            "a transient failure poisoned the cache"
        );
    }
}
