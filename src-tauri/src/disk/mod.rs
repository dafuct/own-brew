//! What Homebrew costs in disk, and what it would cost to reclaim it.
//!
//! own-brew runs Homebrew with `HOMEBREW_NO_INSTALL_CLEANUP=1`, which is what
//! keeps superseded kegs around for instant rollback. That is a decision the
//! app makes on the user's behalf, and it spends their disk — so the app owes
//! them an honest account of the cost and a way to reclaim it.
//!
//! The trade-off is stated rather than hidden: **the superseded kegs are the
//! undo capability.** Reclaiming that space removes the ability to go back.

use crate::brew::Brew;
use crate::error::Result;
use crate::rollback::cellar;
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupersededKeg {
    pub formula: String,
    pub version: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Footprint {
    pub cellar_bytes: u64,
    pub caskroom_bytes: u64,
    pub cache_bytes: u64,
    pub total_bytes: u64,

    /// Old versions kept on disk purely so an upgrade can be undone.
    pub superseded: Vec<SupersededKeg>,
    pub superseded_bytes: u64,

    /// What `brew cleanup` reports it would free, when it can be determined.
    pub cleanup_estimate_bytes: Option<u64>,
}

/// Measure the installation.
///
/// Walking the Cellar touches a lot of inodes, so this runs on a blocking
/// thread and is not something to call on every render.
pub async fn footprint(brew: &Brew) -> Result<Footprint> {
    let prefix = brew.prefix().to_path_buf();
    let cache = brew.cache_dir();
    let active = active_versions(brew).await;

    let mut footprint = tokio::task::spawn_blocking(move || {
        let cellar = prefix.join("Cellar");
        let caskroom = prefix.join("Caskroom");

        let mut superseded = Vec::new();
        for (formula, versions) in cellar::inventory(&cellar_prefix(&prefix)) {
            let current = active.get(&formula);
            for version in versions {
                if Some(&version) == current {
                    continue;
                }
                superseded.push(SupersededKeg {
                    bytes: size_of(&cellar.join(&formula).join(&version)),
                    formula: formula.clone(),
                    version,
                });
            }
        }
        // Biggest first: that is the order a user reclaiming space cares about.
        superseded.sort_by_key(|k| std::cmp::Reverse(k.bytes));

        let cellar_bytes = size_of(&cellar);
        let caskroom_bytes = size_of(&caskroom);
        let cache_bytes = cache.as_deref().map(size_of).unwrap_or(0);

        Footprint {
            cellar_bytes,
            caskroom_bytes,
            cache_bytes,
            total_bytes: cellar_bytes + caskroom_bytes + cache_bytes,
            superseded_bytes: superseded.iter().map(|k| k.bytes).sum(),
            superseded,
            cleanup_estimate_bytes: None,
        }
    })
    .await
    .map_err(|e| crate::Error::Catalog(format!("disk scan failed: {e}")))?;

    footprint.cleanup_estimate_bytes = cleanup_estimate(brew).await;
    Ok(footprint)
}

/// `cellar::inventory` expects the prefix, not the Cellar directory itself.
fn cellar_prefix(prefix: &Path) -> std::path::PathBuf {
    prefix.to_path_buf()
}

/// Which version of each formula is currently linked, so the rest can be
/// counted as superseded.
async fn active_versions(brew: &Brew) -> std::collections::HashMap<String, String> {
    match crate::state::installed(brew).await {
        Ok(packages) => packages
            .into_iter()
            .filter_map(|p| Some((p.id, p.version?)))
            .collect(),
        Err(_) => std::collections::HashMap::new(),
    }
}

/// Real disk usage of a directory tree, in bytes.
///
/// Uses allocated blocks rather than file length so the number matches what
/// the filesystem actually spends, and counts each inode once so Homebrew's
/// many hard links are not double-counted.
pub fn size_of(path: &Path) -> u64 {
    use std::collections::HashSet;
    use std::os::unix::fs::MetadataExt;

    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut stack = vec![path.to_path_buf()];
    let mut total = 0u64;

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(entry.path());
                continue;
            }
            // Hard links share an inode; count the bytes once.
            if seen.insert((metadata.dev(), metadata.ino())) {
                total += metadata.blocks() * 512;
            }
        }
    }
    total
}

/// Ask Homebrew what a cleanup would free, without performing one.
async fn cleanup_estimate(brew: &Brew) -> Option<u64> {
    let output = brew.output(&["cleanup", "--dry-run"]).await.ok()?;
    output.lines().rev().find_map(parse_freed)
}

/// "==> This operation would free approximately 205.7MB of disk space."
fn parse_freed(line: &str) -> Option<u64> {
    let after = line.split("approximately").nth(1)?.trim();
    let end = after
        .find(|c: char| c.is_whitespace() || c == ',')
        .unwrap_or(after.len());
    parse_size(&after[..end])
}

/// `205.7MB` -> bytes. Homebrew uses decimal units here.
fn parse_size(raw: &str) -> Option<u64> {
    let raw = raw.trim();
    let split = raw.find(|c: char| c.is_ascii_alphabetic())?;
    let (number, unit) = raw.split_at(split);
    let value: f64 = number.trim().parse().ok()?;

    let multiplier = match unit.trim().to_ascii_uppercase().as_str() {
        "B" => 1.0,
        "KB" => 1_000.0,
        "MB" => 1_000_000.0,
        "GB" => 1_000_000_000.0,
        "TB" => 1_000_000_000_000.0,
        _ => return None,
    };
    Some((value * multiplier) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rollback::cellar::tempdir::Dir;

    #[test]
    fn parses_homebrews_cleanup_summary() {
        assert_eq!(
            parse_freed("==> This operation would free approximately 205.7MB of disk space."),
            Some(205_700_000)
        );
        assert_eq!(
            parse_freed("This operation would free approximately 1.2GB of disk space."),
            Some(1_200_000_000)
        );
        assert_eq!(parse_freed("Would remove: /some/path (12 files, 5.3MB)"), None);
        assert_eq!(parse_freed("nothing to report"), None);
    }

    #[test]
    fn parses_size_units() {
        assert_eq!(parse_size("512B"), Some(512));
        assert_eq!(parse_size("9.6MB"), Some(9_600_000));
        assert_eq!(parse_size("2.2GB"), Some(2_200_000_000));
        assert_eq!(parse_size("garbage"), None);
        assert_eq!(parse_size("12"), None);
    }

    #[test]
    fn measures_a_directory_tree() {
        let dir = Dir::new();
        let nested = dir.path().join("a/b");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("file"), vec![0u8; 8192]).unwrap();

        let measured = size_of(dir.path());
        assert!(
            measured >= 8192,
            "expected at least the written bytes, got {measured}"
        );
    }

    #[test]
    fn an_empty_or_missing_directory_measures_zero() {
        let dir = Dir::new();
        assert_eq!(size_of(&dir.path().join("does-not-exist")), 0);
        assert_eq!(size_of(dir.path()), 0);
    }

    #[test]
    fn hard_links_are_counted_once() {
        let dir = Dir::new();
        let original = dir.path().join("original");
        std::fs::write(&original, vec![0u8; 16_384]).unwrap();
        let single = size_of(dir.path());

        std::fs::hard_link(&original, dir.path().join("link")).unwrap();
        assert_eq!(
            size_of(dir.path()),
            single,
            "a hard link must not double-count the same inode"
        );
    }

    /// The real installation. The superseded total is the price own-brew is
    /// asking the user to pay for undo, so it must be truthful.
    #[tokio::test]
    async fn real_footprint_is_internally_consistent() {
        let Ok(brew) = Brew::discover() else { return };
        let footprint = footprint(&brew).await.expect("disk scan");

        assert_eq!(
            footprint.total_bytes,
            footprint.cellar_bytes + footprint.caskroom_bytes + footprint.cache_bytes
        );
        assert_eq!(
            footprint.superseded_bytes,
            footprint.superseded.iter().map(|k| k.bytes).sum::<u64>()
        );
        assert!(
            footprint.superseded_bytes <= footprint.cellar_bytes,
            "superseded kegs live inside the Cellar"
        );

        // Every superseded keg must exist and must not be the live version.
        for keg in &footprint.superseded {
            assert!(
                cellar::rack(brew.prefix(), &keg.formula)
                    .join(&keg.version)
                    .is_dir(),
                "{} {} is reported but not on disk",
                keg.formula,
                keg.version
            );
        }

        // Biggest first.
        let sizes: Vec<u64> = footprint.superseded.iter().map(|k| k.bytes).collect();
        let mut sorted = sizes.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(sizes, sorted);
    }
}
