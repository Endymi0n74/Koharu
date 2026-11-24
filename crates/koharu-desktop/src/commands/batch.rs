//! Folder batch translation driven from the app: `koharu-batch` as a child
//! process.
//!
//! [`start_batch`] spawns the CLI and waits for it to finish. Errors are
//! reported back to the interface.

use std::path::Path;
use std::process::Stdio;

use tauri::{WebviewWindow};
use tauri_runtime_cef::CefRuntime;
use tokio::io::{AsyncBufReadExt as _, BufReader};

use super::{Error, Result};

/// Folder picker for the batch dialog.
#[tauri::command]
#[specta::specta]
pub(crate) async fn pick_batch_folder(
    window: WebviewWindow<CefRuntime>,
) -> std::result::Result<Option<String>, Error> {
    let dialog = rfd::AsyncFileDialog::new().set_parent(&window);
    Ok(dialog
        .pick_folder()
        .await
        .map(|folder| folder.path().to_string_lossy().into_owned()))
}

/// Resolves the `koharu-batch` binary next to the running app, then in the
/// workspace's target folder (development), then through `KOHARU_BATCH_BIN`.
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
    return Err(anyhow::anyhow!(
        "{executable} was not found; build it with \
         `cargo build --release -p koharu-batch` \
         or point KOHARU_BATCH_BIN at it"
    )
    .into())
}

/// Translates a folder with `koharu-batch` as a child process. Blocks until
/// the child exits, then reports success or failure.
#[tauri::command]
pub(crate) async fn start_batch(
    input: String,
    output: String,
    lang: String,
    translator: Option<String>,
    deterministic: bool,
    overwrite: bool,
) -> Result<()> {
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
    if let Some(translator) = translator.as_deref() {
        if !translator.is_empty() {
            command.arg("--translator").arg(translator);
        }
    }
    if deterministic {
        command.arg("--deterministic");
    }
    if overwrite {
        command.arg("--overwrite");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Err(anyhow::anyhow!("could not start {}: {error:#}", binary).into());
        }
    };

    let Some(stderr) = child.stderr.take() else {
        return Err(anyhow::anyhow!("koharu-batch did not expose stderr").into());
    };

    let mut lines = BufReader::new(stderr).lines();
    let mut last_failure: Option<String> = None;

    while let Some(line) = lines.next_line().await? {
        let lower = line.to_ascii_lowercase();
        if lower.contains("failed") || lower.contains("error") {
            last_failure = Some(line);
        }
    }

    let status = child.wait().await?;
    if status.success() {
        Ok(())
    } else {
        let detail = last_failure
            .unwrap_or_else(|| match status.code() {
                Some(code) => format!("koharu-batch exited with code {code}"),
                None => "koharu-batch exited without a code".to_owned(),
            });
        Err(anyhow::anyhow!("{detail}").into())
    }
}
