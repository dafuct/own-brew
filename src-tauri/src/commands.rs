//! The IPC surface.
//!
//! These are thin: they resolve state, delegate, and return. Anything worth
//! testing lives in the modules they call.

use crate::app::{App, Environment};
use crate::catalog::{Page, Query, Stats};
use crate::error::Result;
use crate::model::entry::Kind;
use crate::model::{Detail, Outdated, Service};
use crate::ops::{Event, Request};
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
#[tauri::command]
pub async fn op_run(
    app: State<'_, App>,
    request: Request,
    channel: Channel<Event>,
) -> Result<u64> {
    app.runner.run(app.brew()?, request, channel).await
}

#[tauri::command]
pub fn op_cancel(app: State<'_, App>, id: u64) -> Result<()> {
    app.runner.cancel(id)
}

#[tauri::command]
pub fn op_active(app: State<'_, App>) -> Vec<u64> {
    app.runner.active()
}
