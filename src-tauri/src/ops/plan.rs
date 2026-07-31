//! Turning a UI request into an argument list for `brew`.
//!
//! Kept pure and separate from execution so the exact command can be asserted
//! in tests — and shown to the user before anything runs.

use crate::error::{Error, Result};
use crate::model::entry::Kind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    Uninstall,
    Upgrade,
    /// Hold a formula at its current version.
    Pin,
    Unpin,
    /// Refresh Homebrew itself and its taps.
    Update,
    /// Reclaim disk: removes superseded kegs and stale downloads. This is the
    /// one operation that destroys rollback targets.
    Cleanup,
}

impl Action {
    fn verb(self) -> &'static str {
        match self {
            Action::Install => "install",
            Action::Uninstall => "uninstall",
            Action::Upgrade => "upgrade",
            Action::Pin => "pin",
            Action::Unpin => "unpin",
            Action::Update => "update",
            Action::Cleanup => "cleanup",
        }
    }

    /// `brew pin` has no cask equivalent.
    fn formula_only(self) -> bool {
        matches!(self, Action::Pin | Action::Unpin)
    }

    /// `brew upgrade` with no arguments upgrades everything, which is a
    /// legitimate request; `brew install` with none is meaningless.
    fn allows_no_targets(self) -> bool {
        matches!(self, Action::Upgrade | Action::Update | Action::Cleanup)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub action: Action,
    pub kind: Kind,
    #[serde(default)]
    pub targets: Vec<String>,
}

/// Build the argument list, rejecting anything that isn't a plausible package id.
pub fn args(request: &Request) -> Result<Vec<String>> {
    if request.targets.is_empty() && !request.action.allows_no_targets() {
        return Err(Error::Catalog(format!(
            "`brew {}` needs at least one package",
            request.action.verb()
        )));
    }

    if request.kind == Kind::Cask && request.action.formula_only() {
        return Err(Error::Catalog(format!(
            "casks cannot be {}ned; Homebrew only supports pinning formulae",
            request.action.verb()
        )));
    }

    for target in &request.targets {
        validate_id(target)?;
    }

    let mut args = vec![request.action.verb().to_owned()];

    // These take no package arguments and no --cask.
    if matches!(request.action, Action::Update | Action::Cleanup) {
        return Ok(args);
    }

    if request.kind == Kind::Cask {
        args.push("--cask".to_owned());
    }
    args.extend(request.targets.iter().cloned());
    Ok(args)
}

/// Package ids may contain letters, digits and `@ . _ + -`, and must not begin
/// with `-`.
///
/// We never invoke a shell, so there is no quoting to get wrong — but an id
/// like `--force` would still be read by `brew` as an option rather than as a
/// package, so ids are checked before they become arguments.
fn validate_id(id: &str) -> Result<()> {
    let rejected = |why: &str| Error::Catalog(format!("{id:?} is not a valid package name: {why}"));

    if id.is_empty() {
        return Err(rejected("it is empty"));
    }
    if id.starts_with('-') {
        return Err(rejected("it would be read as a command-line option"));
    }
    if id.len() > 128 {
        return Err(rejected("it is implausibly long"));
    }
    if let Some(bad) = id
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-' | '/')))
    {
        return Err(rejected(&format!("{bad:?} is not allowed")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(action: Action, kind: Kind, targets: &[&str]) -> Request {
        Request {
            action,
            kind,
            targets: targets.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn installs_a_formula() {
        let args = args(&request(Action::Install, Kind::Formula, &["jq"])).unwrap();
        assert_eq!(args, ["install", "jq"]);
    }

    #[test]
    fn installs_a_cask_with_the_cask_flag() {
        let args = args(&request(Action::Install, Kind::Cask, &["ghostty"])).unwrap();
        assert_eq!(args, ["install", "--cask", "ghostty"]);
    }

    #[test]
    fn supports_multiple_targets_in_one_command() {
        let args = args(&request(Action::Install, Kind::Formula, &["jq", "fd"])).unwrap();
        assert_eq!(args, ["install", "jq", "fd"]);
    }

    #[test]
    fn upgrade_without_targets_upgrades_everything() {
        let args = args(&request(Action::Upgrade, Kind::Formula, &[])).unwrap();
        assert_eq!(args, ["upgrade"]);
    }

    #[test]
    fn install_without_targets_is_rejected() {
        let err = args(&request(Action::Install, Kind::Formula, &[])).unwrap_err();
        assert!(err.to_string().contains("needs at least one package"));
    }

    #[test]
    fn cleanup_takes_no_package_arguments() {
        let args = args(&request(Action::Cleanup, Kind::Cask, &[])).unwrap();
        assert_eq!(args, ["cleanup"], "no --cask, no targets");
    }

    #[test]
    fn update_takes_no_package_arguments() {
        let args = args(&request(Action::Update, Kind::Cask, &[])).unwrap();
        assert_eq!(args, ["update"], "no --cask, no targets");
    }

    #[test]
    fn casks_cannot_be_pinned() {
        let err = args(&request(Action::Pin, Kind::Cask, &["ghostty"])).unwrap_err();
        assert!(err.to_string().contains("cannot be pin"));
    }

    #[test]
    fn an_id_that_looks_like_a_flag_is_rejected() {
        for hostile in ["--force", "-v", "--cask"] {
            let err = args(&request(Action::Install, Kind::Formula, &[hostile]))
                .expect_err("flags must never pass as package names");
            assert!(err.to_string().contains("command-line option"), "{hostile}");
        }
    }

    #[test]
    fn shell_metacharacters_are_rejected() {
        for hostile in [
            "jq; rm -rf /",
            "jq&&whoami",
            "jq|tee",
            "$(whoami)",
            "jq`id`",
        ] {
            assert!(
                args(&request(Action::Install, Kind::Formula, &[hostile])).is_err(),
                "{hostile} should be rejected"
            );
        }
    }

    #[test]
    fn empty_and_overlong_ids_are_rejected() {
        assert!(args(&request(Action::Install, Kind::Formula, &[""])).is_err());
        let long = "a".repeat(129);
        assert!(args(&request(Action::Install, Kind::Formula, &[&long])).is_err());
    }

    #[test]
    fn real_package_names_are_accepted() {
        // Genuine names use @ . _ + - and tap-qualified slashes.
        for good in [
            "python@3.14",
            "gcc",
            "libx11",
            "openssl@3",
            "gtk+3",
            "font-fira-code",
            "homebrew/cask/ghostty",
            "app_engine",
        ] {
            args(&request(Action::Install, Kind::Formula, &[good]))
                .unwrap_or_else(|e| panic!("{good} should be valid: {e}"));
        }
    }

    #[test]
    fn one_bad_target_rejects_the_whole_batch() {
        let err = args(&request(Action::Install, Kind::Formula, &["jq", "--force"])).unwrap_err();
        assert!(err.to_string().contains("command-line option"));
    }
}
