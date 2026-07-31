//! Process-wide application state.

use crate::brew::Brew;
use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::history::History;
use crate::ops::Runner;
use serde::Serialize;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct App {
    /// `None` when Homebrew isn't installed. The app still starts, so it can
    /// explain how to fix that rather than refusing to open.
    brew: Option<Brew>,
    http: reqwest::Client,
    pub runner: Runner,
    catalog: RwLock<Option<Arc<Catalog>>>,
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
            history,
        }
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
