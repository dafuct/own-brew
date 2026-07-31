//! The lean catalog record.
//!
//! Deliberately small: one of these exists for every package Homebrew knows
//! about, and they all stay resident so search never touches disk.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Formula,
    Cask,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Formula => "formula",
            Kind::Cask => "cask",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub kind: Kind,
    /// `name` for a formula, `token` for a cask — the identifier `brew` accepts.
    pub id: String,
    /// Human-facing name. Casks carry a separate display name; formulae reuse `id`.
    pub name: String,
    pub desc: Option<String>,
    pub version: String,
    pub tap: String,
    pub homepage: Option<String>,
    pub deprecated: bool,
    pub disabled: bool,
    /// Installs over the last 90 days, from Homebrew's public analytics.
    /// `None` when the analytics feed was unavailable.
    #[serde(default)]
    pub installs_90d: Option<u64>,
    /// Lowercased `id` + `name`, precomputed so search avoids re-allocating.
    #[serde(skip)]
    pub haystack_name: String,
    /// Lowercased description, precomputed for the same reason.
    #[serde(skip)]
    pub haystack_desc: String,
}

impl Entry {
    /// Restore the fields skipped during serialization of the cached index.
    pub fn rehydrate(&mut self) {
        self.haystack_name = if self.id.eq_ignore_ascii_case(&self.name) {
            self.id.to_lowercase()
        } else {
            format!("{} {}", self.id.to_lowercase(), self.name.to_lowercase())
        };
        self.haystack_desc = self.desc.as_deref().unwrap_or_default().to_lowercase();
    }

    pub fn is_available(&self) -> bool {
        !self.disabled
    }
}

/// The subset of a formula the catalog dump needs to yield an [`Entry`].
#[derive(Deserialize)]
pub struct FormulaEntry {
    pub name: String,
    #[serde(default)]
    pub tap: Option<String>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub versions: Versions,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Default, Deserialize)]
pub struct Versions {
    #[serde(default)]
    pub stable: Option<String>,
}

impl From<FormulaEntry> for Entry {
    fn from(f: FormulaEntry) -> Self {
        let mut entry = Entry {
            kind: Kind::Formula,
            name: f.name.clone(),
            id: f.name,
            desc: f.desc,
            version: f.versions.stable.unwrap_or_default(),
            tap: f.tap.unwrap_or_else(|| "homebrew/core".to_owned()),
            homepage: f.homepage,
            deprecated: f.deprecated,
            disabled: f.disabled,
            installs_90d: None,
            haystack_name: String::new(),
            haystack_desc: String::new(),
        };
        entry.rehydrate();
        entry
    }
}

/// The subset of a cask the catalog dump needs to yield an [`Entry`].
#[derive(Deserialize)]
pub struct CaskEntry {
    pub token: String,
    #[serde(default)]
    pub tap: Option<String>,
    /// Casks may list several display names; the first is the canonical one.
    #[serde(default)]
    pub name: Vec<String>,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default)]
    pub disabled: bool,
}

impl From<CaskEntry> for Entry {
    fn from(c: CaskEntry) -> Self {
        let display = c
            .name
            .into_iter()
            .find(|n| !n.is_empty())
            .unwrap_or_else(|| c.token.clone());
        let mut entry = Entry {
            kind: Kind::Cask,
            id: c.token,
            name: display,
            desc: c.desc,
            version: c.version.unwrap_or_default(),
            tap: c.tap.unwrap_or_else(|| "homebrew/cask".to_owned()),
            homepage: c.homepage,
            deprecated: c.deprecated,
            disabled: c.disabled,
            installs_90d: None,
            haystack_name: String::new(),
            haystack_desc: String::new(),
        };
        entry.rehydrate();
        entry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cask_takes_its_first_display_name() {
        let cask: CaskEntry = serde_json::from_str(
            r#"{"token":"ghostty","name":["Ghostty","ghostty"],"version":"1.3.1"}"#,
        )
        .unwrap();
        let entry = Entry::from(cask);
        assert_eq!(entry.id, "ghostty");
        assert_eq!(entry.name, "Ghostty");
        assert_eq!(entry.tap, "homebrew/cask");
    }

    #[test]
    fn cask_without_a_display_name_falls_back_to_its_token() {
        let cask: CaskEntry = serde_json::from_str(r#"{"token":"0-ad","name":[]}"#).unwrap();
        assert_eq!(Entry::from(cask).name, "0-ad");
    }

    #[test]
    fn formula_entry_ignores_unknown_fields() {
        // The published dump carries dozens of fields we do not model; adding
        // more upstream must never break parsing.
        let formula: FormulaEntry = serde_json::from_str(
            r#"{"name":"jq","desc":"JSON processor","versions":{"stable":"1.8.2","head":"HEAD"},
                "tap":"homebrew/core","bottle":{"stable":{"files":{}}},"revision":0}"#,
        )
        .unwrap();
        let entry = Entry::from(formula);
        assert_eq!(entry.version, "1.8.2");
        assert_eq!(entry.kind, Kind::Formula);
    }

    #[test]
    fn haystacks_are_lowercased_for_search() {
        // Searching "visual studio" must find the cask whose token is "vscode".
        let cask: CaskEntry = serde_json::from_str(
            r#"{"token":"vscode","name":["Visual Studio Code"],"desc":"Code EDITOR"}"#,
        )
        .unwrap();
        let entry = Entry::from(cask);
        assert_eq!(entry.haystack_name, "vscode visual studio code");
        assert_eq!(entry.haystack_desc, "code editor");
    }

    #[test]
    fn identical_id_and_name_are_not_duplicated_in_the_haystack() {
        let formula: FormulaEntry = serde_json::from_str(r#"{"name":"jq"}"#).unwrap();
        assert_eq!(Entry::from(formula).haystack_name, "jq");
    }
}
