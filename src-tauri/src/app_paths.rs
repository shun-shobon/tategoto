use anyhow::Context;
use tauri::{Manager, Runtime};

use crate::model::AppPaths;

pub(crate) fn build_paths<R: Runtime>(app: &tauri::AppHandle<R>) -> anyhow::Result<AppPaths> {
    let config_dir = app.path().app_config_dir()?;
    let output_directory = dirs::document_dir()
        .or_else(dirs::home_dir)
        .context("Documents directory not found")?
        .join("Tategoto");
    Ok(AppPaths {
        config_file: config_dir.join("settings.json"),
        output_directory,
    })
}
