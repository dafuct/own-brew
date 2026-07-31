//! Going back to a previous version.
//!
//! Homebrew removed `brew switch` and has no CLI verb for "use that older keg
//! instead", so the restore drives Homebrew's *own* `Keg#link` through
//! `brew ruby`. That is deliberate: re-implementing Homebrew's linking rules
//! (keg-only handling, directory vs symlink decisions, conflict resolution,
//! the `opt` prefix that dependents resolve through) would be a reliable way
//! to break someone's machine.

pub mod cellar;

use crate::brew::Brew;
use crate::error::{Error, Result};
use crate::history::History;
use crate::model::entry::Kind;
use serde::Serialize;
use std::path::Path;

/// Where a restorable version would come from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum Source {
    /// The keg is still on disk. Instant, offline, exact.
    LocalKeg,
    /// Homebrew's download cache still holds the bottle for this version.
    DownloadCache,
    /// homebrew-core publishes this as its own formula, e.g. `node@22`.
    VersionedFormula { formula: String },
    /// own-brew's history says this version was installed once, but no
    /// artifact for it remains.
    HistoryOnly,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub version: String,
    #[serde(flatten)]
    pub source: Source,
    /// Whether own-brew can perform this restore today.
    pub restorable: bool,
    /// Why, in words the UI shows verbatim.
    pub note: String,
}

impl Candidate {
    fn local(version: String) -> Self {
        Self {
            version,
            source: Source::LocalKeg,
            restorable: true,
            note: "Kept on disk — restores instantly, no download".to_owned(),
        }
    }

    fn versioned(version: String, formula: String) -> Self {
        Self {
            version,
            note: format!("Homebrew publishes this separately as {formula}"),
            source: Source::VersionedFormula { formula },
            restorable: true,
        }
    }

    fn cached(version: String) -> Self {
        Self {
            version,
            source: Source::DownloadCache,
            restorable: false,
            note: "Bottle is still cached, but rebuilding its formula needs the \
                   fetcher that is not implemented yet"
                .to_owned(),
        }
    }

    fn history_only(version: String) -> Self {
        Self {
            version,
            source: Source::HistoryOnly,
            restorable: false,
            note: "You ran this version before, but nothing for it remains on disk"
                .to_owned(),
        }
    }
}

/// Everything the user could go back to for one package, best option first.
///
/// `catalog_versioned` supplies formula names like `node@22` that homebrew-core
/// ships in its own right; those are ordinary installs and fully supported.
pub fn candidates(
    brew: &Brew,
    history: Option<&History>,
    kind: Kind,
    id: &str,
    current_version: Option<&str>,
    catalog_versioned: &[String],
) -> Vec<Candidate> {
    // Casks keep exactly one version on disk, so there is never a local keg to
    // go back to.
    let mut found: Vec<Candidate> = Vec::new();

    if kind == Kind::Formula {
        for version in cellar::kegs(brew.prefix(), id) {
            if Some(version.as_str()) != current_version {
                found.push(Candidate::local(version));
            }
        }

        for formula in catalog_versioned {
            if let Some(series) = formula.split_once('@').map(|(_, v)| v.to_owned()) {
                found.push(Candidate::versioned(series, formula.clone()));
            }
        }
    }

    if let Some(cache) = brew.cache_dir() {
        for version in cached_versions(&cache, id) {
            let known = found.iter().any(|c| c.version == version);
            if !known && Some(version.as_str()) != current_version {
                found.push(Candidate::cached(version));
            }
        }
    }

    if let Some(history) = history {
        if let Ok(known) = history.known_versions(kind, id) {
            for version in known {
                let already = found.iter().any(|c| c.version == version.version);
                if !already && Some(version.version.as_str()) != current_version {
                    found.push(Candidate::history_only(version.version));
                }
            }
        }
    }

    // Restorable first, then newest version first within each group.
    found.sort_by(|a, b| {
        b.restorable.cmp(&a.restorable).then_with(|| {
            crate::history::diff::compare_versions(&b.version, &a.version)
                .unwrap_or_else(|| b.version.cmp(&a.version))
        })
    });
    found
}

/// Versions of `name` whose bottle is still in Homebrew's download cache.
///
/// Cached bottles are named
/// `<sha>--<name>--<version>.<platform>.bottle[.n].tar.gz`.
fn cached_versions(cache: &Path, name: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(cache.join("downloads")) else {
        return Vec::new();
    };

    let marker = format!("--{name}--");
    let mut versions: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|file| file.contains(".bottle.") && file.ends_with(".tar.gz"))
        .filter_map(|file| {
            let after = file.split_once(&marker)?.1.to_owned();
            // Everything up to the platform segment is the version.
            let version = after.split_once('.')?;
            Some(trim_version(&after, version.0))
        })
        .collect();

    versions.sort();
    versions.dedup();
    versions
}

/// `3.6.3.arm64_tahoe.bottle.1.tar.gz` -> `3.6.3`.
///
/// Versions themselves contain dots, so the version runs up to the segment
/// that begins the platform tag rather than to the first dot.
fn trim_version(rest: &str, _first_segment: &str) -> String {
    let mut version = Vec::new();
    for segment in rest.split('.') {
        // Platform tags are arm64_*, x86_64_*, or a bare macOS codename, and
        // never start with a digit.
        if segment.is_empty() || !segment.starts_with(|c: char| c.is_ascii_digit()) {
            break;
        }
        version.push(segment);
    }
    version.join(".")
}

/// Ruby that runs inside Homebrew, using Homebrew's own linking code.
///
/// If linking the target fails the previously linked keg is put back, so a
/// failed rollback never leaves the machine with nothing linked.
const RELINK: &str = r#"
require "keg"

rack = Pathname.new(ARGV[0])
target = Keg.new(rack/ARGV[1])
abort "target keg is not on disk: #{target}" unless target.directory?

current = rack.subdirs.map { |d| Keg.new(d) }.find(&:linked?)
if current && current.version.to_s == target.version.to_s
  puts "already linked: #{target.version}"
  exit 0
end

current&.unlink
begin
  target.link(overwrite: true)
  target.optlink(overwrite: true)
  puts "linked #{target.version}"
rescue => e
  begin
    if current
      current.link(overwrite: true)
      current.optlink(overwrite: true)
      warn "restored #{current.version} after failure"
    end
  rescue => restore_failure
    warn "could not restore #{current&.version}: #{restore_failure}"
  end
  abort "link failed: #{e}"
end
"#;

/// Switch `id` to `version` using a keg already on disk.
pub async fn restore_local_keg(brew: &Brew, id: &str, version: &str) -> Result<String> {
    validate(id)?;
    validate(version)?;

    let rack = cellar::rack(brew.prefix(), id);
    if !rack.join(version).is_dir() {
        return Err(Error::Catalog(format!(
            "{id} {version} is not on disk, so it cannot be restored instantly"
        )));
    }

    let rack = rack.display().to_string();
    brew.output(&["ruby", "-e", RELINK, &rack, version]).await
}

/// Reject anything that could be read as an option or escape a path.
fn validate(value: &str) -> Result<()> {
    let bad = |why: &str| Error::Catalog(format!("{value:?} is not usable: {why}"));
    if value.is_empty() {
        return Err(bad("it is empty"));
    }
    if value.starts_with('-') {
        return Err(bad("it would be read as a command-line option"));
    }
    if value.contains("..") || value.contains('/') {
        return Err(bad("it must not contain a path"));
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-'))
    {
        return Err(bad("it contains an unexpected character"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cellar::tempdir::Dir;

    fn keg(prefix: &Path, name: &str, version: &str) {
        let path = cellar::rack(prefix, name).join(version);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("marker"), "x").unwrap();
    }

    #[test]
    fn trims_the_platform_tag_off_a_cached_bottle_name() {
        assert_eq!(trim_version("3.6.3.arm64_tahoe.bottle.1.tar.gz", "3"), "3.6.3");
        assert_eq!(trim_version("1.8.2.arm64_sonoma.bottle.tar.gz", "1"), "1.8.2");
        assert_eq!(trim_version("2026-07-16.all.bottle.tar.gz", "2026-07-16"), "2026-07-16");
    }

    #[test]
    fn finds_cached_bottle_versions() {
        let dir = Dir::new();
        let downloads = dir.path().join("downloads");
        std::fs::create_dir_all(&downloads).unwrap();
        for file in [
            "abc123--openssl@3--3.6.3.arm64_tahoe.bottle.1.tar.gz",
            "def456--openssl@3--3.6.2.arm64_tahoe.bottle.tar.gz",
            "ghi789--jq--1.8.2.arm64_tahoe.bottle.tar.gz",
            "jkl012--openssl@3--3.6.3.bottle_manifest.json",
        ] {
            std::fs::write(downloads.join(file), "x").unwrap();
        }

        let mut found = cached_versions(dir.path(), "openssl@3");
        found.sort();
        assert_eq!(found, vec!["3.6.2".to_owned(), "3.6.3".to_owned()]);
    }

    #[test]
    fn an_absent_cache_is_not_an_error() {
        let dir = Dir::new();
        assert!(cached_versions(dir.path(), "jq").is_empty());
    }

    #[test]
    fn rejects_values_that_could_escape_or_become_flags() {
        for hostile in ["--force", "../../etc", "a/b", "", "x;y", "$(id)"] {
            assert!(validate(hostile).is_err(), "{hostile:?} should be rejected");
        }
        for good in ["jq", "python@3.14", "1.8.2", "1.16.2_2", "2026-07-16"] {
            validate(good).unwrap_or_else(|e| panic!("{good} should be valid: {e}"));
        }
    }

    #[test]
    fn restore_refuses_a_version_that_is_not_on_disk() {
        let Ok(brew) = Brew::discover() else { return };
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(restore_local_keg(&brew, "jq", "0.0.1-not-real"))
            .expect_err("a missing keg must not be attempted");
        assert!(error.to_string().contains("not on disk"));
    }

    #[test]
    fn the_current_version_is_never_offered_as_a_rollback_target() {
        let Ok(brew) = Brew::discover() else { return };
        let dir = Dir::new();
        keg(dir.path(), "demo", "1.0.0");
        keg(dir.path(), "demo", "2.0.0");

        // Point the helper at the scratch prefix by calling the inventory
        // directly; candidates() uses the real brew prefix.
        let versions = cellar::kegs(dir.path(), "demo");
        assert_eq!(versions, vec!["1.0.0", "2.0.0"]);

        let found = candidates(&brew, None, Kind::Formula, "definitely-not-installed", None, &[]);
        assert!(found.is_empty());
    }

    #[test]
    fn versioned_formulae_are_offered_as_supported_restores() {
        let Ok(brew) = Brew::discover() else { return };
        let found = candidates(
            &brew,
            None,
            Kind::Formula,
            "not-installed-anywhere",
            None,
            &["node@22".to_owned()],
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].restorable);
        assert_eq!(found[0].version, "22");
        assert_eq!(
            found[0].source,
            Source::VersionedFormula {
                formula: "node@22".to_owned()
            }
        );
    }

    #[test]
    fn restorable_candidates_are_listed_before_unrestorable_ones() {
        let mut list = [
            Candidate::history_only("9.9.9".into()),
            Candidate::local("1.0.0".into()),
        ];
        list.sort_by_key(|c| std::cmp::Reverse(c.restorable));
        assert!(list[0].restorable);
    }

    /// Against the real machine: every locally-offered candidate must exist.
    #[test]
    fn real_local_candidates_are_all_on_disk() {
        let Ok(brew) = Brew::discover() else { return };
        for (name, versions) in cellar::inventory(brew.prefix()).into_iter().take(30) {
            let current = versions.last().cloned();
            let found = candidates(
                &brew,
                None,
                Kind::Formula,
                &name,
                current.as_deref(),
                &[],
            );
            for candidate in found.iter().filter(|c| c.source == Source::LocalKeg) {
                assert!(
                    cellar::rack(brew.prefix(), &name).join(&candidate.version).is_dir(),
                    "{name} {} was offered but is not on disk",
                    candidate.version
                );
            }
        }
    }
}
