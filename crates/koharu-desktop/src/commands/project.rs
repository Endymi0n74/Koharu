//! The projects folder and the open project.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, ensure};
use koharu_scene::Session;
use tauri::State;

use super::Result;
use crate::app::App;

#[tauri::command]
#[specta::specta]
pub fn list_projects(app: State<'_, Arc<App>>) -> Result<Vec<String>> {
    Ok(app.library.list()?)
}

/// Creates a project and opens it.
#[tauri::command]
#[specta::specta]
pub async fn create(app: State<'_, Arc<App>>, name: String) -> Result<()> {
    let app = Arc::clone(&app);
    Ok(tokio::task::spawn_blocking(move || {
        let session = app.library.create(&name)?;
        app.open(name, session)
    })
    .await??)
}

#[tauri::command]
#[specta::specta]
pub async fn open(app: State<'_, Arc<App>>, name: String) -> Result<()> {
    let app = Arc::clone(&app);
    Ok(tokio::task::spawn_blocking(move || {
        let session = app.library.open(&name)?;
        app.open(name, session)
    })
    .await??)
}

/// Deletes a project, closing it first if it's the open one.
#[tauri::command]
#[specta::specta]
pub fn delete_project(app: State<'_, Arc<App>>, name: String) -> Result<()> {
    if app
        .project
        .lock()
        .as_ref()
        .is_some_and(|project| project.name == name)
    {
        app.close();
    }
    Ok(app.library.delete(&name)?)
}

#[tauri::command]
#[specta::specta]
pub fn close(app: State<'_, Arc<App>>) {
    app.close();
}

/// Open a project by its `.khr` file path. Bypasses the library's name-based
/// lookup, so the app can open a project created by `koharu-batch` outside
/// the projects folder.
#[tauri::command]
#[specta::specta]
pub async fn open_project_path(app: State<'_, Arc<App>>, path: String) -> Result<()> {
    let app = Arc::clone(&app);
    Ok(tokio::task::spawn_blocking(move || {
        let path = Path::new(&path);
        ensure!(
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("khr")),
            "{path:?} is not a .khr project file"
        );
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("imported")
            .to_owned();
        let session =
            Session::open(path).with_context(|| format!("couldn't open {}", path.display()))?;
        app.open(name, session)
    })
    .await??)
}

/// The open project's name, for an interface that loaded after it opened.
#[tauri::command]
#[specta::specta]
pub fn get_open_project(app: State<'_, Arc<App>>) -> Option<String> {
    app.project
        .lock()
        .as_ref()
        .map(|project| project.name.clone())
}
