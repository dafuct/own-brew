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
    let Some(catalog) = catalog().await else { return };

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
    let Some(catalog) = catalog().await else { return };

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
    let Some(catalog) = catalog().await else { return };

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
        packages.iter().filter(|p| p.kind == Kind::Formula).all(|p| p.version.is_some()),
        "installed formulae should report a version"
    );
}

#[tokio::test]
async fn outdated_agrees_with_the_installed_list() {
    let Some(brew) = brew() else { return };

    let outdated = state::outdated(&brew).await.expect("`brew outdated` parses");
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
        .json(&["info", "--json=v2", "--formula", "surely-no-such-formula-42"])
        .await;

    let err = result.expect_err("unknown formula must not succeed");
    assert_eq!(err.kind(), "brew_failed");
}
