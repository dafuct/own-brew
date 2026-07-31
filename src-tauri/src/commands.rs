//! The IPC surface.
//!
//! These are thin: they resolve state, delegate, and return. Anything worth
//! testing lives in the modules they call.

use crate::app::{App, Environment};
use crate::catalog::{Page, Query, Stats};
use crate::disk;
use crate::error::Result;
use crate::history::{diff, Change, Operation};
use crate::impact;
use crate::model::detail::Formula;
use crate::model::entry::Kind;
use crate::model::{Detail, Outdated, Service};
use crate::ops::{Action, Event, Request};
use crate::policy::{Decision, Policy};
use crate::rollback::{self, Candidate};
use crate::security;
use crate::state::{self, InstalledPackage, Summary};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledView {
    pub packages: Vec<InstalledPackage>,
    pub summary: Summary,
}

#[tauri::command]
pub async fn environment(app: State<'_, App>) -> Result<Environment> {
    Ok(app.environment().await)
}

#[tauri::command]
pub async fn catalog_stats(app: State<'_, App>) -> Result<Stats> {
    Ok(app.catalog().await?.stats())
}

#[tauri::command]
pub async fn catalog_reload(app: State<'_, App>) -> Result<Stats> {
    Ok(app.reload_catalog().await?.stats())
}

#[tauri::command]
pub async fn catalog_search(app: State<'_, App>, query: Query) -> Result<Page> {
    Ok(app.catalog().await?.search(&query))
}

/// Full detail for one package, straight from `brew info` so it reflects
/// local install state rather than the published catalog.
#[tauri::command]
pub async fn package_detail(app: State<'_, App>, kind: Kind, id: String) -> Result<Detail> {
    crate::ops::plan::args(&Request {
        action: crate::ops::Action::Install,
        kind,
        targets: vec![id.clone()],
    })?; // reuse the id validation

    let brew = app.brew()?;
    let args: Vec<&str> = match kind {
        Kind::Formula => vec!["info", "--json=v2", "--formula", &id],
        Kind::Cask => vec!["info", "--json=v2", "--cask", &id],
    };

    let mut info: crate::model::detail::Info = brew.json(&args).await?;
    match kind {
        Kind::Formula if !info.formulae.is_empty() => {
            Ok(Detail::Formula(Box::new(info.formulae.remove(0))))
        }
        Kind::Cask if !info.casks.is_empty() => Ok(Detail::Cask(Box::new(info.casks.remove(0)))),
        _ => Err(crate::Error::Catalog(format!(
            "Homebrew returned no {} named {id:?}",
            kind.as_str()
        ))),
    }
}

#[tauri::command]
pub async fn installed(app: State<'_, App>) -> Result<InstalledView> {
    let packages = state::installed(app.brew()?).await?;
    let summary = state::summarize(&packages);
    Ok(InstalledView { packages, summary })
}

#[tauri::command]
pub async fn outdated(app: State<'_, App>) -> Result<Outdated> {
    state::outdated(app.brew()?).await
}

#[tauri::command]
pub async fn services(app: State<'_, App>) -> Result<Vec<Service>> {
    state::services(app.brew()?).await
}

/// Run an operation, streaming progress over `channel` until it finishes.
///
/// The installed set is captured either side of the operation so history
/// records what actually changed — including dependencies the user never
/// named. Snapshots are skipped for operations that cannot move a version.
#[tauri::command]
pub async fn op_run(app: State<'_, App>, request: Request, channel: Channel<Event>) -> Result<u64> {
    let brew = app.brew()?;
    let changes_versions = matches!(
        request.action,
        Action::Install | Action::Uninstall | Action::Upgrade
    );

    let before = if changes_versions {
        state::installed(brew).await.ok()
    } else {
        None
    };

    let command = crate::ops::plan::args(&request)
        .map(|args| format!("brew {}", args.join(" ")))
        .unwrap_or_default();

    let record = app.history().ok().and_then(|h| {
        h.begin(request.action, request.kind, &request.targets, &command)
            .ok()
    });

    let outcome = app.runner.run(brew, request, channel).await;

    let changes: Vec<Change> = match (&before, changes_versions) {
        (Some(before), true) => match state::installed(brew).await {
            Ok(after) => diff::diff(before, &after),
            Err(_) => Vec::new(),
        },
        _ => Vec::new(),
    };

    if let (Ok(history), Some(id)) = (app.history(), record) {
        let cancelled = matches!(outcome, Err(crate::Error::Cancelled));
        let error = outcome.as_ref().err().map(|e| e.to_string());
        if let Err(e) = history.finish(id, outcome.is_ok(), cancelled, error.as_deref(), &changes) {
            tracing::warn!(error = %e, "could not record operation history");
        }
    }

    outcome
}

/// Recent operations and what each one changed.
#[tauri::command]
pub async fn history_recent(app: State<'_, App>, limit: usize) -> Result<Vec<Operation>> {
    app.history()?.recent(limit.clamp(1, 500))
}

/// Versions this package could be taken back to.
#[tauri::command]
pub async fn rollback_candidates(
    app: State<'_, App>,
    kind: Kind,
    id: String,
) -> Result<Vec<Candidate>> {
    let brew = app.brew()?;

    // Ask Homebrew what is current, and which versioned formulae exist for it.
    let (current, versioned) = match kind {
        Kind::Formula => {
            let info: crate::model::detail::Info = brew
                .json(&["info", "--json=v2", "--formula", &id])
                .await
                .unwrap_or_default();
            let formula: Option<&Formula> = info.formulae.first();
            (
                formula.and_then(|f| f.active_version().map(str::to_owned)),
                formula
                    .map(|f| f.versioned_formulae.clone())
                    .unwrap_or_default(),
            )
        }
        Kind::Cask => (None, Vec::new()),
    };

    Ok(rollback::candidates(
        brew,
        Some(app.http()),
        app.history().ok(),
        kind,
        &id,
        current.as_deref(),
        &versioned,
    )
    .await)
}

/// Switch back to a version whose keg is still on disk.
#[tauri::command]
pub async fn rollback_restore(app: State<'_, App>, id: String, version: String) -> Result<String> {
    let brew = app.brew()?;
    let before = state::installed(brew).await.ok();

    let record = app.history().ok().and_then(|h| {
        h.begin(
            Action::Upgrade,
            Kind::Formula,
            std::slice::from_ref(&id),
            &format!("restore {id} {version}"),
        )
        .ok()
    });

    let outcome = rollback::restore_local_keg(brew, &id, &version).await;

    let changes = match (&before, state::installed(brew).await) {
        (Some(before), Ok(after)) => diff::diff(before, &after),
        _ => Vec::new(),
    };
    if let (Ok(history), Some(op)) = (app.history(), record) {
        let error = outcome.as_ref().err().map(|e| e.to_string());
        let _ = history.finish(op, outcome.is_ok(), false, error.as_deref(), &changes);
    }

    outcome
}

/// Recover a version that is no longer on disk, install it, and make it live.
///
/// Streamed like any other operation. Nothing is uninstalled: the recovered
/// version is installed alongside the current one and the links are swapped,
/// so returning to the newest version later is another swap.
#[tauri::command]
pub async fn rollback_recover(
    app: State<'_, App>,
    id: String,
    version: String,
    channel: Channel<Event>,
) -> Result<String> {
    let brew = app.brew()?;
    let before = state::installed(brew).await.ok();

    let record = app.history().ok().and_then(|h| {
        h.begin(
            Action::Install,
            Kind::Formula,
            std::slice::from_ref(&id),
            &format!("recover {id} {version}"),
        )
        .ok()
    });

    let outcome = recover(&app, brew, &id, &version, channel).await;

    let changes = match (&before, state::installed(brew).await) {
        (Some(before), Ok(after)) => diff::diff(before, &after),
        _ => Vec::new(),
    };
    if let (Ok(history), Some(op)) = (app.history(), record) {
        let error = outcome.as_ref().err().map(|e| e.to_string());
        let _ = history.finish(op, outcome.is_ok(), false, error.as_deref(), &changes);
    }

    outcome
}

async fn recover(
    app: &State<'_, App>,
    brew: &crate::brew::Brew,
    id: &str,
    version: &str,
    channel: Channel<Event>,
) -> Result<String> {
    let plan = rollback::recovery_plan(brew, app.http(), id, version).await?;

    // Each step streams into the console like any other operation, so the user
    // watches the download finish before their working version is touched.
    for (index, step) in plan.steps(id).into_iter().enumerate() {
        let args: Vec<&str> = step.iter().map(String::as_str).collect();
        let outcome = app.runner.run_raw(brew, &args, channel.clone()).await;

        if let Err(e) = outcome {
            // Steps 0 and 1 leave the machine as it was. A failure at step 2
            // means the old version is gone and the new one did not land, so
            // put the original back before reporting.
            if index == 2 {
                let _ = rollback::reinstall_original(brew, id).await;
                return Err(crate::Error::Catalog(format!(
                    "recovering {id} {version} failed after the current version \
                     was removed, so it has been reinstalled: {e}"
                )));
            }
            return Err(e);
        }
    }

    Ok(plan.formula)
}

/// Put the newest installed version back in charge after a recovery.
#[tauri::command]
pub async fn rollback_return_to_latest(app: State<'_, App>, id: String) -> Result<()> {
    rollback::return_to_latest(app.brew()?, &id).await
}

#[tauri::command]
pub async fn policy_list(app: State<'_, App>) -> Result<Vec<Policy>> {
    app.history()?.policies()
}

#[tauri::command]
pub async fn policy_set(app: State<'_, App>, policy: Policy) -> Result<()> {
    app.history()?.set_policy(&policy)
}

/// Apply the stored rules to whatever is currently outdated.
#[tauri::command]
pub async fn policy_decisions(app: State<'_, App>) -> Result<Vec<Decision>> {
    let brew = app.brew()?;
    let outdated = state::outdated(brew).await?;
    let history = app.history()?;

    // Start the bake clock for anything newly seen.
    for formula in &outdated.formulae {
        if let Some(version) = &formula.current_version {
            let _ = history.observe_version(Kind::Formula, &formula.name, version);
        }
    }
    for cask in &outdated.casks {
        if let Some(version) = &cask.current_version {
            let _ = history.observe_version(Kind::Cask, &cask.name, version);
        }
    }

    Ok(crate::policy::evaluate(
        crate::history::now(),
        &outdated,
        |kind, package| {
            history
                .policy(kind, package)
                .unwrap_or_else(|_| Policy::auto(kind, package))
        },
        |kind, package, version| history.first_seen(kind, package, version).ok().flatten(),
    ))
}

/// Known vulnerabilities across everything installed.
#[tauri::command]
pub async fn security_scan(app: State<'_, App>) -> Result<security::Report> {
    security::scan(app.brew()?).await
}

/// Risk and urgency for everything currently outdated.
#[tauri::command]
pub async fn impact_all(app: State<'_, App>) -> Result<Vec<impact::Assessment>> {
    impact::assess_outdated(app.brew()?, app.http()).await
}

/// What Homebrew costs in disk, and what reclaiming would give back.
#[tauri::command]
pub async fn disk_footprint(app: State<'_, App>) -> Result<disk::Footprint> {
    disk::footprint(app.brew()?).await
}

#[tauri::command]
pub fn op_cancel(app: State<'_, App>, id: u64) -> Result<()> {
    app.runner.cancel(id)
}

#[tauri::command]
pub fn op_active(app: State<'_, App>) -> Vec<u64> {
    app.runner.active()
}
