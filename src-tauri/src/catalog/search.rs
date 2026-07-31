//! Ranking for catalog search.
//!
//! Every query term has to match somewhere, and terms that match the package's
//! name outrank terms that only appear in its description — so searching
//! "code" surfaces the tool actually called `code` rather than the hundreds of
//! packages that merely mention code in passing.

use crate::model::entry::{Entry, Kind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// Best match first; falls back to popularity when there is no query.
    #[default]
    Relevance,
    Popularity,
    Name,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    pub text: String,
    pub kind: Option<Kind>,
    /// Deprecated packages still install, so they are shown but ranked down.
    /// Disabled ones cannot be installed at all and are hidden by default.
    pub include_unavailable: bool,
    pub sort: Sort,
    pub limit: usize,
    pub offset: usize,
}

impl Default for Query {
    fn default() -> Self {
        Self {
            text: String::new(),
            kind: None,
            include_unavailable: false,
            sort: Sort::default(),
            limit: 50,
            offset: 0,
        }
    }
}

const EXACT_ID: i64 = 1000;
const NAME_PREFIX: i64 = 600;
const NAME_WORD_START: i64 = 450;
const NAME_SUBSTRING: i64 = 300;
const DESC_SUBSTRING: i64 = 110;
const DEPRECATED_PENALTY: i64 = -400;

/// Score `entry` against the already-lowercased `terms`.
/// `None` means at least one term didn't match, so the entry is excluded.
pub fn score(entry: &Entry, terms: &[String]) -> Option<i64> {
    if terms.is_empty() {
        return Some(popularity_bonus(entry));
    }

    let mut total = 0;
    for term in terms {
        total += term_score(entry, term)?;
    }

    // Prefer the shorter of two equally good matches: searching "git" should
    // rank `git` above `git-delta`.
    total -= (entry.id.len() as i64).min(40);
    total += popularity_bonus(entry);
    if entry.deprecated {
        total += DEPRECATED_PENALTY;
    }
    Some(total)
}

fn term_score(entry: &Entry, term: &str) -> Option<i64> {
    if entry.id.eq_ignore_ascii_case(term) {
        return Some(EXACT_ID);
    }
    if entry.haystack_name.starts_with(term) {
        return Some(NAME_PREFIX);
    }
    if starts_a_word(&entry.haystack_name, term) {
        return Some(NAME_WORD_START);
    }
    if entry.haystack_name.contains(term) {
        return Some(NAME_SUBSTRING);
    }
    if entry.haystack_desc.contains(term) {
        return Some(DESC_SUBSTRING);
    }
    None
}

/// Does `term` begin a word inside `haystack`? Hyphens, spaces and `@` all
/// separate words in package names (`visual-studio-code`, `python@3.14`).
fn starts_a_word(haystack: &str, term: &str) -> bool {
    haystack.match_indices(term).any(|(at, _)| {
        at == 0
            || haystack[..at]
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_alphanumeric())
    })
}

/// A mild nudge so popular packages win ties, never so large that it
/// outweighs a genuinely better textual match.
fn popularity_bonus(entry: &Entry) -> i64 {
    match entry.installs_90d {
        Some(installs) => ((installs as f64).max(1.0).log10() * 12.0) as i64,
        None => 0,
    }
}

/// Split a raw query into lowercased terms.
pub fn terms(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(str::to_lowercase)
        .filter(|t| !t.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str, desc: &str) -> Entry {
        let mut e = Entry {
            kind: Kind::Formula,
            id: id.to_owned(),
            name: name.to_owned(),
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

    fn rank(entries: &[Entry], query: &str) -> Vec<String> {
        let terms = terms(query);
        let mut scored: Vec<_> = entries
            .iter()
            .filter_map(|e| score(e, &terms).map(|s| (s, e)))
            .collect();
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        scored.into_iter().map(|(_, e)| e.id.clone()).collect()
    }

    #[test]
    fn exact_name_wins() {
        let entries = [
            entry("git-delta", "git-delta", "syntax highlighter"),
            entry("git", "git", "distributed version control"),
            entry("tig", "tig", "text interface for git"),
        ];
        assert_eq!(rank(&entries, "git")[0], "git");
    }

    #[test]
    fn name_matches_outrank_description_matches() {
        let entries = [
            entry("cmus", "cmus", "music player for the terminal"),
            entry("musicbox", "musicbox", "album art fetcher"),
        ];
        assert_eq!(rank(&entries, "music")[0], "musicbox");
    }

    #[test]
    fn shorter_names_win_ties() {
        let entries = [
            entry("node-build", "node-build", "install node versions"),
            entry("node", "node", "javascript runtime"),
        ];
        assert_eq!(rank(&entries, "node")[0], "node");
    }

    #[test]
    fn every_term_must_match() {
        let entries = [
            entry("vscode", "Visual Studio Code", "code editor"),
            entry("vim", "vim", "text editor"),
        ];
        let found = rank(&entries, "visual editor");
        assert_eq!(found, ["vscode"], "vim matches 'editor' but not 'visual'");
    }

    #[test]
    fn matches_a_word_inside_a_hyphenated_name() {
        let entries = [entry("visual-studio-code", "visual-studio-code", "editor")];
        assert!(score(&entries[0], &terms("studio")).is_some());
    }

    #[test]
    fn word_start_beats_a_mid_word_substring() {
        let mid = entry("libgit2", "libgit2", "unrelated");
        let start = entry("git-lfs", "git-lfs", "unrelated");
        let t = terms("git");
        assert!(score(&start, &t) > score(&mid, &t));
    }

    #[test]
    fn deprecated_packages_rank_below_maintained_ones() {
        let mut old = entry("foo-old", "foo-old", "does foo");
        old.deprecated = true;
        let current = entry("foo-new", "foo-new", "does foo");
        let t = terms("foo");
        assert!(score(&current, &t) > score(&old, &t));
    }

    #[test]
    fn popularity_breaks_ties_but_does_not_override_relevance() {
        let mut popular = entry("aaa", "aaa", "mentions node in passing");
        popular.installs_90d = Some(5_000_000);
        let relevant = entry("node", "node", "javascript runtime");
        let t = terms("node");
        assert!(
            score(&relevant, &t) > score(&popular, &t),
            "a name match must beat a popular description match"
        );

        let mut quiet = entry("bbb", "bbb", "mentions node in passing");
        quiet.installs_90d = Some(10);
        assert!(score(&popular, &t) > score(&quiet, &t));
    }

    #[test]
    fn non_matching_entries_are_excluded() {
        let e = entry("wget", "wget", "internet file retriever");
        assert!(score(&e, &terms("photoshop")).is_none());
    }

    #[test]
    fn an_empty_query_matches_everything() {
        let e = entry("wget", "wget", "internet file retriever");
        assert!(score(&e, &terms("   ")).is_some());
    }

    #[test]
    fn search_is_case_insensitive() {
        let e = entry("vscode", "Visual Studio Code", "editor");
        assert!(score(&e, &terms("VISUAL")).is_some());
    }
}
