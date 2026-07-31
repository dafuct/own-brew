//! The environment own-brew runs Homebrew in.
//!
//! Two things make this non-obvious:
//!
//! 1. An app launched from Finder gets a minimal `PATH` (`/usr/bin:/bin:...`)
//!    that excludes Homebrew's own prefix, so `brew` would fail to find the
//!    tools it shells out to. We rebuild `PATH` explicitly.
//! 2. Homebrew's defaults are tuned for an interactive terminal. We turn off
//!    colour, emoji and hints so output is parseable, and turn off the implicit
//!    `auto-update` so a click on "Install" does exactly one thing.

use std::path::{Path, PathBuf};
use tokio::process::Command;

pub fn apply(cmd: &mut Command, prefix: &Path) {
    cmd.env("PATH", path_for(prefix));

    // Machine-readable output.
    cmd.env("HOMEBREW_NO_COLOR", "1");
    cmd.env("HOMEBREW_NO_EMOJI", "1");
    cmd.env("HOMEBREW_NO_ENV_HINTS", "1");

    // Predictability: the user asked to install one thing, not to also sync
    // every tap first. own-brew exposes updating as its own explicit action.
    cmd.env("HOMEBREW_NO_AUTO_UPDATE", "1");

    // Keep superseded kegs on disk. Homebrew otherwise prunes them periodically,
    // and those kegs are what make rolling back an upgrade instant and offline.
    // own-brew takes over reclaiming that space as a deliberate, visible action.
    cmd.env("HOMEBREW_NO_INSTALL_CLEANUP", "1");

    // Deliberately NOT set: HOMEBREW_NO_ANALYTICS. Whether to send analytics is
    // the user's decision, already recorded in their Homebrew config; a GUI
    // should not silently override it in either direction.
}

/// Homebrew's prefix first, then the standard system directories.
fn path_for(prefix: &Path) -> String {
    let mut parts: Vec<PathBuf> = vec![prefix.join("bin"), prefix.join("sbin")];

    for system in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
        parts.push(PathBuf::from(system));
    }

    if let Some(inherited) = std::env::var_os("PATH") {
        for entry in std::env::split_paths(&inherited) {
            if !parts.contains(&entry) {
                parts.push(entry);
            }
        }
    }

    std::env::join_paths(parts)
        .map(|joined| joined.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "/usr/bin:/bin".to_owned())
}

/// Homebrew's cache directory, where bottles and the API catalog live.
pub fn cache_dir() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("HOMEBREW_CACHE") {
        return Some(PathBuf::from(explicit));
    }

    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    if cfg!(target_os = "macos") {
        Some(home.join("Library/Caches/Homebrew"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| Some(home.join(".cache")))
            .map(|base| base.join("Homebrew"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_leads_with_the_homebrew_prefix() {
        let path = path_for(Path::new("/opt/homebrew"));
        let first: Vec<_> = std::env::split_paths(&path).take(2).collect();
        assert_eq!(first[0], PathBuf::from("/opt/homebrew/bin"));
        assert_eq!(first[1], PathBuf::from("/opt/homebrew/sbin"));
    }

    #[test]
    fn path_always_contains_the_system_directories() {
        let path = path_for(Path::new("/opt/homebrew"));
        let entries: Vec<_> = std::env::split_paths(&path).collect();
        for system in ["/usr/bin", "/bin", "/usr/sbin", "/sbin"] {
            assert!(
                entries.contains(&PathBuf::from(system)),
                "{system} missing from PATH"
            );
        }
    }

    #[test]
    fn path_has_no_duplicates() {
        let path = path_for(Path::new("/opt/homebrew"));
        let entries: Vec<_> = std::env::split_paths(&path).collect();
        let mut unique = entries.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(entries.len(), unique.len(), "PATH should not repeat entries");
    }
}
