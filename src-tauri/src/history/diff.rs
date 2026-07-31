//! Working out what an operation actually changed.
//!
//! Homebrew reports what it did in prose, not in data. Rather than parse that,
//! own-brew compares the installed set before and after — which is accurate
//! even when an operation touched packages the user never named, such as
//! dependencies pulled in by an install.

use crate::model::entry::Kind;
use crate::state::InstalledPackage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Installed,
    Removed,
    Upgraded,
    Downgraded,
    /// Versions differ but neither is comparable as semver.
    Changed,
}

impl ChangeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChangeKind::Installed => "installed",
            ChangeKind::Removed => "removed",
            ChangeKind::Upgraded => "upgraded",
            ChangeKind::Downgraded => "downgraded",
            ChangeKind::Changed => "changed",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "installed" => ChangeKind::Installed,
            "removed" => ChangeKind::Removed,
            "upgraded" => ChangeKind::Upgraded,
            "downgraded" => ChangeKind::Downgraded,
            _ => ChangeKind::Changed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub kind: Kind,
    pub package: String,
    pub before_version: Option<String>,
    pub after_version: Option<String>,
    pub change: ChangeKind,
}

/// Compare two installed sets and report what moved.
///
/// Results are sorted by package name so a rendered history entry is stable.
pub fn diff(before: &[InstalledPackage], after: &[InstalledPackage]) -> Vec<Change> {
    let index = |packages: &[InstalledPackage]| -> HashMap<(Kind, String), Option<String>> {
        packages
            .iter()
            .map(|p| ((p.kind, p.id.clone()), p.version.clone()))
            .collect()
    };

    let before_index = index(before);
    let after_index = index(after);
    let mut changes = Vec::new();

    for ((kind, package), after_version) in &after_index {
        match before_index.get(&(*kind, package.clone())) {
            None => changes.push(Change {
                kind: *kind,
                package: package.clone(),
                before_version: None,
                after_version: after_version.clone(),
                change: ChangeKind::Installed,
            }),
            Some(before_version) if before_version != after_version => changes.push(Change {
                kind: *kind,
                package: package.clone(),
                before_version: before_version.clone(),
                after_version: after_version.clone(),
                change: direction(before_version.as_deref(), after_version.as_deref()),
            }),
            Some(_) => {}
        }
    }

    for ((kind, package), before_version) in &before_index {
        if !after_index.contains_key(&(*kind, package.clone())) {
            changes.push(Change {
                kind: *kind,
                package: package.clone(),
                before_version: before_version.clone(),
                after_version: None,
                change: ChangeKind::Removed,
            });
        }
    }

    changes.sort_by(|a, b| a.package.cmp(&b.package).then(a.kind.as_str().cmp(b.kind.as_str())));
    changes
}

/// Which way a version moved.
///
/// Homebrew versions are frequently not valid semver (`2026-07-16`,
/// `1.16.2_2`, `3.14.6`), so a lenient numeric comparison is used and anything
/// undecidable is reported as a plain change rather than guessed at.
fn direction(before: Option<&str>, after: Option<&str>) -> ChangeKind {
    let (Some(before), Some(after)) = (before, after) else {
        return ChangeKind::Changed;
    };
    match compare_versions(before, after) {
        Some(std::cmp::Ordering::Less) => ChangeKind::Upgraded,
        Some(std::cmp::Ordering::Greater) => ChangeKind::Downgraded,
        Some(std::cmp::Ordering::Equal) | None => ChangeKind::Changed,
    }
}

/// Compare version strings by their numeric components, left to right.
pub fn compare_versions(a: &str, b: &str) -> Option<std::cmp::Ordering> {
    let parts = |v: &str| -> Vec<u64> {
        v.split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect()
    };

    let (left, right) = (parts(a), parts(b));
    if left.is_empty() || right.is_empty() {
        return None;
    }

    for index in 0..left.len().max(right.len()) {
        let l = left.get(index).copied().unwrap_or(0);
        let r = right.get(index).copied().unwrap_or(0);
        if l != r {
            return Some(l.cmp(&r));
        }
    }
    Some(std::cmp::Ordering::Equal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(id: &str, version: Option<&str>) -> InstalledPackage {
        InstalledPackage {
            kind: Kind::Formula,
            id: id.to_owned(),
            name: id.to_owned(),
            desc: None,
            version: version.map(str::to_owned),
            outdated: false,
            pinned: false,
            installed_on_request: true,
            installed_at: None,
            rollback_targets: Vec::new(),
            self_updating: false,
        }
    }

    #[test]
    fn detects_an_upgrade() {
        let changes = diff(&[pkg("jq", Some("1.8.1"))], &[pkg("jq", Some("1.8.2"))]);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].change, ChangeKind::Upgraded);
        assert_eq!(changes[0].before_version.as_deref(), Some("1.8.1"));
        assert_eq!(changes[0].after_version.as_deref(), Some("1.8.2"));
    }

    #[test]
    fn detects_a_downgrade() {
        let changes = diff(&[pkg("jq", Some("1.8.2"))], &[pkg("jq", Some("1.8.1"))]);
        assert_eq!(changes[0].change, ChangeKind::Downgraded);
    }

    #[test]
    fn detects_installs_and_removals() {
        let changes = diff(&[pkg("old", Some("1"))], &[pkg("new", Some("2"))]);
        let by_name: HashMap<_, _> = changes.iter().map(|c| (c.package.as_str(), c.change)).collect();
        assert_eq!(by_name["new"], ChangeKind::Installed);
        assert_eq!(by_name["old"], ChangeKind::Removed);
    }

    #[test]
    fn unchanged_packages_are_not_reported() {
        let state = vec![pkg("jq", Some("1.8.2")), pkg("fd", Some("10.4.2"))];
        assert!(diff(&state, &state).is_empty());
    }

    #[test]
    fn catches_dependencies_the_user_never_named() {
        // Installing ffmpeg drags in dozens of libraries; history should show them.
        let after = vec![pkg("ffmpeg", Some("8.1.2")), pkg("dav1d", Some("1.5.1"))];
        let changes = diff(&[], &after);
        assert_eq!(changes.len(), 2);
        assert!(changes.iter().all(|c| c.change == ChangeKind::Installed));
    }

    #[test]
    fn a_cask_and_a_formula_of_the_same_name_are_distinct() {
        let mut cask = pkg("docker", Some("4.0"));
        cask.kind = Kind::Cask;
        let before = vec![pkg("docker", Some("1.0")), cask.clone()];

        let mut cask_after = cask;
        cask_after.version = Some("4.1".into());
        let after = vec![pkg("docker", Some("1.0")), cask_after];

        let changes = diff(&before, &after);
        assert_eq!(changes.len(), 1, "only the cask changed");
        assert_eq!(changes[0].kind, Kind::Cask);
    }

    #[test]
    fn compares_homebrew_style_versions() {
        use std::cmp::Ordering;
        assert_eq!(compare_versions("1.8.1", "1.8.2"), Some(Ordering::Less));
        assert_eq!(compare_versions("1.10.0", "1.9.0"), Some(Ordering::Greater));
        // Revision suffixes participate in the comparison.
        assert_eq!(compare_versions("1.16.2_1", "1.16.2_2"), Some(Ordering::Less));
        // Date-style versions, as used by ca-certificates.
        assert_eq!(
            compare_versions("2026-05-14", "2026-07-16"),
            Some(Ordering::Less)
        );
        // Missing trailing components count as zero.
        assert_eq!(compare_versions("3.14", "3.14.0"), Some(Ordering::Equal));
    }

    #[test]
    fn undecidable_versions_are_reported_without_a_direction() {
        assert_eq!(compare_versions("HEAD", "latest"), None);
        let changes = diff(&[pkg("x", Some("HEAD"))], &[pkg("x", Some("latest"))]);
        assert_eq!(changes[0].change, ChangeKind::Changed);
    }

    #[test]
    fn results_are_sorted_for_stable_rendering() {
        let before = vec![pkg("zstd", Some("1")), pkg("abc", Some("1"))];
        let after = vec![pkg("zstd", Some("2")), pkg("abc", Some("2"))];
        let changes = diff(&before, &after);
        assert_eq!(changes[0].package, "abc");
        assert_eq!(changes[1].package, "zstd");
    }
}
