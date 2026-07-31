//! `brew outdated --json=v2`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Outdated {
    #[serde(default)]
    pub formulae: Vec<OutdatedFormula>,
    #[serde(default)]
    pub casks: Vec<OutdatedCask>,
}

impl Outdated {
    pub fn total(&self) -> usize {
        self.formulae.len() + self.casks.len()
    }

    /// Everything that would actually change if the user upgraded now.
    /// Pinned formulae are reported by Homebrew but deliberately left alone.
    pub fn upgradable(&self) -> usize {
        self.formulae.iter().filter(|f| !f.pinned).count() + self.casks.len()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OutdatedFormula {
    pub name: String,
    #[serde(default)]
    pub installed_versions: Vec<String>,
    #[serde(default)]
    pub current_version: Option<String>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub pinned_version: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct OutdatedCask {
    pub name: String,
    #[serde(default)]
    pub installed_versions: Vec<String>,
    #[serde(default)]
    pub current_version: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTDATED: &str = include_str!("../../tests/fixtures/outdated.json");

    #[test]
    fn parses_real_outdated_output() {
        let outdated: Outdated = serde_json::from_str(OUTDATED).expect("real output parses");
        assert!(outdated.total() > 0, "fixture should list outdated packages");

        let ada = outdated
            .formulae
            .iter()
            .find(|f| f.name == "ada-url")
            .expect("ada-url is outdated in the fixture");
        assert_eq!(ada.installed_versions, vec!["3.4.4".to_owned()]);
        assert_eq!(ada.current_version.as_deref(), Some("4.0.0"));
        assert!(!ada.pinned);
    }

    #[test]
    fn pinned_formulae_are_excluded_from_the_upgradable_count() {
        let outdated: Outdated = serde_json::from_str(
            r#"{"formulae":[
                 {"name":"a","installed_versions":["1"],"current_version":"2","pinned":true},
                 {"name":"b","installed_versions":["1"],"current_version":"2","pinned":false}],
               "casks":[{"name":"c","installed_versions":["1"],"current_version":"2"}]}"#,
        )
        .unwrap();
        assert_eq!(outdated.total(), 3);
        assert_eq!(outdated.upgradable(), 2);
    }

    #[test]
    fn empty_output_is_valid() {
        let outdated: Outdated = serde_json::from_str(r#"{"formulae":[],"casks":[]}"#).unwrap();
        assert_eq!(outdated.total(), 0);
    }
}
