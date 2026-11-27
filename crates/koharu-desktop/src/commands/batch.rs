//! Folder batch translation driven from the app: `koharu-batch` as a child
//! process.
//!
//! [`start_batch`] spawns the CLI and waits for it to finish. Errors are
//! reported back to the interface.

use std::path::Path;
use std::process::Stdio;

use tauri::{Window, ipc::Channel};
use tauri_runtime_cef::CefRuntime;
use tokio::io::{AsyncBufReadExt as _, BufReader};

use super::{Error, Result};

/// Folder picker for the batch dialog.
#[tauri::command]
#[specta::specta]
pub(crate) async fn pick_batch_folder(
    window: Window<CefRuntime>,
) -> std::result::Result<Option<String>, Error> {
    let dialog = rfd::AsyncFileDialog::new().set_parent(&window);
    Ok(dialog
        .pick_folder()
        .await
        .map(|folder| folder.path().to_string_lossy().into_owned()))
}

/// Folder picker for the output a batch writes translated images to.
#[tauri::command]
#[specta::specta]
pub(crate) async fn pick_batch_output(
    window: Window<CefRuntime>,
) -> std::result::Result<Option<String>, Error> {
    let dialog = rfd::AsyncFileDialog::new()
        .set_parent(&window)
        .set_title("Save translated images")
        .pick_folder();
    Ok(dialog
        .await
        .map(|folder| folder.path().to_string_lossy().into_owned()))
}

/// Resolves the `koharu-batch` binary: `KOHARU_BATCH_BIN`, then next to the
/// running app, then in the workspace's target folder (development).
fn batch_binary() -> Result<String> {
    if let Some(explicit) = std::env::var_os("KOHARU_BATCH_BIN") {
        let path = Path::new(&explicit);
        if path.exists() {
            return Ok(explicit.to_string_lossy().into_owned());
        }
        return Err(anyhow::anyhow!(
            "KOHARU_BATCH_BIN points at {} which does not exist",
            explicit.to_string_lossy()
        )
        .into());
    }
    let executable = format!("koharu-batch{}", std::env::consts::EXE_SUFFIX);
    if let Ok(current) = std::env::current_exe()
        && let Some(directory) = current.parent()
    {
        let sibling = directory.join(&executable);
        if sibling.exists() {
            return Ok(sibling.to_string_lossy().into_owned());
        }
    }
    // Development: the app runs from target/{debug,release} of its workspace.
    if let Ok(current) = std::env::current_dir() {
        for profile in ["release", "debug"] {
            let candidate = current.join("target").join(profile).join(&executable);
            if candidate.exists() {
                return Ok(candidate.to_string_lossy().into_owned());
            }
        }
    }
    Err(anyhow::anyhow!(
        "{executable} was not found; build it with \
         `cargo build --release -p koharu-batch` \
         or point KOHARU_BATCH_BIN at it"
    )
    .into())
}

/// Translates a folder with `koharu-batch` as a child process. Blocks until
/// the child exits, then reports the project file it wrote, or a failure;
/// `on_progress` hears each line the run prints, so a long chapter says where
/// it is.
#[tauri::command]
#[specta::specta]
pub(crate) async fn start_batch(
    input: String,
    output: String,
    lang: String,
    translator: Option<String>,
    on_progress: Channel<String>,
) -> Result<String> {
    if !Path::new(&input).exists() {
        return Err(anyhow::anyhow!("the input folder {} does not exist", input).into());
    }
    let binary = batch_binary()?;

    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("run")
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--target")
        .arg(&lang);
    // Re-run on a folder that already holds a project: the CLI makes a new
    // project there otherwise. Keep the version being replaced, so a run that
    // goes wrong can be recovered; the `-wal` file holds the commits the
    // project has not checkpointed yet, so it is copied with the file.
    let project = Path::new(&output).join("project.khr");
    if project.exists() {
        let backup = Path::new(&output).join("project.khr.bak");
        std::fs::copy(&project, &backup).map_err(|error| {
            anyhow::anyhow!(
                "could not back up {} to {}: {error}",
                project.display(),
                backup.display()
            )
        })?;
        let wal = Path::new(&output).join("project.khr-wal");
        let wal_backup = Path::new(&output).join("project.khr-wal.bak");
        if wal.exists() {
            std::fs::copy(&wal, &wal_backup).map_err(|error| {
                anyhow::anyhow!(
                    "could not back up {} to {}: {error}",
                    wal.display(),
                    wal_backup.display()
                )
            })?;
        } else if wal_backup.exists() {
            std::fs::remove_file(&wal_backup).map_err(|error| {
                anyhow::anyhow!("could not remove {}: {error}", wal_backup.display())
            })?;
        }
        command.arg("--overwrite");
    }
    if let Some(translator) = translator.as_deref()
        && !translator.is_empty()
    {
        command.arg("--translator").arg(translator);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Err(anyhow::anyhow!("could not start {}: {error:#}", binary).into());
        }
    };

    let Some(stdout) = child.stdout.take() else {
        return Err(anyhow::anyhow!("koharu-batch did not expose stdout").into());
    };
    let Some(stderr) = child.stderr.take() else {
        return Err(anyhow::anyhow!("koharu-batch did not expose stderr").into());
    };

    // stderr carries the run's diagnostics, and its last line is what a failed
    // exit is blamed on; stdout is the progress.
    let diagnostics = tokio::spawn(async move {
        let mut last = None;
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            last = Some(line);
        }
        last
    });
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        // Nobody listening isn't a failure.
        let _ = on_progress.send(line);
    }
    let failure = diagnostics.await?;

    let status = child.wait().await?;
    if status.success() {
        let project_path = Path::new(&output).join("project.khr");
        Ok(project_path.to_string_lossy().into_owned())
    } else {
        let detail = failure.unwrap_or_else(|| match status.code() {
            Some(code) => format!("koharu-batch exited with code {code}"),
            None => "koharu-batch exited without a code".to_owned(),
        });
        Err(anyhow::anyhow!("{detail}").into())
    }
}
