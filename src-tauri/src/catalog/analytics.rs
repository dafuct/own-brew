//! Homebrew's public install counts, used for popularity ranking.
//!
//! Counts arrive as thousands-separated strings (`"1,432,817"`), and the feed
//! is optional: if it can't be fetched the catalog still works, just without
//! a popularity ordering.

use crate::model::entry::Kind;
use serde::Deserialize;
use std::collections::HashMap;

const API_BASE: &str = "https://formulae.brew.sh/api/analytics";

#[derive(Deserialize)]
struct Report {
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    /// Present on the formula feed.
    #[serde(default)]
    formula: Option<String>,
    /// Present on the cask feed.
    #[serde(default)]
    cask: Option<String>,
    #[serde(default)]
    count: String,
}

impl Item {
    fn id(&self) -> Option<&str> {
        self.formula.as_deref().or(self.cask.as_deref())
    }
}

/// Fetch 90-day install counts, keyed by package id.
///
/// Errors are deliberately swallowed into an empty map: popularity is a
/// nice-to-have, and an offline machine should still get a usable catalog.
pub async fn fetch(http: &reqwest::Client, kind: Kind) -> HashMap<String, u64> {
    let endpoint = match kind {
        Kind::Formula => "install/90d.json",
        Kind::Cask => "cask-install/90d.json",
    };
    let url = format!("{API_BASE}/{endpoint}");

    match load(http, &url).await {
        Ok(counts) => counts,
        Err(e) => {
            tracing::warn!(%url, error = %e, "install analytics unavailable; ranking by name instead");
            HashMap::new()
        }
    }
}

/// Build failures per formula over the last 30 days.
///
/// Used as a proxy for "this formula is currently troublesome". Absent data
/// degrades to an empty map rather than an error.
pub async fn build_errors(http: &reqwest::Client) -> HashMap<String, u64> {
    let url = format!("{API_BASE}/build-error/30d.json");
    match load(http, &url).await {
        Ok(counts) => counts,
        Err(e) => {
            tracing::warn!(%url, error = %e, "build-error analytics unavailable");
            HashMap::new()
        }
    }
}

async fn load(http: &reqwest::Client, url: &str) -> crate::Result<HashMap<String, u64>> {
    let report: Report = http.get(url).send().await?.error_for_status()?.json().await?;
    Ok(report
        .items
        .iter()
        .filter_map(|item| Some((item.id()?.to_owned(), parse_count(&item.count)?)))
        .collect())
}

/// `"1,432,817"` -> `1432817`.
fn parse_count(raw: &str) -> Option<u64> {
    let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_thousands_separators() {
        assert_eq!(parse_count("1,432,817"), Some(1_432_817));
        assert_eq!(parse_count("288048"), Some(288_048));
        assert_eq!(parse_count("0"), Some(0));
    }

    #[test]
    fn rejects_unparseable_counts() {
        assert_eq!(parse_count(""), None);
        assert_eq!(parse_count("n/a"), None);
    }

    #[test]
    fn reads_both_feed_shapes() {
        let formula: Report = serde_json::from_str(
            r#"{"items":[{"number":1,"formula":"openssl@3","count":"1,432,817"}]}"#,
        )
        .unwrap();
        assert_eq!(formula.items[0].id(), Some("openssl@3"));

        let cask: Report = serde_json::from_str(
            r#"{"items":[{"number":1,"cask":"claude-code","count":"288,048"}]}"#,
        )
        .unwrap();
        assert_eq!(cask.items[0].id(), Some("claude-code"));
    }
}
