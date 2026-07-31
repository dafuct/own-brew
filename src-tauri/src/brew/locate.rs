use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Standard install locations, in the order Homebrew itself documents them.
const WELL_KNOWN: &[&str] = &[
    "/opt/homebrew/bin/brew",              // Apple Silicon
    "/usr/local/bin/brew",                 // Intel macOS
    "/home/linuxbrew/.linuxbrew/bin/brew", // Linux
];

#[derive(Clone, Debug)]
pub struct Installation {
    pub binary: PathBuf,
    pub prefix: PathBuf,
}

/// Locate Homebrew.
///
/// A GUI app launched from Finder inherits a minimal `PATH` that usually
/// excludes Homebrew, so probing well-known locations comes first and `PATH`
/// is only a fallback for unusual prefixes.
pub fn find() -> Result<Installation> {
    if let Some(prefix) = std::env::var_os("HOMEBREW_PREFIX").map(PathBuf::from) {
        let binary = prefix.join("bin/brew");
        if binary.is_file() {
            return Ok(Installation { binary, prefix });
        }
    }

    for candidate in WELL_KNOWN {
        let binary = Path::new(candidate);
        if binary.is_file() {
            return Ok(Installation {
                binary: binary.to_path_buf(),
                prefix: prefix_of(binary),
            });
        }
    }

    if let Ok(binary) = which::which("brew") {
        let prefix = prefix_of(&binary);
        return Ok(Installation { binary, prefix });
    }

    Err(Error::BrewNotFound)
}

/// `<prefix>/bin/brew` -> `<prefix>`.
fn prefix_of(binary: &Path) -> PathBuf {
    binary
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_is_two_levels_up_from_the_binary() {
        assert_eq!(
            prefix_of(Path::new("/opt/homebrew/bin/brew")),
            PathBuf::from("/opt/homebrew")
        );
        assert_eq!(
            prefix_of(Path::new("/home/linuxbrew/.linuxbrew/bin/brew")),
            PathBuf::from("/home/linuxbrew/.linuxbrew")
        );
    }

    #[test]
    fn degenerate_paths_do_not_panic() {
        assert_eq!(prefix_of(Path::new("brew")), PathBuf::from("/"));
    }
}
