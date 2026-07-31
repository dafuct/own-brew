//! End-to-end checks against the Homebrew installation on this machine.
//!
//! These exercise the real pipeline — real `brew` invocations, the real
//! catalog cache, real JSON — because the failure mode that matters most for
//! this app is Homebrew changing its output, which a mocked test cannot catch.
//!
//! Every test degrades to a no-op when Homebrew is absent, so the suite still
//! passes on a machine (or CI runner) without it.

use own_brew_lib::brew::Brew;
use own_brew_lib::catalog::{Catalog, Query, Sort};
use own_brew_lib::model::entry::Kind;
use own_brew_lib::state;

fn brew() -> Option<Brew> {
    Brew::discover().ok()
}

async fn catalog() -> Option<Catalog> {
    let brew = brew()?;
    let http = reqwest::Client::new();
    Catalog::load(&http, brew.cache_dir()).await.ok()
}

#[tokio::test]
async fn loads_the_whole_catalog() {
    let Some(catalog) = catalog().await else {
        eprintln!("skipped: Homebrew is not installed");
        return;
    };
    let stats = catalog.stats();

    assert!(
        stats.formulae > 5_000,
        "expected thousands of formulae, got {}",
        stats.formulae
    );
    assert!(
        stats.casks > 3_000,
        "expected thousands of casks, got {}",
        stats.casks
    );
}

#[tokio::test]
async fn finds_well_known_packages_by_name() {
    let Some(catalog) = catalog().await else {
        return;
    };

    for (kind, id) in [(Kind::Formula, "git"), (Kind::Formula, "wget")] {
        let page = catalog.search(&Query {
            text: id.to_owned(),
            kind: Some(kind),
            ..Default::default()
        });
        assert_eq!(
            page.items.first().map(|e| e.id.as_str()),
            Some(id),
            "searching {id:?} should rank it first"
        );
    }
}

#[tokio::test]
async fn searching_by_description_finds_packages_whose_name_does_not_match() {
    let Some(catalog) = catalog().await else {
        return;
    };

    let page = catalog.search(&Query {
        text: "json processor".to_owned(),
        kind: Some(Kind::Formula),
        limit: 25,
        ..Default::default()
    });

    assert!(page.total > 0, "description search returned nothing");
    assert!(
        page.items.iter().any(|e| e.id == "jq"),
        "jq should surface for 'json processor'"
    );
}

#[tokio::test]
async fn paging_covers_every_result_exactly_once() {
    let Some(catalog) = catalog().await else {
        return;
    };

    let query = |offset| Query {
        kind: Some(Kind::Cask),
        sort: Sort::Name,
        limit: 500,
        offset,
        ..Default::default()
    };

    let first = catalog.search(&query(0));
    let second = catalog.search(&query(500));

    let mut ids: Vec<&str> = first
        .items
        .iter()
        .chain(second.items.iter())
        .map(|e| e.id.as_str())
        .collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();

    assert_eq!(before, ids.len(), "pages overlapped");
    assert_eq!(before, 1000.min(first.total), "pages skipped results");
}

#[tokio::test]
async fn reads_the_installed_package_list() {
    let Some(brew) = brew() else { return };

    let packages = state::installed(&brew)
        .await
        .expect("`brew info --installed` should parse");
    let summary = state::summarize(&packages);

    assert_eq!(summary.formulae + summary.casks, packages.len());
    assert!(
        packages.iter().all(|p| !p.id.is_empty()),
        "every installed package needs an id"
    );
    // Anything installed must report the version that is actually in use.
    assert!(
        packages
            .iter()
            .filter(|p| p.kind == Kind::Formula)
            .all(|p| p.version.is_some()),
        "installed formulae should report a version"
    );
}

#[tokio::test]
async fn outdated_agrees_with_the_installed_list() {
    let Some(brew) = brew() else { return };

    let outdated = state::outdated(&brew)
        .await
        .expect("`brew outdated` parses");
    let installed = state::installed(&brew).await.expect("`brew info` parses");

    // Everything reported as outdated must actually be installed.
    for formula in &outdated.formulae {
        assert!(
            installed
                .iter()
                .any(|p| p.kind == Kind::Formula && p.id == formula.name),
            "{} is outdated but not installed",
            formula.name
        );
    }
}

#[tokio::test]
async fn a_read_only_brew_command_streams_output() {
    let Some(brew) = brew() else { return };

    let mut stream = brew.stream(&["--version"]).expect("spawn");
    let mut lines = Vec::new();
    while let Some(line) = stream.next_line().await {
        lines.push(line.text);
    }
    let status = stream.wait().await.expect("wait");

    assert!(status.success());
    assert!(
        lines.iter().any(|l| l.contains("Homebrew")),
        "expected a version banner, got {lines:?}"
    );
}

#[tokio::test]
async fn detail_for_an_unknown_package_fails_cleanly() {
    let Some(brew) = brew() else { return };

    let result: own_brew_lib::Result<own_brew_lib::model::detail::Info> = brew
        .json(&[
            "info",
            "--json=v2",
            "--formula",
            "surely-no-such-formula-42",
        ])
        .await;

    let err = result.expect_err("unknown formula must not succeed");
    assert_eq!(err.kind(), "brew_failed");
}

// ------------------------------------------------------------- phase 3 ---

/// The security scan against the real machine.
///
/// `brew vulns` exits non-zero when it finds something, so a successful scan
/// here also proves that exit code is not mistaken for a failure.
#[tokio::test]
async fn security_scan_against_the_real_machine() {
    let Some(brew) = brew() else { return };
    let report = own_brew_lib::security::scan(&brew)
        .await
        .expect("a scan that finds vulnerabilities must not be an error");

    assert_eq!(
        report.critical + report.high + report.medium + report.low + report.unknown,
        report.total
    );
    for package in &report.packages {
        assert!(!package.formula.is_empty());
        assert!(
            package.vulnerabilities.iter().all(|v| !v.id.is_empty()),
            "{} reported an advisory with no identifier",
            package.formula
        );
    }
}

/// The full impact assessment over whatever is actually outdated.
#[tokio::test]
async fn impact_assessment_over_real_pending_updates() {
    let Some(brew) = brew() else { return };
    let http = reqwest::Client::new();

    let Ok(assessments) = own_brew_lib::impact::assess_outdated(&brew, &http).await else {
        return; // offline: analytics unavailable
    };

    for assessment in &assessments {
        assert!(!assessment.package.is_empty());
        assert!(
            !assessment.reasons.is_empty(),
            "{} was assessed with no explanation, which the UI would render blank",
            assessment.package
        );
        // A security finding must never be reported without a severity.
        if assessment.known_vulnerabilities > 0 {
            assert!(assessment.worst_severity.is_some());
        }
    }

    // Sorted most-urgent first.
    let urgencies: Vec<_> = assessments.iter().map(|a| a.urgency).collect();
    let mut sorted = urgencies.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(
        urgencies, sorted,
        "assessments must lead with the urgent ones"
    );
}

/// The disk footprint, and the claim that reclaiming costs you your undo.
#[tokio::test]
async fn disk_footprint_matches_the_rollback_targets() {
    let Some(brew) = brew() else { return };
    let footprint = own_brew_lib::disk::footprint(&brew)
        .await
        .expect("disk scan");

    // Every byte offered as reclaimable belongs to a keg that really exists,
    // and every such keg is one the rollback engine would offer to restore.
    for keg in &footprint.superseded {
        let path =
            own_brew_lib::rollback::cellar::rack(brew.prefix(), &keg.formula).join(&keg.version);
        assert!(
            path.is_dir(),
            "{} {} is not on disk",
            keg.formula,
            keg.version
        );
        assert!(
            keg.bytes > 0,
            "{} {} measured as empty",
            keg.formula,
            keg.version
        );
    }

    assert!(footprint.superseded_bytes <= footprint.cellar_bytes);
    assert!(footprint.total_bytes >= footprint.cellar_bytes);
}

// ------------------------------------------------------------- phase 4 ---

/// Recovering a version that is not on disk.
///
/// This is the product's central promise generalised: the formula is located
/// in homebrew-core's history, written into own-brew's tap, and Homebrew
/// itself confirms the version before anything would be installed.
///
/// Skips when GitHub's anonymous rate limit is exhausted.
#[tokio::test]
async fn recovers_a_version_that_is_not_on_disk() {
    let Some(brew) = brew() else { return };
    let http = reqwest::Client::builder()
        .user_agent("own-brew-test")
        .build()
        .unwrap();

    // A version of jq that is not installed here.
    let plan = match own_brew_lib::rollback::recovery_plan(&brew, &http, "jq", "1.8.1").await {
        Ok(plan) => plan,
        Err(e) if e.to_string().contains("rate limit") => return,
        Err(e) if e.to_string().contains("already on disk") => return,
        Err(e) => panic!("could not recover jq 1.8.1: {e}"),
    };

    // The file keeps the original name: `jq@1.8.1` would make Homebrew look
    // for the bottle at homebrew/core/jq/1.8.1 and 404.
    assert_eq!(plan.formula, "own-brew/rollback/jq");

    // Fetch first, and only then remove what is installed: a failed download
    // must never leave the machine without the package.
    let steps = plan.steps("jq");
    assert_eq!(steps[0][0], "fetch");
    assert_eq!(steps[1][0], "uninstall");
    assert_eq!(steps[2][0], "install");
    assert_eq!(plan.commit.len(), 40, "provenance must name a real commit");
    assert!(plan.provenance.contains("homebrew-core"));

    // Homebrew must agree, independently, that this is jq 1.8.1 and poured
    // from a bottle rather than built from source.
    let info: own_brew_lib::model::detail::Info = brew
        .json(&["info", "--json=v2", "--formula", &plan.formula])
        .await
        .expect("brew should recognise the materialised formula");
    let formula = info.formulae.first().expect("one formula");
    assert_eq!(formula.versions.stable.as_deref(), Some("1.8.1"));
    assert!(formula.versions.bottle, "a bottle is needed to install it");

    // Asking for a version that never existed must fail cleanly rather than
    // materialising something wrong.
    let bogus = own_brew_lib::rollback::recovery_plan(&brew, &http, "jq", "999.999.999").await;
    assert!(
        bogus.is_err(),
        "a version that never shipped must not resolve"
    );

    own_brew_lib::rollback::tap::discard(&brew, "jq", "1.8.1").expect("cleanup");
}
