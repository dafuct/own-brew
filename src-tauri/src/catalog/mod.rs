//! The in-memory package catalog.
//!
//! Loaded once at startup and kept resident: ~16,000 lean records is a few
//! megabytes, and having them in memory is what makes search feel instant.
//! Full package detail is deliberately *not* held here — it comes from
//! `brew info` on demand, where it is authoritative about local state.

pub mod analytics;
pub mod search;
pub mod source;

pub use search::{Query, Sort};
pub use source::Origin;

use crate::error::Result;
use crate::model::entry::{Entry, Kind};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;

pub struct Catalog {
    entries: Vec<Entry>,
    origin: Origin,
    loaded_at: SystemTime,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub formulae: usize,
    pub casks: usize,
    pub origin: Origin,
    /// Unix seconds; the UI shows this as "catalog updated …".
    pub loaded_at: u64,
    pub has_popularity: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    /// Total matches, not the number returned — the UI needs it for paging.
    pub total: usize,
    pub items: Vec<Entry>,
}

impl Catalog {
    /// Load both catalogs and decorate them with install counts.
    ///
    /// The two catalogs and the two analytics feeds are independent, so they
    /// are fetched concurrently; a failed analytics fetch degrades to "no
    /// popularity data" rather than failing the load.
    pub async fn load(http: &reqwest::Client, brew_cache: Option<PathBuf>) -> Result<Self> {
        let loader = source::Loader { http, brew_cache };

        let (formulae, casks, formula_installs, cask_installs) = tokio::join!(
            loader.load(Kind::Formula),
            loader.load(Kind::Cask),
            analytics::fetch(http, Kind::Formula),
            analytics::fetch(http, Kind::Cask),
        );

        let (mut entries, formula_origin) = formulae?;
        let (casks, cask_origin) = casks?;
        entries.extend(casks);

        apply_popularity(&mut entries, &formula_installs, &cask_installs);

        Ok(Self {
            entries,
            // If either half had to hit the network, say so.
            origin: if formula_origin == Origin::Network || cask_origin == Origin::Network {
                Origin::Network
            } else {
                Origin::BrewCache
            },
            loaded_at: SystemTime::now(),
        })
    }

    pub fn stats(&self) -> Stats {
        Stats {
            formulae: self.count(Kind::Formula),
            casks: self.count(Kind::Cask),
            origin: self.origin,
            loaded_at: self
                .loaded_at
                .duration_since(SystemTime::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or_default(),
            has_popularity: self.entries.iter().any(|e| e.installs_90d.is_some()),
        }
    }

    fn count(&self, kind: Kind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }

    pub fn get(&self, kind: Kind, id: &str) -> Option<&Entry> {
        self.entries
            .iter()
            .find(|e| e.kind == kind && e.id.eq_ignore_ascii_case(id))
    }

    pub fn search(&self, query: &Query) -> Page {
        let terms = search::terms(&query.text);

        let mut hits: Vec<(i64, &Entry)> = self
            .entries
            .iter()
            .filter(|e| query.kind.is_none_or(|k| e.kind == k))
            .filter(|e| query.include_unavailable || e.is_available())
            .filter_map(|e| search::score(e, &terms).map(|s| (s, e)))
            .collect();

        match query.sort {
            search::Sort::Relevance => {
                hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.id.cmp(&b.1.id)))
            }
            search::Sort::Popularity => hits.sort_by(|a, b| {
                b.1.installs_90d
                    .cmp(&a.1.installs_90d)
                    .then_with(|| a.1.id.cmp(&b.1.id))
            }),
            search::Sort::Name => hits.sort_by(|a, b| a.1.id.cmp(&b.1.id)),
        }

        let total = hits.len();
        let items = hits
            .into_iter()
            .skip(query.offset)
            .take(query.limit)
            .map(|(_, e)| e.clone())
            .collect();

        Page { total, items }
    }
}

fn apply_popularity(
    entries: &mut [Entry],
    formula_installs: &HashMap<String, u64>,
    cask_installs: &HashMap<String, u64>,
) {
    for entry in entries {
        let table = match entry.kind {
            Kind::Formula => formula_installs,
            Kind::Cask => cask_installs,
        };
        entry.installs_90d = table.get(&entry.id).copied();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(entries: Vec<Entry>) -> Catalog {
        Catalog {
            entries,
            origin: Origin::BrewCache,
            loaded_at: SystemTime::UNIX_EPOCH,
        }
    }

    fn entry(kind: Kind, id: &str, desc: &str) -> Entry {
        let mut e = Entry {
            kind,
            id: id.to_owned(),
            name: id.to_owned(),
            desc: Some(desc.to_owned()),
            version: "1.0".to_owned(),
            tap: "homebrew/core".to_owned(),
            homepage: None,
            deprecated: false,
            disabled: false,
            installs_90d: None,
            haystack_name: String::new(),
            haystack_desc: String::new(),
        };
        e.rehydrate();
        e
    }

    fn sample() -> Catalog {
        catalog(vec![
            entry(Kind::Formula, "git", "version control"),
            entry(Kind::Formula, "wget", "file retriever"),
            entry(Kind::Cask, "ghostty", "terminal emulator"),
            entry(Kind::Cask, "warp", "terminal emulator"),
        ])
    }

    #[test]
    fn filters_by_kind() {
        let page = sample().search(&Query {
            kind: Some(Kind::Cask),
            ..Default::default()
        });
        assert_eq!(page.total, 2);
        assert!(page.items.iter().all(|e| e.kind == Kind::Cask));
    }

    #[test]
    fn reports_total_matches_independently_of_the_page_size() {
        let page = sample().search(&Query {
            text: "terminal".into(),
            limit: 1,
            ..Default::default()
        });
        assert_eq!(page.total, 2, "both terminals matched");
        assert_eq!(page.items.len(), 1, "but only one was returned");
    }

    #[test]
    fn paginates_without_repeating_or_skipping() {
        let cat = sample();
        let first = cat.search(&Query {
            sort: Sort::Name,
            limit: 2,
            ..Default::default()
        });
        let second = cat.search(&Query {
            sort: Sort::Name,
            limit: 2,
            offset: 2,
            ..Default::default()
        });
        let ids: Vec<_> = first
            .items
            .iter()
            .chain(second.items.iter())
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(ids, ["ghostty", "git", "warp", "wget"]);
    }

    #[test]
    fn hides_disabled_packages_unless_asked() {
        let mut gone = entry(Kind::Formula, "abandoned", "no longer works");
        gone.disabled = true;
        let cat = catalog(vec![gone]);

        assert_eq!(cat.search(&Query::default()).total, 0);
        assert_eq!(
            cat.search(&Query {
                include_unavailable: true,
                ..Default::default()
            })
            .total,
            1
        );
    }

    #[test]
    fn lookup_is_case_insensitive_and_kind_scoped() {
        let cat = sample();
        assert!(cat.get(Kind::Cask, "GHOSTTY").is_some());
        assert!(
            cat.get(Kind::Formula, "ghostty").is_none(),
            "a cask must not be found as a formula"
        );
    }

    #[test]
    fn popularity_sort_orders_by_install_count() {
        let mut a = entry(Kind::Formula, "a", "");
        a.installs_90d = Some(10);
        let mut b = entry(Kind::Formula, "b", "");
        b.installs_90d = Some(9_000);
        let page = catalog(vec![a, b]).search(&Query {
            sort: Sort::Popularity,
            ..Default::default()
        });
        assert_eq!(page.items[0].id, "b");
    }

    #[test]
    fn popularity_is_matched_per_kind() {
        // A formula and a cask can share a name; their counts must not cross.
        let mut entries = vec![
            entry(Kind::Formula, "docker", "cli"),
            entry(Kind::Cask, "docker", "desktop app"),
        ];
        let formulae = HashMap::from([("docker".to_owned(), 111)]);
        let casks = HashMap::from([("docker".to_owned(), 222)]);
        apply_popularity(&mut entries, &formulae, &casks);

        assert_eq!(entries[0].installs_90d, Some(111));
        assert_eq!(entries[1].installs_90d, Some(222));
    }

    #[test]
    fn stats_count_each_kind() {
        let stats = sample().stats();
        assert_eq!(stats.formulae, 2);
        assert_eq!(stats.casks, 2);
        assert!(!stats.has_popularity);
    }
}
