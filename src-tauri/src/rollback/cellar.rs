//! What is genuinely on disk.
//!
//! `brew info --json=v2` cannot be trusted for this. On a real machine it
//! reported `sdl2-compat 2.32.10` among a formula's installed kegs while that
//! keg directory no longer existed — Homebrew's install receipts outlive the
//! kegs they describe. Offering a rollback target that isn't there would make
//! the feature lie, so every candidate is confirmed against the filesystem.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Directory holding all versions of one formula: `<prefix>/Cellar/<name>`.
pub fn rack(prefix: &Path, name: &str) -> PathBuf {
    prefix.join("Cellar").join(name)
}

/// Versions of `name` that actually exist as keg directories, sorted oldest
/// first. A keg is only counted when it has real content — Homebrew leaves
/// empty rack directories behind after some uninstalls.
pub fn kegs(prefix: &Path, name: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(rack(prefix, name)) else {
        return Vec::new();
    };

    let mut versions: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| is_populated(&entry.path()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.starts_with('.'))
        .collect();

    versions
        .sort_by(|a, b| crate::history::diff::compare_versions(a, b).unwrap_or_else(|| a.cmp(b)));
    versions
}

/// Does a keg directory contain anything? An empty directory is a leftover,
/// not a restorable version.
fn is_populated(keg: &Path) -> bool {
    std::fs::read_dir(keg)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

/// Every rack on the machine and the versions it holds.
///
/// Read once per refresh so the installed list can be annotated without a
/// directory scan per package.
pub fn inventory(prefix: &Path) -> BTreeMap<String, Vec<String>> {
    let cellar = prefix.join("Cellar");
    let Ok(entries) = std::fs::read_dir(&cellar) else {
        return BTreeMap::new();
    };

    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            let versions = kegs(prefix, &name);
            (!versions.is_empty()).then_some((name, versions))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> tempdir::Dir {
        tempdir::Dir::new()
    }

    #[test]
    fn lists_only_directories_that_contain_something() {
        let dir = scratch();
        let prefix = dir.path();

        std::fs::create_dir_all(rack(prefix, "jq").join("1.8.1/bin")).unwrap();
        std::fs::write(rack(prefix, "jq").join("1.8.1/bin/jq"), "binary").unwrap();
        // An empty leftover directory must not be offered as restorable.
        std::fs::create_dir_all(rack(prefix, "jq").join("1.8.0")).unwrap();

        assert_eq!(kegs(prefix, "jq"), vec!["1.8.1".to_owned()]);
    }

    #[test]
    fn a_missing_rack_yields_nothing_rather_than_failing() {
        let dir = scratch();
        assert!(kegs(dir.path(), "never-installed").is_empty());
    }

    #[test]
    fn versions_are_ordered_oldest_first() {
        let dir = scratch();
        let prefix = dir.path();
        for version in ["1.10.0", "1.9.0", "1.8.1"] {
            let keg = rack(prefix, "jq").join(version);
            std::fs::create_dir_all(&keg).unwrap();
            std::fs::write(keg.join("marker"), "x").unwrap();
        }
        assert_eq!(kegs(prefix, "jq"), vec!["1.8.1", "1.9.0", "1.10.0"]);
    }

    #[test]
    fn inventory_covers_every_populated_rack() {
        let dir = scratch();
        let prefix = dir.path();
        for (name, version) in [("jq", "1.8.2"), ("fd", "10.4.2")] {
            let keg = rack(prefix, name).join(version);
            std::fs::create_dir_all(&keg).unwrap();
            std::fs::write(keg.join("marker"), "x").unwrap();
        }
        std::fs::create_dir_all(rack(prefix, "empty-rack")).unwrap();

        let inventory = inventory(prefix);
        assert_eq!(inventory.len(), 2);
        assert_eq!(inventory["jq"], vec!["1.8.2".to_owned()]);
        assert!(!inventory.contains_key("empty-rack"));
    }

    /// The real Cellar: guards the assumption the whole feature rests on.
    #[test]
    fn real_cellar_agrees_with_the_filesystem() {
        let Ok(brew) = crate::brew::Brew::discover() else {
            return;
        };
        let inventory = inventory(brew.prefix());
        for (name, versions) in inventory.iter().take(20) {
            for version in versions {
                assert!(
                    rack(brew.prefix(), name).join(version).is_dir(),
                    "{name} {version} was reported but is not on disk"
                );
            }
        }
    }
}

/// A minimal scratch directory helper, so tests never touch the real Cellar.
#[cfg(test)]
pub mod tempdir {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    pub struct Dir(PathBuf);

    impl Default for Dir {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Dir {
        pub fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path =
                std::env::temp_dir().join(format!("own-brew-test-{}-{unique}", std::process::id()));
            std::fs::create_dir_all(&path).expect("scratch directory");
            Self(path)
        }

        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
