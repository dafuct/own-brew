pub mod app;
pub mod brew;
pub mod catalog;
pub mod commands;
pub mod error;
pub mod model;
pub mod ops;
pub mod state;

pub use error::{Error, Result};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "own_brew_lib=info,warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app::App::new())
        .setup(|app| {
            // Warm the catalog while the user is still on the first screen, so
            // search is ready by the time they reach it.
            let handle: tauri::AppHandle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri::Manager;
                let state = handle.state::<app::App>();
                if let Err(e) = state.catalog().await {
                    tracing::warn!(error = %e, "could not preload the catalog");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::environment,
            commands::catalog_stats,
            commands::catalog_reload,
            commands::catalog_search,
            commands::package_detail,
            commands::installed,
            commands::outdated,
            commands::services,
            commands::op_run,
            commands::op_cancel,
            commands::op_active,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
