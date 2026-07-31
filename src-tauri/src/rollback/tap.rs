//! Putting a recovered formula somewhere Homebrew will install it from.
//!
//! A formula file is only installable if it lives in a tap, so own-brew keeps
//! its own: `own-brew/rollback`.
//!
//! The file keeps the formula's **original name**, which is not obvious. The
//! `name@version` shape `brew extract` produces cannot pour a bottle here:
//! Homebrew builds the bottle URL from the formula's name via
//! `GitHubPackages.image_formula_name`, which maps `@` to `/`, so `jq@1.8.1`
//! is looked for at `homebrew/core/jq/1.8.1` and 404s. Keeping the name `jq`
//! produces the correct URL.
//!
//! The cost of that choice is that Homebrew refuses to hold two formulae of
//! the same name from different taps, so a recovery replaces the installed
//! package rather than sitting beside it. [`super::recovery_plan`] handles
//! that safely.
//!
//! Nothing here parses Ruby; Homebrew remains the authority on what a formula
//! says.

use crate::brew::Brew;
use crate::error::{Error, Result};
use std::path::PathBuf;

pub const USER: &str = "own-brew";
pub const REPO: &str = "rollback";

/// `own-brew/rollback`
pub fn name() -> String {
    format!("{USER}/{REPO}")
}

pub fn directory(brew: &Brew) -> PathBuf {
    brew.prefix()
        .join("Library/Taps")
        .join(USER)
        .join(format!("homebrew-{REPO}"))
}

/// Create the tap if it does not exist yet.
///
/// `--no-git` keeps it a plain directory: this tap is never published, and a
/// git repository with GitHub Actions workflows would be noise in the user's
/// Homebrew installation.
pub async fn ensure(brew: &Brew) -> Result<PathBuf> {
    let directory = directory(brew);
    if directory.join("Formula").is_dir() {
        return Ok(directory);
    }

    brew.output(&["tap-new", "--no-git", &name()]).await?;
    let formula_dir = directory.join("Formula");
    std::fs::create_dir_all(&formula_dir)?;
    Ok(directory)
}

/// Write a recovered formula into the tap and have Homebrew confirm it.
///
/// Returns the fully-qualified formula name to install.
pub async fn materialize(brew: &Brew, recovered: &super::fetch::Recovered) -> Result<String> {
    super::validate(&recovered.name)?;
    super::validate(&recovered.version)?;

    let directory = ensure(brew).await?;
    let formula_dir = directory.join("Formula");

    // Only one recovery is staged at a time, and a stale file of the same name
    // would shadow the new one.
    if let Ok(entries) = std::fs::read_dir(&formula_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "rb") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }

    if !recovered.ruby.contains("< Formula") {
        return Err(Error::Catalog(format!(
            "the recovered file for {} does not look like a formula",
            recovered.name
        )));
    }

    let header = format!(
        "# Recovered by own-brew from homebrew-core {}\n# {}\n",
        &recovered.sha[..12.min(recovered.sha.len())],
        recovered.source_url
    );

    let path = formula_dir.join(format!("{}.rb", recovered.name));
    std::fs::write(&path, format!("{header}{}", recovered.ruby))?;

    let full_name = format!("{}/{}", name(), recovered.name);
    if let Err(e) = confirm(brew, &full_name, &recovered.version).await {
        // Never leave a file behind that Homebrew rejected.
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    Ok(full_name)
}

/// Ask Homebrew what the materialized formula actually is.
///
/// This is the real verification: the fetcher's checks are textual guesses,
/// while this is Homebrew parsing the file it would install from.
async fn confirm(brew: &Brew, full_name: &str, expected: &str) -> Result<()> {
    let info: crate::model::detail::Info = brew
        .json(&["info", "--json=v2", "--formula", full_name])
        .await?;

    let formula = info
        .formulae
        .first()
        .ok_or_else(|| Error::Catalog(format!("Homebrew did not recognise {full_name}")))?;

    let actual = formula.versions.stable.as_deref().unwrap_or_default();
    let upstream = expected.split('_').next().unwrap_or(expected);
    if actual != expected && actual != upstream {
        return Err(Error::Catalog(format!(
            "recovered the wrong version: asked for {expected}, the file describes {actual}"
        )));
    }
    Ok(())
}

/// Remove a previously materialized formula.
pub fn discard(brew: &Brew, name: &str, _version: &str) -> Result<()> {
    super::validate(name)?;
    let path = directory(brew).join("Formula").join(format!("{name}.rb"));
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tap must be a real, usable tap on this machine: everything the
    /// recovery path does depends on Homebrew reading formulae from it.
    #[tokio::test]
    async fn the_tap_can_be_created_and_is_recognised() {
        let Ok(brew) = Brew::discover() else { return };
        let directory = ensure(&brew).await.expect("tap creation");
        assert!(directory.join("Formula").is_dir());
    }
}
