//! Known vulnerabilities in what is installed.
//!
//! Homebrew 6.0.11+ ships `brew vulns`, which checks installed formulae
//! against OSV.dev. own-brew surfaces it rather than reimplementing it, and is
//! explicit about the coverage gaps so nobody reads a clean report as proof of
//! safety:
//!
//! * **Casks are not checked at all.** GUI applications are the largest part
//!   of most people's attack surface and none of them are covered.
//! * **Formulae without a derivable upstream repository are skipped**, so a
//!   package can be absent from the report without having been checked.

use crate::brew::Brew;
use crate::error::Result;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Severity {
    /// OSV has no severity for this advisory. Ranked lowest so it never
    /// outranks a graded finding, but still reported.
    Unknown,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Critical => "CRITICAL",
            Severity::High => "HIGH",
            Severity::Medium => "MEDIUM",
            Severity::Low => "LOW",
            Severity::Unknown => "UNKNOWN",
        }
    }
}

impl<'de> serde::Deserialize<'de> for SeverityWire {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(SeverityWire(match raw.to_ascii_uppercase().as_str() {
            "CRITICAL" => Severity::Critical,
            "HIGH" => Severity::High,
            "MEDIUM" | "MODERATE" => Severity::Medium,
            "LOW" => Severity::Low,
            _ => Severity::Unknown,
        }))
    }
}

/// Newtype so an unexpected severity string degrades to `Unknown` instead of
/// failing the whole scan.
#[derive(Clone, Copy, Debug)]
pub struct SeverityWire(pub Severity);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vulnerability {
    pub id: String,
    #[serde(deserialize_with = "severity", default = "unknown_severity")]
    pub severity: Severity,
    /// OSV frequently omits this, and sends an explicit `null` when it does.
    #[serde(default, deserialize_with = "nullable")]
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "nullable_vec")]
    pub aliases: Vec<String>,
    /// Upstream commits or versions carrying the fix. Often a long list of
    /// commit hashes, which is why the UI shows only a count.
    #[serde(default, deserialize_with = "nullable_vec")]
    pub fixed_versions: Vec<String>,
}

/// Treat an explicit `null` as "not provided".
///
/// `#[serde(default)]` only covers an *absent* field; the scanner sends
/// `"summary": null`, which would otherwise fail the whole scan.
fn nullable<'de, D, T>(d: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d)
}

fn nullable_vec<'de, D, T>(d: D) -> std::result::Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Ok(Option::<Vec<T>>::deserialize(d)?.unwrap_or_default())
}

fn severity<'de, D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Severity, D::Error> {
    Ok(SeverityWire::deserialize(d)?.0)
}

fn unknown_severity() -> Severity {
    Severity::Unknown
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageVulnerabilities {
    pub formula: String,
    /// The installed version that was checked.
    #[serde(default, deserialize_with = "nullable")]
    pub version: Option<String>,
    #[serde(default)]
    pub repo_url: Option<String>,
    #[serde(default)]
    pub vulnerabilities: Vec<Vulnerability>,
}

impl PackageVulnerabilities {
    pub fn worst(&self) -> Option<Severity> {
        self.vulnerabilities.iter().map(|v| v.severity).max()
    }

    pub fn count_of(&self, severity: Severity) -> usize {
        self.vulnerabilities
            .iter()
            .filter(|v| v.severity == severity)
            .count()
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub packages: Vec<PackageVulnerabilities>,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub unknown: usize,
    pub total: usize,
    /// Unix seconds.
    pub scanned_at: i64,
}

impl Report {
    pub fn build(mut packages: Vec<PackageVulnerabilities>) -> Self {
        // Most severe package first, then most findings.
        packages.sort_by(|a, b| {
            b.worst()
                .cmp(&a.worst())
                .then_with(|| b.vulnerabilities.len().cmp(&a.vulnerabilities.len()))
                .then_with(|| a.formula.cmp(&b.formula))
        });

        let count =
            |severity: Severity| packages.iter().map(|p| p.count_of(severity)).sum::<usize>();

        Self {
            critical: count(Severity::Critical),
            high: count(Severity::High),
            medium: count(Severity::Medium),
            low: count(Severity::Low),
            unknown: count(Severity::Unknown),
            total: packages.iter().map(|p| p.vulnerabilities.len()).sum(),
            scanned_at: crate::history::now(),
            packages,
        }
    }

    /// Findings serious enough to act on today.
    pub fn actionable(&self) -> usize {
        self.critical + self.high
    }
}

/// Scan every installed formula.
///
/// `brew vulns` signals "found something" with exit code 1, so a non-zero exit
/// is expected and must not be treated as a failure.
pub async fn scan(brew: &Brew) -> Result<Report> {
    let packages: Vec<PackageVulnerabilities> =
        brew.json_tolerating(&["vulns", "-j"], &[1]).await?;
    Ok(Report::build(packages))
}

/// Scan a single formula.
pub async fn scan_one(brew: &Brew, formula: &str) -> Result<Report> {
    let packages: Vec<PackageVulnerabilities> = brew
        .json_tolerating(&["vulns", "-j", formula], &[1])
        .await?;
    Ok(Report::build(packages))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VULNS: &str = include_str!("../../tests/fixtures/vulns.json");

    fn fixture() -> Vec<PackageVulnerabilities> {
        serde_json::from_str(VULNS).expect("real `brew vulns -j` output must parse")
    }

    #[test]
    fn parses_real_scanner_output() {
        let packages = fixture();
        assert!(!packages.is_empty());

        let openssl = packages
            .iter()
            .find(|p| p.formula == "openssl@3")
            .expect("openssl@3 is vulnerable in the fixture");
        assert!(openssl.version.is_some());
        assert!(!openssl.vulnerabilities.is_empty());
        assert!(openssl
            .vulnerabilities
            .iter()
            .all(|v| v.id.starts_with("CVE") || v.id.starts_with("GHSA")));
    }

    #[test]
    fn severities_order_from_critical_down() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        assert!(Severity::Medium > Severity::Low);
        assert!(Severity::Low > Severity::Unknown);
    }

    #[test]
    fn a_null_summary_does_not_break_the_scan() {
        // Real output: unbound's CVE-2026-32665 carries "summary": null.
        let vulnerability: Vulnerability = serde_json::from_str(
            r#"{"id":"CVE-1","severity":"HIGH","summary":null,"aliases":[],"fixed_versions":null}"#,
        )
        .expect("an explicit null must be tolerated");
        assert_eq!(vulnerability.summary, None);
        assert!(vulnerability.fixed_versions.is_empty());
    }

    #[test]
    fn a_real_null_summary_is_present_in_the_fixture() {
        let packages = fixture();
        assert!(
            packages
                .iter()
                .flat_map(|p| &p.vulnerabilities)
                .any(|v| v.summary.is_none()),
            "the fixture should still exercise the null-summary path"
        );
    }

    #[test]
    fn an_unrecognised_severity_degrades_instead_of_failing() {
        let vulnerability: Vulnerability = serde_json::from_str(
            r#"{"id":"CVE-1","severity":"SPICY","summary":"x","fixed_versions":[]}"#,
        )
        .expect("an unknown severity must not break the scan");
        assert_eq!(vulnerability.severity, Severity::Unknown);
    }

    #[test]
    fn moderate_is_treated_as_medium() {
        let vulnerability: Vulnerability =
            serde_json::from_str(r#"{"id":"CVE-1","severity":"moderate"}"#).unwrap();
        assert_eq!(vulnerability.severity, Severity::Medium);
    }

    #[test]
    fn report_totals_match_the_findings() {
        let report = Report::build(fixture());
        let counted = report.critical + report.high + report.medium + report.low + report.unknown;
        assert_eq!(counted, report.total);
        assert_eq!(
            report.total,
            report
                .packages
                .iter()
                .map(|p| p.vulnerabilities.len())
                .sum::<usize>()
        );
    }

    #[test]
    fn the_most_severe_package_is_listed_first() {
        let report = Report::build(fixture());
        let severities: Vec<_> = report.packages.iter().filter_map(|p| p.worst()).collect();
        let mut sorted = severities.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(severities, sorted, "packages must be ordered worst-first");
    }

    #[test]
    fn actionable_counts_only_critical_and_high() {
        let report = Report::build(fixture());
        assert_eq!(report.actionable(), report.critical + report.high);
    }

    #[test]
    fn an_empty_scan_is_valid() {
        let report = Report::build(Vec::new());
        assert_eq!(report.total, 0);
        assert_eq!(report.actionable(), 0);
    }

    /// The real scanner. Guards against `brew vulns` changing its JSON shape,
    /// and against its non-zero exit being mistaken for a failure.
    #[tokio::test]
    async fn real_scan_succeeds_despite_a_non_zero_exit() {
        let Ok(brew) = Brew::discover() else { return };
        let report = scan(&brew)
            .await
            .expect("a scan that finds vulnerabilities must not be reported as an error");
        assert_eq!(
            report.total,
            report
                .packages
                .iter()
                .map(|p| p.vulnerabilities.len())
                .sum::<usize>()
        );
    }
}
