//! What is actually installed on this machine.
//!
//! Derived entirely from `brew info --json=v2 --installed`, so it cannot drift
//! from what Homebrew believes.

use crate::brew::Brew;
use crate::error::Result;
use crate::model::detail::{Cask, Formula, Info};
use crate::model::entry::Kind;
use crate::model::{Outdated, Service};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPackage {
    pub kind: Kind,
    pub id: String,
    pub name: String,
    pub desc: Option<String>,
    /// The version currently in use.
    pub version: Option<String>,
    pub outdated: bool,
    pub pinned: bool,
    /// False when this only exists because something else depends on it.
    pub installed_on_request: bool,
    /// Unix seconds of the most recent install.
    pub installed_at: Option<i64>,
    /// Superseded versions still on disk — instant, offline rollback targets.
    pub rollback_targets: Vec<String>,
    /// Casks that update themselves; their Homebrew version lags harmlessly.
    pub self_updating: bool,
}

impl From<&Formula> for InstalledPackage {
    fn from(f: &Formula) -> Self {
        let newest = f.installed.last();
        Self {
            kind: Kind::Formula,
            id: f.name.clone(),
            name: f.name.clone(),
            desc: f.desc.clone(),
            version: f.active_version().map(str::to_owned),
            outdated: f.outdated,
            pinned: f.pinned,
            installed_on_request: newest.is_some_and(|k| k.installed_on_request),
            installed_at: newest.and_then(|k| k.time),
            rollback_targets: f
                .rollback_candidates()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            self_updating: false,
        }
    }
}

impl From<&Cask> for InstalledPackage {
    fn from(c: &Cask) -> Self {
        Self {
            kind: Kind::Cask,
            id: c.token.clone(),
            name: c.display_name().to_owned(),
            desc: c.desc.clone(),
            version: c.installed.clone(),
            outdated: c.outdated,
            pinned: false,
            // Casks are always installed deliberately; nothing depends on them.
            installed_on_request: true,
            installed_at: c.installed_time,
            // A cask keeps only one version on disk, so there is no local
            // rollback target; recovering an old version needs the download
            // cache or the upstream URL.
            rollback_targets: Vec::new(),
            self_updating: c.auto_updates.unwrap_or(false),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub formulae: usize,
    pub casks: usize,
    /// Installed deliberately, as opposed to pulled in as a dependency.
    pub requested: usize,
    pub outdated: usize,
    pub pinned: usize,
}

pub async fn installed(brew: &Brew) -> Result<Vec<InstalledPackage>> {
    let info: Info = brew.json(&["info", "--json=v2", "--installed"]).await?;

    let mut packages: Vec<InstalledPackage> = info
        .formulae
        .iter()
        .map(InstalledPackage::from)
        .chain(info.casks.iter().map(InstalledPackage::from))
        .collect();

    packages.sort_by_key(|p| p.name.to_lowercase());
    Ok(packages)
}

pub fn summarize(packages: &[InstalledPackage]) -> Summary {
    Summary {
        formulae: packages.iter().filter(|p| p.kind == Kind::Formula).count(),
        casks: packages.iter().filter(|p| p.kind == Kind::Cask).count(),
        requested: packages.iter().filter(|p| p.installed_on_request).count(),
        outdated: packages.iter().filter(|p| p.outdated).count(),
        pinned: packages.iter().filter(|p| p.pinned).count(),
    }
}

/// What `brew upgrade` would actually change.
///
/// Deliberately not `--greedy-auto-updates`: casks that update themselves are
/// reported as outdated by that flag but are left alone by `brew upgrade`, so
/// including them would show the user a count they cannot act on.
pub async fn outdated(brew: &Brew) -> Result<Outdated> {
    brew.json(&["outdated", "--json=v2"]).await
}

pub async fn services(brew: &Brew) -> Result<Vec<Service>> {
    brew.json(&["services", "list", "--json"]).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORMULAE: &str = include_str!("../../tests/fixtures/info_formulae.json");
    const CASKS: &str = include_str!("../../tests/fixtures/info_casks.json");

    fn parse(raw: &str) -> Info {
        serde_json::from_str(raw).expect("fixture parses")
    }

    #[test]
    fn maps_a_real_formula_to_an_installed_package() {
        let info = parse(FORMULAE);
        let jq = info.formulae.iter().find(|f| f.name == "jq").unwrap();
        let pkg = InstalledPackage::from(jq);

        assert_eq!(pkg.kind, Kind::Formula);
        assert_eq!(pkg.version.as_deref(), Some("1.8.2"));
        assert!(pkg.installed_on_request);
        assert!(pkg.installed_at.is_some());
        assert!(!pkg.self_updating);
    }

    #[test]
    fn surfaces_superseded_kegs_as_rollback_targets() {
        let info = parse(FORMULAE);
        let python = info
            .formulae
            .iter()
            .find(|f| f.name.starts_with("python@"))
            .expect("python fixture");
        let pkg = InstalledPackage::from(python);

        if python.installed.len() > 1 {
            assert!(!pkg.rollback_targets.is_empty());
            assert!(!pkg.rollback_targets.contains(pkg.version.as_ref().unwrap()));
        }
    }

    #[test]
    fn maps_a_real_cask_to_an_installed_package() {
        let info = parse(CASKS);
        let ghostty = info.casks.iter().find(|c| c.token == "ghostty").unwrap();
        let pkg = InstalledPackage::from(ghostty);

        assert_eq!(pkg.kind, Kind::Cask);
        assert_eq!(pkg.name, "Ghostty");
        assert_eq!(pkg.version.as_deref(), Some("1.3.1"));
        assert!(pkg.self_updating, "ghostty sets auto_updates");
        assert!(pkg.rollback_targets.is_empty());
    }

    #[test]
    fn summary_counts_each_category() {
        let mut info = parse(FORMULAE);
        info.casks = parse(CASKS).casks;
        let packages: Vec<_> = info
            .formulae
            .iter()
            .map(InstalledPackage::from)
            .chain(info.casks.iter().map(InstalledPackage::from))
            .collect();

        let summary = summarize(&packages);
        assert_eq!(summary.formulae, info.formulae.len());
        assert_eq!(summary.casks, info.casks.len());
        assert!(summary.requested <= packages.len());
    }
}
