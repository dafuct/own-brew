//! What an upgrade is likely to do to you.
//!
//! Deliberately **two independent axes**, not one blended score:
//!
//! * **Risk** — how likely this upgrade is to break something. Driven by how
//!   many installed packages depend on it, how far the version moves, and how
//!   often the formula fails to build for other people.
//! * **Urgency** — how much you want it anyway. Driven by known
//!   vulnerabilities in the version currently installed.
//!
//! Collapsing these into a single number would hide the case that matters
//! most: a frightening upgrade you should nevertheless do today because the
//! version you are running has a critical CVE.

use crate::brew::Brew;
use crate::error::Result;
use crate::history::diff::compare_versions;
use crate::security::Severity;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Low,
    Moderate,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VersionJump {
    /// Leading component changed: the upstream project's own signal for
    /// "this may break you".
    Major,
    Minor,
    Patch,
    /// Homebrew rebuilt the same upstream version, usually for a dependency
    /// bump. Nearly always safe.
    Revision,
    /// Versions that cannot be compared numerically.
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    pub package: String,
    pub current_version: Option<String>,
    pub new_version: Option<String>,
    pub jump: VersionJump,

    /// Installed packages that depend on this one, and would be rebuilt or
    /// relinked against the new version.
    pub dependents: Vec<String>,
    /// Reported build failures for this formula in the last 30 days, from
    /// Homebrew's public analytics.
    pub build_errors_30d: Option<u64>,
    pub deprecated: bool,

    /// Vulnerabilities in the version currently installed.
    pub known_vulnerabilities: usize,
    pub worst_severity: Option<Severity>,

    pub risk: Level,
    pub urgency: Level,
    /// Plain sentences, shown verbatim. Never a bare score.
    pub reasons: Vec<String>,
    /// True when a superseded keg would remain on disk afterwards, so the
    /// upgrade can be undone instantly.
    pub undoable: bool,
}

/// Enough dependents that a bad upgrade ripples widely.
const WIDE_BLAST_RADIUS: usize = 8;
const SOME_BLAST_RADIUS: usize = 2;
/// Build failures are counted across all Homebrew users, so the bar is high.
const NOISY_BUILD: u64 = 5_000;

pub struct Inputs<'a> {
    pub package: &'a str,
    pub current_version: Option<&'a str>,
    pub new_version: Option<&'a str>,
    pub dependents: Vec<String>,
    pub build_errors_30d: Option<u64>,
    pub deprecated: bool,
    pub known_vulnerabilities: usize,
    pub worst_severity: Option<Severity>,
    pub undoable: bool,
}

pub fn assess(input: Inputs<'_>) -> Assessment {
    let jump = classify(input.current_version, input.new_version);
    let mut reasons = Vec::new();

    // ---- risk -------------------------------------------------------------
    let mut risk = Level::Low;

    match jump {
        VersionJump::Major => {
            risk = Level::High;
            reasons.push(
                "Major version change — upstream reserves these for breaking changes".to_owned(),
            );
        }
        VersionJump::Minor => {
            risk = Level::Moderate;
            reasons.push("Minor version change".to_owned());
        }
        VersionJump::Patch => reasons.push("Patch release".to_owned()),
        VersionJump::Revision => {
            reasons.push("Same upstream version, rebuilt by Homebrew".to_owned())
        }
        VersionJump::Unknown => {
            risk = Level::Moderate;
            reasons.push(
                "Versions cannot be compared, so the size of the change is unclear".to_owned(),
            );
        }
    }

    let dependents = input.dependents.len();
    if dependents >= WIDE_BLAST_RADIUS {
        risk = Level::High;
        reasons.push(format!(
            "{dependents} installed packages depend on this, so a bad build affects all of them"
        ));
    } else if dependents >= SOME_BLAST_RADIUS {
        risk = risk.max(Level::Moderate);
        reasons.push(format!("{dependents} installed packages depend on this"));
    } else if dependents == 1 {
        reasons.push("1 installed package depends on this".to_owned());
    }

    if let Some(errors) = input.build_errors_30d {
        if errors >= NOISY_BUILD {
            risk = Level::High;
            reasons.push(format!(
                "{} build failures reported by other users in the last 30 days",
                thousands(errors)
            ));
        }
    }

    if input.deprecated {
        risk = risk.max(Level::Moderate);
        reasons.push("This formula is deprecated and may stop working".to_owned());
    }

    if input.undoable {
        // Reduces the consequence of being wrong, not the chance.
        reasons
            .push("The current version stays on disk, so this can be undone instantly".to_owned());
    }

    // ---- urgency ----------------------------------------------------------
    let urgency = match (input.known_vulnerabilities, input.worst_severity) {
        (0, _) | (_, None) => Level::Low,
        (count, Some(worst)) => {
            reasons.push(format!(
                "The installed version has {count} known {} ({} at worst)",
                if count == 1 {
                    "vulnerability"
                } else {
                    "vulnerabilities"
                },
                worst.as_str().to_lowercase()
            ));
            match worst {
                Severity::Critical | Severity::High => Level::High,
                Severity::Medium => Level::Moderate,
                Severity::Low | Severity::Unknown => Level::Low,
            }
        }
    };

    Assessment {
        package: input.package.to_owned(),
        current_version: input.current_version.map(str::to_owned),
        new_version: input.new_version.map(str::to_owned),
        jump,
        dependents: input.dependents,
        build_errors_30d: input.build_errors_30d,
        deprecated: input.deprecated,
        known_vulnerabilities: input.known_vulnerabilities,
        worst_severity: input.worst_severity,
        risk,
        urgency,
        reasons,
        undoable: input.undoable,
    }
}

/// How far a version moved.
///
/// Homebrew appends `_n` for its own rebuilds of an unchanged upstream
/// version, which is why that case is distinguished from a patch release.
pub fn classify(current: Option<&str>, new: Option<&str>) -> VersionJump {
    let (Some(current), Some(new)) = (current, new) else {
        return VersionJump::Unknown;
    };

    let upstream = |v: &str| v.split('_').next().unwrap_or(v).to_owned();
    if upstream(current) == upstream(new) && current != new {
        return VersionJump::Revision;
    }

    let parts = |v: &str| -> Vec<u64> {
        v.split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect()
    };

    let (a, b) = (parts(current), parts(new));
    if a.is_empty() || b.is_empty() || compare_versions(current, new).is_none() {
        return VersionJump::Unknown;
    }

    for index in 0..a.len().max(b.len()) {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        if left != right {
            return match index {
                0 => VersionJump::Major,
                1 => VersionJump::Minor,
                _ => VersionJump::Patch,
            };
        }
    }
    VersionJump::Revision
}

fn thousands(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Installed packages that depend on `formula`, asked of Homebrew directly.
///
/// Costs one process per call, so prefer [`blast_radius`] when assessing more
/// than one package.
pub async fn dependents(brew: &Brew, formula: &str) -> Result<Vec<String>> {
    let raw = brew
        .output(&["uses", "--installed", "--formula", formula])
        .await?;
    Ok(raw
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Reverse-dependency map for everything installed, in a single call.
///
/// Built by inverting each keg's `runtime_dependencies`, which is the full
/// runtime closure rather than the direct dependency list — inverting
/// `dependencies` instead would miss transitive users (it finds 9 dependents
/// for openssl@3 where Homebrew reports 24). Verified to match
/// `brew uses --installed` exactly.
pub fn blast_radius(info: &crate::model::detail::Info) -> HashMap<String, Vec<String>> {
    let mut reverse: HashMap<String, Vec<String>> = HashMap::new();

    for formula in &info.formulae {
        let Some(keg) = formula.installed.last() else {
            continue;
        };
        for dependency in &keg.runtime_dependencies {
            reverse
                .entry(dependency.full_name.clone())
                .or_default()
                .push(formula.name.clone());
        }
    }

    for dependents in reverse.values_mut() {
        dependents.sort();
        dependents.dedup();
    }
    reverse
}

/// Build failures per formula over the last 30 days.
pub async fn build_errors(http: &reqwest::Client) -> HashMap<String, u64> {
    crate::catalog::analytics::build_errors(http).await
}

/// Assess every pending update.
///
/// Everything expensive is gathered once for the whole list: the outdated set,
/// the installed graph, the vulnerability scan and the build-error feed run
/// concurrently, and the reverse-dependency map is inverted rather than asked
/// per package.
pub async fn assess_all(
    outdated: &crate::model::Outdated,
    info: &crate::model::detail::Info,
    vulns: &crate::security::Report,
    brew: &Brew,
    http: &reqwest::Client,
) -> Vec<Assessment> {
    let radius = blast_radius(info);
    let errors = build_errors(http).await;
    let on_disk = crate::rollback::cellar::inventory(brew.prefix());

    let mut assessments: Vec<Assessment> = outdated
        .formulae
        .iter()
        .map(|formula| {
            let found = vulns.packages.iter().find(|p| p.formula == formula.name);
            assess(Inputs {
                package: &formula.name,
                current_version: formula.installed_versions.first().map(String::as_str),
                new_version: formula.current_version.as_deref(),
                dependents: radius.get(&formula.name).cloned().unwrap_or_default(),
                build_errors_30d: errors.get(&formula.name).copied(),
                deprecated: info
                    .formulae
                    .iter()
                    .find(|f| f.name == formula.name)
                    .is_some_and(|f| f.deprecated),
                known_vulnerabilities: found.map(|p| p.vulnerabilities.len()).unwrap_or(0),
                worst_severity: found.and_then(|p| p.worst()),
                // The version being replaced stays in the Cellar, so this
                // upgrade can be undone instantly afterwards.
                undoable: on_disk.contains_key(&formula.name),
            })
        })
        .collect();

    // Most urgent first, then riskiest: the order they should be read in.
    assessments.sort_by(|a, b| {
        b.urgency
            .cmp(&a.urgency)
            .then_with(|| b.risk.cmp(&a.risk))
            .then_with(|| a.package.cmp(&b.package))
    });
    assessments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(current: &'static str, new: &'static str) -> Inputs<'static> {
        Inputs {
            package: "demo",
            current_version: Some(current),
            new_version: Some(new),
            dependents: Vec::new(),
            build_errors_30d: None,
            deprecated: false,
            known_vulnerabilities: 0,
            worst_severity: None,
            undoable: false,
        }
    }

    fn names(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("pkg{i}")).collect()
    }

    #[test]
    fn classifies_version_movement() {
        assert_eq!(classify(Some("1.2.3"), Some("2.0.0")), VersionJump::Major);
        assert_eq!(classify(Some("1.2.3"), Some("1.3.0")), VersionJump::Minor);
        assert_eq!(classify(Some("1.2.3"), Some("1.2.4")), VersionJump::Patch);
        // A Homebrew rebuild of the same upstream release.
        assert_eq!(
            classify(Some("1.16.2_1"), Some("1.16.2_2")),
            VersionJump::Revision
        );
        assert_eq!(classify(Some("HEAD"), Some("latest")), VersionJump::Unknown);
        assert_eq!(classify(None, Some("1.0")), VersionJump::Unknown);
    }

    #[test]
    fn date_versions_are_handled() {
        // ca-certificates ships dates; a new day is not a major upgrade panic.
        assert_eq!(
            classify(Some("2026-05-14"), Some("2026-07-16")),
            VersionJump::Minor
        );
    }

    #[test]
    fn a_patch_release_with_no_dependents_is_low_risk() {
        let assessment = assess(base("1.2.3", "1.2.4"));
        assert_eq!(assessment.risk, Level::Low);
        assert_eq!(assessment.urgency, Level::Low);
    }

    #[test]
    fn a_major_change_is_high_risk() {
        let assessment = assess(base("1.2.3", "2.0.0"));
        assert_eq!(assessment.risk, Level::High);
        assert!(assessment
            .reasons
            .iter()
            .any(|r| r.contains("Major version")));
    }

    #[test]
    fn a_wide_blast_radius_raises_risk_on_its_own() {
        let mut input = base("1.2.3", "1.2.4");
        input.dependents = names(24);
        let assessment = assess(input);
        assert_eq!(
            assessment.risk,
            Level::High,
            "a patch release can still be risky"
        );
        assert!(assessment
            .reasons
            .iter()
            .any(|r| r.contains("24 installed packages")));
    }

    #[test]
    fn a_few_dependents_raise_risk_only_to_moderate() {
        let mut input = base("1.2.3", "1.2.4");
        input.dependents = names(3);
        assert_eq!(assess(input).risk, Level::Moderate);
    }

    #[test]
    fn frequent_build_failures_raise_risk() {
        let mut input = base("1.2.3", "1.2.4");
        input.build_errors_30d = Some(21_762);
        let assessment = assess(input);
        assert_eq!(assessment.risk, Level::High);
        assert!(assessment.reasons.iter().any(|r| r.contains("21,762")));
    }

    #[test]
    fn a_quiet_build_record_does_not_raise_risk() {
        let mut input = base("1.2.3", "1.2.4");
        input.build_errors_30d = Some(12);
        assert_eq!(assess(input).risk, Level::Low);
    }

    #[test]
    fn vulnerabilities_drive_urgency_not_risk() {
        let mut input = base("1.2.3", "1.2.4");
        input.known_vulnerabilities = 17;
        input.worst_severity = Some(Severity::Critical);
        let assessment = assess(input);

        assert_eq!(assessment.urgency, Level::High);
        assert_eq!(
            assessment.risk,
            Level::Low,
            "a security problem does not make the upgrade itself dangerous"
        );
        assert!(assessment
            .reasons
            .iter()
            .any(|r| r.contains("17 known vulnerabilities") && r.contains("critical")));
    }

    #[test]
    fn the_frightening_but_necessary_upgrade_is_representable() {
        // openssl@3: a big blast radius *and* a critical CVE. Both must show.
        let mut input = base("3.6.2", "3.6.3");
        input.package = "openssl@3";
        input.dependents = names(24);
        input.known_vulnerabilities = 17;
        input.worst_severity = Some(Severity::Critical);

        let assessment = assess(input);
        assert_eq!(assessment.risk, Level::High);
        assert_eq!(assessment.urgency, Level::High);
    }

    #[test]
    fn one_dependent_is_reported_in_the_singular() {
        let mut input = base("1.2.3", "1.2.4");
        input.dependents = names(1);
        let assessment = assess(input);
        assert!(assessment
            .reasons
            .iter()
            .any(|r| r == "1 installed package depends on this"));
        assert_eq!(assessment.risk, Level::Low);
    }

    #[test]
    fn a_single_vulnerability_is_reported_in_the_singular() {
        let mut input = base("1.2.3", "1.2.4");
        input.known_vulnerabilities = 1;
        input.worst_severity = Some(Severity::Medium);
        let assessment = assess(input);
        assert_eq!(assessment.urgency, Level::Moderate);
        assert!(assessment
            .reasons
            .iter()
            .any(|r| r.contains("1 known vulnerability")));
    }

    #[test]
    fn deprecation_is_surfaced() {
        let mut input = base("1.2.3", "1.2.4");
        input.deprecated = true;
        let assessment = assess(input);
        assert_eq!(assessment.risk, Level::Moderate);
        assert!(assessment.reasons.iter().any(|r| r.contains("deprecated")));
    }

    #[test]
    fn undoability_is_stated_without_lowering_risk() {
        let mut input = base("1.2.3", "2.0.0");
        input.undoable = true;
        let assessment = assess(input);
        assert_eq!(
            assessment.risk,
            Level::High,
            "being undoable does not make it safe"
        );
        assert!(assessment
            .reasons
            .iter()
            .any(|r| r.contains("undone instantly")));
    }

    #[test]
    fn formats_large_counts_readably() {
        assert_eq!(thousands(21_762), "21,762");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_432_817), "1,432,817");
    }

    /// Real reverse dependencies from the machine.
    #[tokio::test]
    async fn reads_real_reverse_dependencies() {
        let Ok(brew) = Brew::discover() else { return };
        let Ok(found) = dependents(&brew, "openssl@3").await else {
            return; // openssl@3 may not be installed
        };
        assert!(found.iter().all(|d| !d.is_empty()));
    }

    /// The whole-graph shortcut must agree with Homebrew, or the risk signal
    /// shown for every update would be quietly wrong.
    #[tokio::test]
    async fn the_inverted_graph_matches_brew_uses() {
        let Ok(brew) = Brew::discover() else { return };
        let Ok(info) = brew
            .json::<crate::model::detail::Info>(&["info", "--json=v2", "--installed"])
            .await
        else {
            return;
        };

        let map = blast_radius(&info);
        // Check the packages with the widest reach, where being wrong matters.
        let mut sampled: Vec<_> = map.iter().collect();
        sampled.sort_by_key(|(_, users)| std::cmp::Reverse(users.len()));

        for (formula, mine) in sampled.into_iter().take(5) {
            let Ok(theirs) = dependents(&brew, formula).await else {
                continue;
            };
            let mut theirs = theirs;
            theirs.sort();
            assert_eq!(
                mine, &theirs,
                "blast radius for {formula} disagrees with `brew uses --installed`"
            );
        }
    }
}
