//! Every version Homebrew ever published for a formula.
//!
//! Bottles live in a GitHub Container Registry repository per formula, with one
//! tag per published version. That tag list is the closest thing to an
//! authoritative "what could I go back to" — it outlives the Cellar, the
//! download cache, and own-brew's own history.

use crate::error::{Error, Result};
use serde::Deserialize;

const TOKEN: &str = "https://ghcr.io/token";
const REGISTRY: &str = "https://ghcr.io/v2/homebrew/core";

#[derive(Deserialize)]
struct Token {
    token: String,
}

#[derive(Deserialize)]
struct Tags {
    #[serde(default)]
    tags: Option<Vec<String>>,
}

/// `python@3.14` lives at `homebrew/core/python/3.14`.
fn repository(name: &str) -> String {
    name.replace('@', "/")
}

/// Published versions, oldest first.
///
/// Anonymous pulls need a token, which the registry hands out without
/// credentials for public repositories.
pub async fn versions(http: &reqwest::Client, name: &str) -> Result<Vec<String>> {
    super::validate(name)?;
    let repository = repository(name);

    // Built by hand rather than with reqwest's query helper, which sits behind
    // a feature flag; the components are validated formula names, so there is
    // nothing here that needs escaping.
    let token_url =
        format!("{TOKEN}?scope=repository:homebrew/core/{repository}:pull&service=ghcr.io");
    let token: Token = http
        .get(&token_url)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let response = http
        .get(format!("{REGISTRY}/{repository}/tags/list"))
        .bearer_auth(&token.token)
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(Error::Catalog(format!(
            "no published bottles found for {name} ({})",
            response.status()
        )));
    }

    let tags: Tags = response.json().await?;
    Ok(tags.tags.unwrap_or_default())
}

/// Published versions strictly older than `current`, newest first.
///
/// Homebrew's `-n` rebuild suffix is dropped: `1.7.1-1` and `1.7.1` install the
/// same upstream release, and offering both would be noise.
pub fn older_than(tags: &[String], current: &str) -> Vec<String> {
    use std::cmp::Ordering;

    let mut seen = std::collections::HashSet::new();
    let mut older: Vec<String> = tags
        .iter()
        .map(|tag| tag.split('-').next().unwrap_or(tag).to_owned())
        .filter(|version| {
            crate::history::diff::compare_versions(version, current) == Some(Ordering::Less)
        })
        .filter(|version| seen.insert(version.clone()))
        .collect();

    older.sort_by(|a, b| crate::history::diff::compare_versions(b, a).unwrap_or_else(|| b.cmp(a)));
    older
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_versioned_names_onto_registry_paths() {
        assert_eq!(repository("jq"), "jq");
        assert_eq!(repository("python@3.14"), "python/3.14");
        assert_eq!(repository("openssl@3"), "openssl/3");
    }

    #[test]
    fn keeps_only_older_versions_newest_first() {
        let tags = vec![
            "1.6-1".into(),
            "1.7".into(),
            "1.7.1".into(),
            "1.7.1-1".into(),
            "1.8.0".into(),
            "1.8.1".into(),
            "1.8.2".into(),
        ];
        let older = older_than(&tags, "1.8.2");
        assert_eq!(older, vec!["1.8.1", "1.8.0", "1.7.1", "1.7", "1.6"]);
    }

    #[test]
    fn rebuild_suffixes_do_not_produce_duplicates() {
        let tags = vec!["1.7.1".into(), "1.7.1-1".into(), "1.7.1-2".into()];
        assert_eq!(older_than(&tags, "1.8.0"), vec!["1.7.1"]);
    }

    #[test]
    fn nothing_older_than_the_oldest() {
        let tags = vec!["1.0".into(), "1.1".into()];
        assert!(older_than(&tags, "1.0").is_empty());
    }

    /// Against the real registry: this is where "any published version" comes
    /// from, so the tag list has to be readable without credentials.
    #[tokio::test]
    async fn lists_real_published_versions() {
        let http = reqwest::Client::builder()
            .user_agent("own-brew-test")
            .build()
            .unwrap();

        let Ok(tags) = versions(&http, "jq").await else {
            return; // offline
        };
        assert!(tags.contains(&"1.8.2".to_owned()), "got {tags:?}");

        let older = older_than(&tags, "1.8.2");
        assert!(older.contains(&"1.8.1".to_owned()));
        assert!(!older.contains(&"1.8.2".to_owned()));
    }
}
