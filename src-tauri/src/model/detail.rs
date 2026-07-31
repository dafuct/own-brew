//! Full package detail, as returned by `brew info --json=v2`.
//!
//! This is the authoritative view: unlike the published catalog dump it also
//! reports what is installed locally, which keg is linked, and whether the
//! package is pinned or outdated.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `brew info --json=v2` always returns both buckets, whatever was asked for.
#[derive(Debug, Default, Deserialize)]
pub struct Info {
    #[serde(default)]
    pub formulae: Vec<Formula>,
    #[serde(default)]
    pub casks: Vec<Cask>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Detail {
    Formula(Box<Formula>),
    Cask(Box<Cask>),
}

// ---------------------------------------------------------------- formula ---

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Formula {
    pub name: String,
    #[serde(default)]
    pub full_name: String,
    #[serde(default)]
    pub tap: Option<String>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub versions: FormulaVersions,

    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub build_dependencies: Vec<String>,
    #[serde(default)]
    pub recommended_dependencies: Vec<String>,
    #[serde(default)]
    pub optional_dependencies: Vec<String>,
    #[serde(default)]
    pub uses_from_macos: Vec<MacosDependency>,

    #[serde(default)]
    pub conflicts_with: Vec<String>,
    #[serde(default)]
    pub conflicts_with_reasons: Vec<Option<String>>,

    #[serde(default)]
    pub keg_only: bool,
    #[serde(default)]
    pub keg_only_reason: Option<KegOnlyReason>,
    #[serde(default)]
    pub caveats: Option<String>,

    #[serde(default)]
    pub installed: Vec<InstalledKeg>,
    #[serde(default)]
    pub linked_keg: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub outdated: bool,

    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub deprecation_date: Option<String>,
    #[serde(default)]
    pub deprecation_reason: Option<String>,
    #[serde(default)]
    pub deprecation_replacement_formula: Option<String>,
    #[serde(default)]
    pub deprecation_replacement_cask: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub disable_date: Option<String>,
    #[serde(default)]
    pub disable_reason: Option<String>,
    #[serde(default)]
    pub disable_replacement_formula: Option<String>,
    #[serde(default)]
    pub disable_replacement_cask: Option<String>,

    #[serde(default)]
    pub versioned_formulae: Vec<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

impl Formula {
    /// The version currently linked into the prefix, if any.
    pub fn active_version(&self) -> Option<&str> {
        self.linked_keg
            .as_deref()
            .or_else(|| self.installed.last().map(|k| k.version.as_str()))
    }

    pub fn is_installed(&self) -> bool {
        !self.installed.is_empty()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct FormulaVersions {
    #[serde(default)]
    pub stable: Option<String>,
    #[serde(default)]
    pub head: Option<String>,
    #[serde(default)]
    pub bottle: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KegOnlyReason {
    #[serde(default)]
    pub reason: String,
    #[serde(default)]
    pub explanation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstalledKeg {
    pub version: String,
    #[serde(default)]
    pub installed_as_dependency: bool,
    #[serde(default)]
    pub installed_on_request: bool,
    #[serde(default)]
    pub poured_from_bottle: bool,
    /// Unix seconds; absent on very old install receipts.
    #[serde(default)]
    pub time: Option<i64>,
    #[serde(default)]
    pub runtime_dependencies: Vec<RuntimeDependency>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuntimeDependency {
    pub full_name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub declared_directly: bool,
}

/// `uses_from_macos` mixes bare names with `{"name": {"since": "..."}}` maps.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum MacosDependency {
    Name(String),
    Conditional(Value),
}

impl MacosDependency {
    pub fn name(&self) -> Option<&str> {
        match self {
            MacosDependency::Name(n) => Some(n),
            MacosDependency::Conditional(v) => v.as_object()?.keys().next().map(String::as_str),
        }
    }
}

// ------------------------------------------------------------------- cask ---

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Cask {
    pub token: String,
    #[serde(default)]
    pub full_token: String,
    #[serde(default)]
    pub tap: Option<String>,
    #[serde(default)]
    pub name: Vec<String>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub version: Option<String>,

    /// The installed version, or `None` when the cask is not installed.
    #[serde(default)]
    pub installed: Option<String>,
    /// Unix seconds.
    #[serde(default)]
    pub installed_time: Option<i64>,
    #[serde(default)]
    pub outdated: bool,

    /// When true the app updates itself, so Homebrew's version can lag
    /// harmlessly behind what is actually on disk.
    #[serde(default)]
    pub auto_updates: Option<bool>,

    #[serde(default)]
    pub depends_on: CaskDependsOn,
    #[serde(default)]
    pub conflicts_with: Option<CaskConflicts>,
    #[serde(default)]
    pub caveats: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<Value>,

    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub deprecation_date: Option<String>,
    #[serde(default)]
    pub deprecation_reason: Option<String>,
    #[serde(default)]
    pub deprecation_replacement_cask: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub disable_date: Option<String>,
    #[serde(default)]
    pub disable_reason: Option<String>,

    #[serde(default)]
    pub old_tokens: Vec<String>,
}

impl Cask {
    pub fn display_name(&self) -> &str {
        self.name
            .iter()
            .find(|n| !n.is_empty())
            .map(String::as_str)
            .unwrap_or(&self.token)
    }

    pub fn is_installed(&self) -> bool {
        self.installed.is_some()
    }

    /// The `.app` bundles this cask places in `/Applications`, for showing the
    /// user what will actually appear on their machine.
    pub fn app_bundles(&self) -> Vec<String> {
        self.artifacts
            .iter()
            .filter_map(|artifact| artifact.get("app")?.as_array())
            .flatten()
            .filter_map(|app| app.as_str())
            .map(str::to_owned)
            .collect()
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CaskDependsOn {
    #[serde(default)]
    pub cask: Vec<String>,
    #[serde(default)]
    pub formula: Vec<String>,
    /// `{">=": ["13"]}` and similar; kept raw because the operators vary.
    #[serde(default)]
    pub macos: Option<Value>,
    #[serde(default)]
    pub arch: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CaskConflicts {
    #[serde(default)]
    pub cask: Vec<String>,
    #[serde(default)]
    pub formula: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMULAE: &str = include_str!("../../tests/fixtures/info_formulae.json");
    const CASKS: &str = include_str!("../../tests/fixtures/info_casks.json");

    fn formulae() -> Vec<Formula> {
        serde_json::from_str::<Info>(FORMULAE)
            .expect("real `brew info --json=v2` output must parse")
            .formulae
    }

    fn casks() -> Vec<Cask> {
        serde_json::from_str::<Info>(CASKS)
            .expect("real `brew info --json=v2 --cask` output must parse")
            .casks
    }

    #[test]
    fn parses_real_formula_output() {
        let all = formulae();
        let jq = all.iter().find(|f| f.name == "jq").expect("jq present");
        assert_eq!(jq.versions.stable.as_deref(), Some("1.8.2"));
        assert!(jq.is_installed());
        assert_eq!(jq.active_version(), Some("1.8.2"));
        assert!(jq.dependencies.contains(&"oniguruma".to_owned()));
    }

    #[test]
    fn runtime_dependencies_carry_exact_versions() {
        // This is what makes a snapshot reproducible rather than approximate.
        let all = formulae();
        let jq = all.iter().find(|f| f.name == "jq").unwrap();
        let keg = jq.installed.last().expect("an installed keg");
        let oniguruma = keg
            .runtime_dependencies
            .iter()
            .find(|d| d.full_name == "oniguruma")
            .expect("oniguruma recorded");
        assert!(oniguruma.version.is_some());
    }

    #[test]
    fn reports_every_keg_the_receipts_mention() {
        // These are install receipts, not disk contents — a keg listed here
        // may already have been removed, which is why rollback targets come
        // from the Cellar instead. See `state::installed`.
        let all = formulae();
        let python = all
            .iter()
            .find(|f| f.name.starts_with("python@"))
            .expect("python fixture");
        assert!(!python.installed.is_empty());
        assert_eq!(python.active_version(), python.linked_keg.as_deref());
    }

    #[test]
    fn parses_real_cask_output() {
        let all = casks();
        let ghostty = all
            .iter()
            .find(|c| c.token == "ghostty")
            .expect("ghostty present");
        assert_eq!(ghostty.display_name(), "Ghostty");
        assert!(ghostty.is_installed());
        assert_eq!(ghostty.auto_updates, Some(true));
        assert_eq!(ghostty.app_bundles(), vec!["Ghostty.app".to_owned()]);
    }

    #[test]
    fn cask_dependencies_and_conflicts_survive_parsing() {
        let all = casks();
        let ghostty = all.iter().find(|c| c.token == "ghostty").unwrap();
        assert!(ghostty.depends_on.macos.is_some());
        assert_eq!(
            ghostty.conflicts_with.as_ref().map(|c| c.cask.as_slice()),
            Some(["ghostty@tip".to_owned()].as_slice())
        );
    }

    #[test]
    fn uses_from_macos_accepts_both_shapes() {
        let bare: MacosDependency = serde_json::from_str(r#""zlib""#).unwrap();
        assert_eq!(bare.name(), Some("zlib"));
        let conditional: MacosDependency =
            serde_json::from_str(r#"{"curl":{"since":"monterey"}}"#).unwrap();
        assert_eq!(conditional.name(), Some("curl"));
    }
}
