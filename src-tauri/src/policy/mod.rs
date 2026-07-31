//! Per-package update policy.
//!
//! Homebrew offers exactly two positions: upgrade everything now, or pin a
//! formula forever. Everything in between — "wait a week before taking a new
//! release", "patch updates only", "never touch this one" — has to be held in
//! the user's head. This module makes those rules explicit and evaluable.
//!
//! Policies never *perform* anything. They decide which of the outdated
//! packages are due, and the user still presses the button.

use crate::error::Result;
use crate::history::History;
use crate::model::entry::Kind;
use crate::model::Outdated;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const DAY: i64 = 86_400;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// No restriction: upgrade whenever an update appears.
    Auto,
    /// Hold this package where it is.
    Never,
    /// Take a new version only once it has been available for a while, so
    /// someone else finds the broken release first.
    Bake,
    /// Accept updates that keep the leading version component.
    MinorOnly,
}

impl Rule {
    pub fn as_str(self) -> &'static str {
        match self {
            Rule::Auto => "auto",
            Rule::Never => "never",
            Rule::Bake => "bake",
            Rule::MinorOnly => "minor_only",
        }
    }

    pub fn parse(raw: &str) -> Self {
        match raw {
            "never" => Rule::Never,
            "bake" => Rule::Bake,
            "minor_only" => Rule::MinorOnly,
            _ => Rule::Auto,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub kind: Kind,
    pub package: String,
    pub rule: Rule,
    /// Days to wait under [`Rule::Bake`].
    pub bake_days: Option<i64>,
    pub note: Option<String>,
}

impl Policy {
    pub fn auto(kind: Kind, package: &str) -> Self {
        Self {
            kind,
            package: package.to_owned(),
            rule: Rule::Auto,
            bake_days: None,
            note: None,
        }
    }
}

/// What the policy engine concluded for one outdated package.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub kind: Kind,
    pub package: String,
    pub current_version: Option<String>,
    pub available_version: Option<String>,
    pub rule: Rule,
    /// Whether own-brew would upgrade this now.
    pub due: bool,
    /// Shown verbatim, so the user always knows why something is being held.
    pub reason: String,
    /// For a baking package, when it becomes due (unix seconds).
    pub due_at: Option<i64>,
}

/// Decide which outdated packages are due, given the rules and when each new
/// version was first seen.
///
/// `first_seen` returns the unix timestamp at which a version was first
/// observed. Homebrew publishes no release timestamps, so own-brew's own
/// sighting record is the only available clock.
pub fn evaluate<F>(
    now: i64,
    outdated: &Outdated,
    policy_for: impl Fn(Kind, &str) -> Policy,
    first_seen: F,
) -> Vec<Decision>
where
    F: Fn(Kind, &str, &str) -> Option<i64>,
{
    let mut decisions = Vec::new();

    for formula in &outdated.formulae {
        let policy = policy_for(Kind::Formula, &formula.name);
        // A Homebrew pin is a stronger, external statement; respect it and say so.
        let decision = if formula.pinned {
            Decision {
                kind: Kind::Formula,
                package: formula.name.clone(),
                current_version: formula.installed_versions.first().cloned(),
                available_version: formula.current_version.clone(),
                rule: policy.rule,
                due: false,
                reason: "Pinned in Homebrew".to_owned(),
                due_at: None,
            }
        } else {
            decide(
                now,
                Kind::Formula,
                &formula.name,
                formula.installed_versions.first().map(String::as_str),
                formula.current_version.as_deref(),
                &policy,
                &first_seen,
            )
        };
        decisions.push(decision);
    }

    for cask in &outdated.casks {
        let policy = policy_for(Kind::Cask, &cask.name);
        decisions.push(decide(
            now,
            Kind::Cask,
            &cask.name,
            cask.installed_versions.first().map(String::as_str),
            cask.current_version.as_deref(),
            &policy,
            &first_seen,
        ));
    }

    decisions
}

fn decide<F>(
    now: i64,
    kind: Kind,
    package: &str,
    current: Option<&str>,
    available: Option<&str>,
    policy: &Policy,
    first_seen: &F,
) -> Decision
where
    F: Fn(Kind, &str, &str) -> Option<i64>,
{
    let mut decision = Decision {
        kind,
        package: package.to_owned(),
        current_version: current.map(str::to_owned),
        available_version: available.map(str::to_owned),
        rule: policy.rule,
        due: true,
        reason: "No policy — upgrades freely".to_owned(),
        due_at: None,
    };

    match policy.rule {
        Rule::Auto => {}

        Rule::Never => {
            decision.due = false;
            decision.reason = policy
                .note
                .clone()
                .unwrap_or_else(|| "Held back by your never-upgrade rule".to_owned());
        }

        Rule::MinorOnly => {
            if let (Some(current), Some(available)) = (current, available) {
                if leading(current) != leading(available) {
                    decision.due = false;
                    decision.reason = format!(
                        "Major version change ({} to {}) — your rule allows minor updates only",
                        leading(current).unwrap_or_default(),
                        leading(available).unwrap_or_default()
                    );
                } else {
                    decision.reason = "Minor update, allowed by your rule".to_owned();
                }
            } else {
                // Without both versions the rule cannot be applied safely, so
                // hold rather than upgrade against the user's intent.
                decision.due = false;
                decision.reason = "Cannot compare versions, so holding".to_owned();
            }
        }

        Rule::Bake => {
            let days = policy.bake_days.unwrap_or(7).max(0);
            match available.and_then(|version| first_seen(kind, package, version)) {
                Some(seen_at) => {
                    let ready_at = seen_at + days * DAY;
                    if now >= ready_at {
                        decision.reason = format!("Available for over {days} days");
                    } else {
                        let remaining = ((ready_at - now) as f64 / DAY as f64).ceil() as i64;
                        decision.due = false;
                        decision.due_at = Some(ready_at);
                        decision.reason = format!(
                            "Baking for {days} days — ready in {remaining} day{}",
                            if remaining == 1 { "" } else { "s" }
                        );
                    }
                }
                None => {
                    // First time we have seen this version: the clock starts now.
                    decision.due = false;
                    decision.due_at = Some(now + days * DAY);
                    decision.reason =
                        format!("Just appeared — baking for {days} days before upgrading");
                }
            }
        }
    }

    decision
}

/// The leading numeric component of a version, used for "minor only".
fn leading(version: &str) -> Option<String> {
    version
        .split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())
        .map(str::to_owned)
}

// ------------------------------------------------------------- persistence ---

impl History {
    pub fn set_policy(&self, policy: &Policy) -> Result<()> {
        let conn = self.connection();
        if policy.rule == Rule::Auto {
            // Auto is the default; storing it would just be noise.
            conn.execute(
                "DELETE FROM policies WHERE kind = ?1 AND package = ?2",
                params![policy.kind.as_str(), policy.package],
            )?;
            return Ok(());
        }

        conn.execute(
            "INSERT INTO policies (kind, package, rule, bake_days, note, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(kind, package) DO UPDATE SET
                 rule = excluded.rule,
                 bake_days = excluded.bake_days,
                 note = excluded.note,
                 updated_at = excluded.updated_at",
            params![
                policy.kind.as_str(),
                policy.package,
                policy.rule.as_str(),
                policy.bake_days,
                policy.note,
                crate::history::now(),
            ],
        )?;
        Ok(())
    }

    pub fn policy(&self, kind: Kind, package: &str) -> Result<Policy> {
        let conn = self.connection();
        let found = conn
            .query_row(
                "SELECT rule, bake_days, note FROM policies WHERE kind = ?1 AND package = ?2",
                params![kind.as_str(), package],
                |row| {
                    Ok(Policy {
                        kind,
                        package: package.to_owned(),
                        rule: Rule::parse(&row.get::<_, String>(0)?),
                        bake_days: row.get(1)?,
                        note: row.get(2)?,
                    })
                },
            )
            .optional()?;
        Ok(found.unwrap_or_else(|| Policy::auto(kind, package)))
    }

    pub fn policies(&self) -> Result<Vec<Policy>> {
        let conn = self.connection();
        let mut statement = conn.prepare(
            "SELECT kind, package, rule, bake_days, note FROM policies ORDER BY package",
        )?;
        let rows = statement.query_map([], |row| {
            let kind = match row.get::<_, String>(0)?.as_str() {
                "cask" => Kind::Cask,
                _ => Kind::Formula,
            };
            Ok(Policy {
                kind,
                package: row.get(1)?,
                rule: Rule::parse(&row.get::<_, String>(2)?),
                bake_days: row.get(3)?,
                note: row.get(4)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::outdated::{OutdatedCask, OutdatedFormula};

    const NOW: i64 = 1_800_000_000;

    fn outdated_formula(name: &str, from: &str, to: &str, pinned: bool) -> OutdatedFormula {
        OutdatedFormula {
            name: name.to_owned(),
            installed_versions: vec![from.to_owned()],
            current_version: Some(to.to_owned()),
            pinned,
            pinned_version: None,
        }
    }

    fn only(formula: OutdatedFormula) -> Outdated {
        Outdated {
            formulae: vec![formula],
            casks: Vec::new(),
        }
    }

    fn decide_with(policy: Policy, outdated: Outdated, seen: Option<i64>) -> Decision {
        evaluate(NOW, &outdated, |_, _| policy.clone(), |_, _, _| seen).remove(0)
    }

    #[test]
    fn without_a_policy_everything_is_due() {
        let decision = decide_with(
            Policy::auto(Kind::Formula, "jq"),
            only(outdated_formula("jq", "1.8.1", "1.8.2", false)),
            None,
        );
        assert!(decision.due);
    }

    #[test]
    fn never_holds_the_package_back() {
        let mut policy = Policy::auto(Kind::Formula, "terraform");
        policy.rule = Rule::Never;
        let decision = decide_with(
            policy,
            only(outdated_formula("terraform", "1.9", "1.10", false)),
            None,
        );
        assert!(!decision.due);
        assert!(decision.reason.contains("never-upgrade"));
    }

    #[test]
    fn a_custom_note_replaces_the_default_reason() {
        let mut policy = Policy::auto(Kind::Formula, "terraform");
        policy.rule = Rule::Never;
        policy.note = Some("Breaks our CI pipeline".to_owned());
        let decision = decide_with(
            policy,
            only(outdated_formula("terraform", "1.9", "1.10", false)),
            None,
        );
        assert_eq!(decision.reason, "Breaks our CI pipeline");
    }

    #[test]
    fn minor_only_allows_a_minor_bump() {
        let mut policy = Policy::auto(Kind::Formula, "node");
        policy.rule = Rule::MinorOnly;
        let decision = decide_with(
            policy,
            only(outdated_formula("node", "26.5.0", "26.6.0", false)),
            None,
        );
        assert!(decision.due);
    }

    #[test]
    fn minor_only_blocks_a_major_bump() {
        let mut policy = Policy::auto(Kind::Formula, "node");
        policy.rule = Rule::MinorOnly;
        let decision = decide_with(
            policy,
            only(outdated_formula("node", "26.5.0", "27.0.0", false)),
            None,
        );
        assert!(!decision.due);
        assert!(decision.reason.contains("Major version change"));
    }

    #[test]
    fn minor_only_holds_when_versions_cannot_be_compared() {
        let mut policy = Policy::auto(Kind::Formula, "weird");
        policy.rule = Rule::MinorOnly;
        let decision = decide_with(
            policy,
            only(OutdatedFormula {
                name: "weird".into(),
                installed_versions: vec![],
                current_version: None,
                pinned: false,
                pinned_version: None,
            }),
            None,
        );
        assert!(!decision.due, "holding is safer than guessing");
    }

    #[test]
    fn bake_holds_a_version_that_has_only_just_appeared() {
        let mut policy = Policy::auto(Kind::Formula, "jq");
        policy.rule = Rule::Bake;
        policy.bake_days = Some(7);

        let decision = decide_with(
            policy,
            only(outdated_formula("jq", "1.8.1", "1.8.2", false)),
            None,
        );
        assert!(!decision.due);
        assert_eq!(decision.due_at, Some(NOW + 7 * DAY));
        assert!(decision.reason.contains("Just appeared"));
    }

    #[test]
    fn bake_releases_once_enough_days_have_passed() {
        let mut policy = Policy::auto(Kind::Formula, "jq");
        policy.rule = Rule::Bake;
        policy.bake_days = Some(7);

        let decision = decide_with(
            policy.clone(),
            only(outdated_formula("jq", "1.8.1", "1.8.2", false)),
            Some(NOW - 8 * DAY),
        );
        assert!(decision.due);

        let still_baking = decide_with(
            policy,
            only(outdated_formula("jq", "1.8.1", "1.8.2", false)),
            Some(NOW - 2 * DAY),
        );
        assert!(!still_baking.due);
        assert!(
            still_baking.reason.contains("ready in 5 days"),
            "got {:?}",
            still_baking.reason
        );
    }

    #[test]
    fn bake_reports_a_single_remaining_day_in_the_singular() {
        let mut policy = Policy::auto(Kind::Formula, "jq");
        policy.rule = Rule::Bake;
        policy.bake_days = Some(7);
        let decision = decide_with(
            policy,
            only(outdated_formula("jq", "1.8.1", "1.8.2", false)),
            Some(NOW - 6 * DAY),
        );
        assert!(
            decision.reason.contains("ready in 1 day"),
            "got {:?}",
            decision.reason
        );
    }

    #[test]
    fn a_homebrew_pin_overrides_any_local_rule() {
        let decision = decide_with(
            Policy::auto(Kind::Formula, "jq"),
            only(outdated_formula("jq", "1.8.1", "1.8.2", true)),
            None,
        );
        assert!(!decision.due);
        assert_eq!(decision.reason, "Pinned in Homebrew");
    }

    #[test]
    fn casks_are_evaluated_too() {
        let outdated = Outdated {
            formulae: Vec::new(),
            casks: vec![OutdatedCask {
                name: "warp".into(),
                installed_versions: vec!["1.0".into()],
                current_version: Some("2.0".into()),
            }],
        };
        let mut policy = Policy::auto(Kind::Cask, "warp");
        policy.rule = Rule::MinorOnly;

        let decisions = evaluate(NOW, &outdated, |_, _| policy.clone(), |_, _, _| None);
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].kind, Kind::Cask);
        assert!(!decisions[0].due, "1.0 to 2.0 is a major change");
    }

    #[test]
    fn leading_component_handles_homebrew_version_shapes() {
        assert_eq!(leading("26.5.0").as_deref(), Some("26"));
        assert_eq!(leading("2026-07-16").as_deref(), Some("2026"));
        assert_eq!(leading("v3.1").as_deref(), Some("3"));
        assert_eq!(leading("HEAD"), None);
    }

    // ---------------------------------------------------------- storage ---

    fn store() -> History {
        History::in_memory().unwrap()
    }

    #[test]
    fn a_stored_policy_round_trips() {
        let history = store();
        let policy = Policy {
            kind: Kind::Formula,
            package: "terraform".into(),
            rule: Rule::Bake,
            bake_days: Some(14),
            note: Some("burned once".into()),
        };
        history.set_policy(&policy).unwrap();

        let loaded = history.policy(Kind::Formula, "terraform").unwrap();
        assert_eq!(loaded.rule, Rule::Bake);
        assert_eq!(loaded.bake_days, Some(14));
        assert_eq!(loaded.note.as_deref(), Some("burned once"));
    }

    #[test]
    fn an_unset_package_defaults_to_auto() {
        let history = store();
        assert_eq!(
            history.policy(Kind::Formula, "anything").unwrap().rule,
            Rule::Auto
        );
    }

    #[test]
    fn setting_a_policy_twice_updates_rather_than_duplicates() {
        let history = store();
        let mut policy = Policy::auto(Kind::Formula, "jq");
        policy.rule = Rule::Never;
        history.set_policy(&policy).unwrap();

        policy.rule = Rule::MinorOnly;
        history.set_policy(&policy).unwrap();

        assert_eq!(history.policies().unwrap().len(), 1);
        assert_eq!(
            history.policy(Kind::Formula, "jq").unwrap().rule,
            Rule::MinorOnly
        );
    }

    #[test]
    fn resetting_to_auto_removes_the_stored_rule() {
        let history = store();
        let mut policy = Policy::auto(Kind::Formula, "jq");
        policy.rule = Rule::Never;
        history.set_policy(&policy).unwrap();
        assert_eq!(history.policies().unwrap().len(), 1);

        policy.rule = Rule::Auto;
        history.set_policy(&policy).unwrap();
        assert!(history.policies().unwrap().is_empty());
    }

    #[test]
    fn a_formula_and_a_cask_can_hold_different_rules() {
        let history = store();
        let mut formula = Policy::auto(Kind::Formula, "docker");
        formula.rule = Rule::Never;
        let mut cask = Policy::auto(Kind::Cask, "docker");
        cask.rule = Rule::MinorOnly;

        history.set_policy(&formula).unwrap();
        history.set_policy(&cask).unwrap();

        assert_eq!(
            history.policy(Kind::Formula, "docker").unwrap().rule,
            Rule::Never
        );
        assert_eq!(
            history.policy(Kind::Cask, "docker").unwrap().rule,
            Rule::MinorOnly
        );
    }
}
