//! Where catalog data comes from.
//!
//! Homebrew already keeps the full catalog on disk as signed JWS envelopes
//! (`formula.jws.json`, `cask.jws.json`) and refreshes them itself. Reading
//! those is instant, works offline, and — crucially — describes exactly the
//! packages `brew` would install. Downloading our own copy is the fallback for
//! a machine whose cache hasn't been populated yet.

use crate::error::{Error, Result};
use crate::model::entry::{CaskEntry, Entry, FormulaEntry, Kind};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Read from Homebrew's own cache — no network involved.
    BrewCache,
    /// Downloaded from formulae.brew.sh.
    Network,
}

const API_BASE: &str = "https://formulae.brew.sh/api";

/// A JWS envelope: the catalog lives in `payload` as an embedded JSON string.
#[derive(Deserialize)]
struct Jws {
    payload: String,
}

pub struct Loader<'a> {
    pub http: &'a reqwest::Client,
    pub brew_cache: Option<PathBuf>,
}

impl Loader<'_> {
    pub async fn load(&self, kind: Kind) -> Result<(Vec<Entry>, Origin)> {
        if let Some(path) = self.cached_path(kind) {
            match self.read_brew_cache(kind, &path).await {
                Ok(entries) => return Ok((entries, Origin::BrewCache)),
                Err(e) => tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "Homebrew's catalog cache was unreadable; falling back to the network"
                ),
            }
        }
        Ok((self.download(kind).await?, Origin::Network))
    }

    fn cached_path(&self, kind: Kind) -> Option<PathBuf> {
        let file = match kind {
            Kind::Formula => "api/formula.jws.json",
            Kind::Cask => "api/cask.jws.json",
        };
        let path = self.brew_cache.as_ref()?.join(file);
        path.is_file().then_some(path)
    }

    async fn read_brew_cache(&self, kind: Kind, path: &Path) -> Result<Vec<Entry>> {
        let raw = tokio::fs::read_to_string(path).await?;
        // ~34 MB of JSON: parse off the async runtime so the UI stays responsive.
        tokio::task::spawn_blocking(move || {
            let envelope: Jws = serde_json::from_str(&raw).map_err(|source| Error::Parse {
                command: "catalog cache".to_owned(),
                source,
            })?;
            parse_entries(kind, &envelope.payload)
        })
        .await
        .map_err(|e| Error::Catalog(format!("catalog parsing task failed: {e}")))?
    }

    async fn download(&self, kind: Kind) -> Result<Vec<Entry>> {
        let url = match kind {
            Kind::Formula => format!("{API_BASE}/formula.json"),
            Kind::Cask => format!("{API_BASE}/cask.json"),
        };
        let body = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        tokio::task::spawn_blocking(move || parse_entries(kind, &body))
            .await
            .map_err(|e| Error::Catalog(format!("catalog parsing task failed: {e}")))?
    }
}

/// Parse a bare JSON array of packages into lean [`Entry`] records.
pub fn parse_entries(kind: Kind, json: &str) -> Result<Vec<Entry>> {
    let parse_error = |source| Error::Parse {
        command: format!("{} catalog", kind.as_str()),
        source,
    };
    match kind {
        Kind::Formula => Ok(serde_json::from_str::<Vec<FormulaEntry>>(json)
            .map_err(parse_error)?
            .into_iter()
            .map(Entry::from)
            .collect()),
        Kind::Cask => Ok(serde_json::from_str::<Vec<CaskEntry>>(json)
            .map_err(parse_error)?
            .into_iter()
            .map(Entry::from)
            .collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwraps_a_jws_envelope() {
        let envelope = serde_json::json!({
            "payload": r#"[{"name":"jq","versions":{"stable":"1.8.2"}}]"#,
            "signatures": [{"protected": "…", "signature": "…"}],
        })
        .to_string();
        let jws: Jws = serde_json::from_str(&envelope).unwrap();
        let entries = parse_entries(Kind::Formula, &jws.payload).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, "jq");
    }

    #[test]
    fn parses_a_cask_array() {
        let entries = parse_entries(
            Kind::Cask,
            r#"[{"token":"ghostty","name":["Ghostty"],"version":"1.3.1"}]"#,
        )
        .unwrap();
        assert_eq!(entries[0].kind, Kind::Cask);
        assert_eq!(entries[0].name, "Ghostty");
    }

    #[test]
    fn malformed_json_reports_which_catalog_failed() {
        let err = parse_entries(Kind::Cask, "{not json").unwrap_err();
        assert_eq!(err.kind(), "parse");
        assert!(err.to_string().contains("cask catalog"));
    }

    /// The real cache, when this machine has one. Guards against upstream
    /// changing the envelope shape underneath us.
    #[tokio::test]
    async fn reads_the_real_homebrew_cache_when_present() {
        let Some(cache) = crate::brew::Brew::discover()
            .ok()
            .and_then(|b| b.cache_dir())
        else {
            return;
        };
        let loader = Loader {
            http: &reqwest::Client::new(),
            brew_cache: Some(cache),
        };
        let Some(path) = loader.cached_path(Kind::Cask) else {
            return;
        };
        let entries = loader
            .read_brew_cache(Kind::Cask, &path)
            .await
            .expect("real cache parses");
        assert!(
            entries.len() > 1000,
            "expected thousands of casks, got {}",
            entries.len()
        );
        assert!(entries.iter().all(|e| !e.id.is_empty()));
    }
}
