//! Recovering the formula for a version that is no longer on disk.
//!
//! The obvious route is a dead end. Every bottle on ghcr.io records the
//! homebrew-core commit that produced it, but those commits are unreachable —
//! Homebrew's merge queue rebases, so `git fetch` of a recorded revision
//! answers `upload-pack: not our ref`. `brew extract` is the supported
//! alternative and needs a full homebrew-core clone, which API-only installs
//! (today's default) do not have.
//!
//! What does work is asking GitHub which commits touched the formula file.
//! Those SHAs are reachable, the file can be fetched at any of them, and for a
//! typical formula the whole history is a few dozen commits.

use crate::error::{Error, Result};
use serde::Deserialize;

const API: &str = "https://api.github.com/repos/Homebrew/homebrew-core";
const RAW: &str = "https://raw.githubusercontent.com/Homebrew/homebrew-core";

/// How many commits to consider. Formula histories are short; this is a guard
/// against pathological cases, not a real limit.
const MAX_COMMITS: usize = 100;
/// How many candidate files to actually download before giving up.
const MAX_FETCHES: usize = 6;

#[derive(Debug, Deserialize)]
struct Commit {
    sha: String,
    commit: CommitDetail,
}

#[derive(Debug, Deserialize)]
struct CommitDetail {
    #[serde(default)]
    message: String,
}

/// A formula file recovered from history.
#[derive(Clone, Debug)]
pub struct Recovered {
    pub name: String,
    pub version: String,
    /// The commit it came from, so the provenance can be shown and audited.
    pub sha: String,
    pub source_url: String,
    pub ruby: String,
}

/// Where a formula file lives, newest layout first.
///
/// homebrew-core moved formulae into per-letter directories in 2024, so older
/// commits still have them at the top level.
fn paths_for(name: &str) -> Vec<String> {
    let first = name.chars().next().unwrap_or('_').to_ascii_lowercase();
    vec![
        format!("Formula/{first}/{name}.rb"),
        format!("Formula/{name}.rb"),
    ]
}

/// Does this formula file actually describe `version`?
///
/// A cheap pre-check, not a parser: Homebrew verifies the version for real
/// once the file is installed into a tap.
fn describes(ruby: &str, version: &str) -> bool {
    let upstream = version.split('_').next().unwrap_or(version);
    ruby.contains(upstream)
}

/// Does a commit message look like it concerns `version`?
///
/// Homebrew's messages are conventionally `jq 1.8.1` or
/// `jq: update 1.8.1 bottle.`, so this is a strong ranking signal — but it is
/// only used to order candidates. The recovered file is always verified
/// afterwards by Homebrew itself.
fn mentions(message: &str, version: &str) -> bool {
    // Compare against the upstream version, ignoring Homebrew's `_n` rebuild
    // suffix which never appears in the commit subject.
    let upstream = version.split('_').next().unwrap_or(version);
    message
        .lines()
        .next()
        .unwrap_or_default()
        .contains(upstream)
}

pub struct Fetcher<'a> {
    pub http: &'a reqwest::Client,
}

impl Fetcher<'_> {
    /// Find the formula file for `name` at `version`.
    ///
    /// Candidates are ordered newest-first, because the commit that *adds* a
    /// version has no bottles yet — the later "update bottle" commit is the one
    /// that can actually be poured.
    pub async fn find(&self, name: &str, version: &str) -> Result<Recovered> {
        super::validate(name)?;
        super::validate(version)?;

        let mut last_error = None;

        for path in paths_for(name) {
            let commits = match self.commits_touching(&path).await {
                Ok(commits) if !commits.is_empty() => commits,
                Ok(_) => continue,
                Err(e) => {
                    last_error = Some(e);
                    continue;
                }
            };

            // Commits whose subject names the version come first; everything
            // else stays as a fallback in history order.
            let (named, rest): (Vec<_>, Vec<_>) = commits
                .iter()
                .partition(|c| mentions(&c.commit.message, version));

            for commit in named.iter().chain(rest.iter()).take(MAX_FETCHES) {
                match self.file_at(&commit.sha, &path).await {
                    Ok(ruby) if !describes(&ruby, version) => {
                        // A formula for this version names it in its url or a
                        // `version` stanza. Anything else is the wrong commit;
                        // Homebrew verifies properly once the file is in a tap,
                        // but there is no point materialising a known miss.
                        last_error = Some(Error::Catalog(format!(
                            "{} does not carry {name} {version}",
                            &commit.sha[..10.min(commit.sha.len())]
                        )));
                    }
                    Ok(ruby) => {
                        return Ok(Recovered {
                            name: name.to_owned(),
                            version: version.to_owned(),
                            sha: commit.sha.clone(),
                            source_url: format!("{RAW}/{}/{path}", commit.sha),
                            ruby,
                        })
                    }
                    Err(e) => last_error = Some(e),
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            Error::Catalog(format!(
                "no commit in homebrew-core appears to carry {name} {version}"
            ))
        }))
    }

    /// Every candidate: commits that touched the file, plus the ones whose
    /// subject names the version, so [`find`] can try them in a useful order.
    ///
    /// Returns an empty list rather than an error when the path never existed.
    async fn commits_touching(&self, path: &str) -> Result<Vec<Commit>> {
        let url = format!("{API}/commits?path={path}&per_page={MAX_COMMITS}");
        let response = self.http.get(&url).send().await?;

        if response.status() == reqwest::StatusCode::FORBIDDEN
            || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            // Unauthenticated GitHub allows 60 requests an hour. Say so plainly
            // rather than reporting a generic failure.
            return Err(Error::Catalog(
                "GitHub's rate limit for anonymous requests has been reached; \
                 recovering old versions will work again within the hour"
                    .to_owned(),
            ));
        }

        let response = response.error_for_status()?;
        Ok(response.json().await?)
    }

    async fn file_at(&self, sha: &str, path: &str) -> Result<String> {
        let url = format!("{RAW}/{sha}/{path}");
        let response = self.http.get(&url).send().await?;
        if !response.status().is_success() {
            return Err(Error::Catalog(format!(
                "formula file not present at {sha:.10} ({})",
                response.status()
            )));
        }
        Ok(response.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_in_both_layouts_newest_first() {
        assert_eq!(
            paths_for("jq"),
            vec!["Formula/j/jq.rb".to_owned(), "Formula/jq.rb".to_owned()]
        );
        // Versioned and punctuated names use their first character.
        assert_eq!(paths_for("python@3.14")[0], "Formula/p/python@3.14.rb");
        assert_eq!(paths_for("p11-kit")[0], "Formula/p/p11-kit.rb");
    }

    #[test]
    fn recognises_homebrews_commit_subjects() {
        assert!(mentions("jq 1.8.1", "1.8.1"));
        assert!(mentions("jq: update 1.8.1 bottle.", "1.8.1"));
        assert!(!mentions("jq 1.8.2", "1.8.1"));
        // Homebrew's own rebuild suffix never appears in the subject.
        assert!(mentions("openssl@3 3.6.3", "3.6.3_1"));
        // Only the subject line counts, not the body.
        assert!(!mentions("jq: tidy up\n\nunrelated 1.8.1 mention", "1.8.1"));
    }

    #[test]
    fn a_file_for_the_wrong_version_is_not_accepted() {
        let jq_182 = r#"class Jq < Formula
          url "https://github.com/jqlang/jq/releases/download/jq-1.8.2/jq-1.8.2.tar.gz"
        end"#;
        assert!(describes(jq_182, "1.8.2"));
        assert!(!describes(jq_182, "1.8.1"));
        // Homebrew's rebuild suffix is not part of the upstream version.
        assert!(describes(jq_182, "1.8.2_1"));
    }

    #[test]
    fn rejects_names_that_could_escape_the_url() {
        let http = reqwest::Client::new();
        let fetcher = Fetcher { http: &http };
        let runtime = tokio::runtime::Runtime::new().unwrap();

        for (name, version) in [("../../etc", "1.0"), ("jq", "--force"), ("a b", "1")] {
            assert!(
                runtime.block_on(fetcher.find(name, version)).is_err(),
                "{name} {version} should be rejected"
            );
        }
    }

    /// Against the real GitHub API. This is the assumption the whole feature
    /// rests on: that a formula's history is reachable and fetchable.
    ///
    /// Skips itself when the anonymous rate limit is exhausted, so a busy
    /// machine does not produce a spurious failure.
    #[tokio::test]
    async fn recovers_a_real_historical_formula() {
        let http = reqwest::Client::builder()
            .user_agent("own-brew-test")
            .build()
            .unwrap();
        let fetcher = Fetcher { http: &http };

        let recovered = match fetcher.find("jq", "1.8.1").await {
            Ok(recovered) => recovered,
            Err(e) if e.to_string().contains("rate limit") => return,
            Err(e) => panic!("could not recover jq 1.8.1: {e}"),
        };

        assert!(recovered.ruby.contains("class Jq"));
        assert!(
            recovered.ruby.contains("1.8.1"),
            "the recovered file should name the version it is for"
        );
        assert!(
            recovered.ruby.contains("bottle do"),
            "without a bottle block the version could not be poured"
        );
        assert_eq!(recovered.sha.len(), 40);
    }
}
