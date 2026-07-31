//! The process layer: everything that shells out to the real `brew` binary.
//!
//! own-brew never writes to the Cellar itself. Homebrew stays the single source
//! of truth, so our view of the system can't drift from reality.

mod env;
mod locate;
pub mod stream;

pub use locate::Installation;
pub use stream::{Line, Stream};

use crate::error::{Error, Result};
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

#[derive(Clone, Debug)]
pub struct Brew {
    binary: PathBuf,
    prefix: PathBuf,
}

impl Brew {
    /// Find Homebrew, or explain that it isn't installed.
    pub fn discover() -> Result<Self> {
        let install = locate::find()?;
        Ok(Self {
            binary: install.binary,
            prefix: install.prefix,
        })
    }

    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn prefix(&self) -> &Path {
        &self.prefix
    }

    /// Where Homebrew keeps its downloaded bottles and its API catalog cache.
    pub fn cache_dir(&self) -> Option<PathBuf> {
        env::cache_dir()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args);
        env::apply(&mut cmd, &self.prefix);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// Run a command to completion and return its stdout.
    ///
    /// Use only for commands with bounded, quick output — anything the user
    /// waits on should go through [`Brew::stream`] instead.
    pub async fn output(&self, args: &[&str]) -> Result<String> {
        let output = self.command(args).output().await?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        match output.status.code() {
            Some(code) => Err(Error::BrewFailed {
                command: args.join(" "),
                code,
                stderr,
            }),
            None => Err(Error::BrewTerminated {
                command: args.join(" "),
            }),
        }
    }

    /// Run a command and deserialize its JSON stdout.
    pub async fn json<T: DeserializeOwned>(&self, args: &[&str]) -> Result<T> {
        let raw = self.output(args).await?;
        serde_json::from_str(&raw).map_err(|source| Error::Parse {
            command: args.join(" "),
            source,
        })
    }

    /// Spawn a long-running command whose output is streamed line by line.
    pub fn stream(&self, args: &[&str]) -> Result<Stream> {
        Stream::spawn(self.command(args), args.join(" "))
    }

    /// Homebrew's own version string, which doubles as a liveness check.
    pub async fn version(&self) -> Result<String> {
        let raw = self.output(&["--version"]).await?;
        Ok(raw
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .trim_start_matches("Homebrew ")
            .to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_reports_a_usable_installation() {
        // The dev machine has Homebrew; if it ever doesn't, the error must be
        // the specific, actionable one rather than a panic.
        match Brew::discover() {
            Ok(brew) => {
                assert!(brew.binary().is_file(), "binary should exist on disk");
                assert!(brew.prefix().is_dir(), "prefix should be a directory");
            }
            Err(e) => assert_eq!(e.kind(), "brew_not_found"),
        }
    }

    #[tokio::test]
    async fn version_parses() {
        let Ok(brew) = Brew::discover() else { return };
        let version = brew.version().await.expect("brew --version");
        assert!(
            version.chars().next().is_some_and(|c| c.is_ascii_digit()),
            "expected a version number, got {version:?}"
        );
    }

    #[tokio::test]
    async fn failed_command_surfaces_exit_code_and_stderr() {
        let Ok(brew) = Brew::discover() else { return };
        let err = brew
            .output(&["info", "--json=v2", "definitely-not-a-real-formula-xyz"])
            .await
            .expect_err("unknown formula should fail");
        assert_eq!(err.kind(), "brew_failed");
        assert!(err.detail().is_some(), "stderr should be captured");
    }
}
