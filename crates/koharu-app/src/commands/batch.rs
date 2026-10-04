//! Folder batch runs driven from the app: `koharu-batch` as a child process.
//!
//! [`start_batch`] spawns the CLI and maps its stderr progress onto the same
//! [`Job`] tables the pipeline uses, so a folder run appears in the activity
//! center, is stopped by [`stop_job`](super::processing::stop_job) and retires
//! like any other job. The child keeps its own VRAM budget, calibration file
//! and exit-code contract — nothing of the batch CLI is re-implemented here.

use std::path::PathBuf;
use std::process::Stdio;
use std::str::FromStr as _;
use std::time::Duration;

use anyhow::Result;
use koharu_pipeline::{Stage, StopToken};
use koharu_translator::{ModelSelection, Provider};
use tauri::{AppHandle, Manager as _, State, WebviewWindow};
use tauri_runtime_cef::CefRuntime;
use tokio::io::{AsyncBufReadExt as _, BufReader};

use super::{
    ChannelExt as _, Error,
    processing::{Job, JobChannel, JobGuard, JobId, JobState, Processing},
};

/// How often the reader re-checks the stop token between stderr lines.
const STOP_POLL: Duration = Duration::from_millis(200);

/// Folder picker for the batch dialog: the paths are kept in the frontend
/// until [`start_batch`] is called, like the import dialog keeps its files.
#[tauri::command]
#[specta::specta]
pub(crate) async fn pick_batch_folder(
    window: WebviewWindow<CefRuntime>,
) -> std::result::Result<Option<PathBuf>, Error> {
    let dialog = rfd::AsyncFileDialog::new().set_parent(&window);
    Ok(dialog
        .pick_folder()
        .await
        .map(|folder| folder.path().to_owned()))
}

/// Resolves the `koharu-batch` binary next to the running app, then in the
/// workspace's target folder (development), then through `KOHARU_BATCH_BIN`.
fn batch_binary() -> Result<PathBuf> {
    if let Some(explicit) = std::env::var_os("KOHARU_BATCH_BIN") {
        let path = PathBuf::from(explicit);
        if path.exists() {
            return Ok(path);
        }
        anyhow::bail!(
            "KOHARU_BATCH_BIN points at {} which does not exist",
            path.display()
        );
    }
    let executable = format!("koharu-batch{}", std::env::consts::EXE_SUFFIX);
    if let Ok(current) = std::env::current_exe()
        && let Some(directory) = current.parent()
    {
        let sibling = directory.join(&executable);
        if sibling.exists() {
            return Ok(sibling);
        }
    }
    // Development: the app runs from target/{debug,release} of its workspace.
    if let Ok(current) = std::env::current_dir() {
        for profile in ["release", "debug"] {
            let candidate = current.join("target").join(profile).join(&executable);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!(
        "{executable} was not found; build it with \
         `cargo build --release -p koharu-pipeline --bin koharu-batch` \
         or point KOHARU_BATCH_BIN at it"
    )
}

/// Pages counted from the batch stderr, one snapshot per publish.
#[derive(Default)]
struct BatchProgress {
    completed: usize,
    total: usize,
    stage: Option<Stage>,
    /// Label announced once the run resolves its model, e.g.
    /// `gemma4-e2b-it Q4_K_XL` or `gpt-5.6-luna (openai)`.
    model: Option<String>,
}

impl BatchProgress {
    /// Folds one stderr line into the counters; returns whether the job
    /// changed and so needs republishing.
    fn observe(&mut self, line: &str) -> bool {
        if let Some(model) = parse_model(line) {
            if self.model.as_deref() == Some(model) {
                return false;
            }
            self.model = Some(model.to_owned());
            return true;
        }
        if let Some(pending) = parse_pending(line) {
            self.total += pending;
            return true;
        }
        if parse_done(line) {
            self.completed += 1;
            return true;
        }
        if let Some((_, _, stage)) = parse_stage_line(line)
            && self.stage != Some(stage)
        {
            self.stage = Some(stage);
            return true;
        }
        false
    }
}

/// `4 pages (3 to translate) -> out` — the run's page total, once per chapter.
fn parse_pending(line: &str) -> Option<usize> {
    let start = line.find(" (")?;
    let (count, rest) = line[start + 2..].split_once(' ')?;
    let pending: usize = count.parse().ok()?;
    rest.starts_with("to translate)").then_some(pending)
}

/// `translation model: <id> <quantization> (…estimate…)` for a local run,
/// `translation model: <id> (<provider>) — hosted; …` for a hosted one —
/// the label the activity row shows while the run lasts.
fn parse_model(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("translation model: ")?;
    let head = rest.split(" — ").next().unwrap_or(rest);
    // A local line carries the VRAM parenthesis, which opens with its figure
    // (`(2.3 GiB, within 5.0 GiB, …`); a hosted one keeps `(provider)` — the
    // backend is part of the label there.
    match head.find(" (") {
        Some(index)
            if head[index + 2..].starts_with(|character: char| character.is_ascii_digit()) =>
        {
            Some(head[..index].trim())
        }
        _ => Some(head.trim()),
    }
}

/// `[page.png] done in 12.31s …` — a page that finished every phase.
fn parse_done(line: &str) -> bool {
    line.match_indices(']')
        .any(|(index, _)| line[index + 1..].trim_start().starts_with("done in"))
}

/// `  [chapter] [2/4] translation · 1.02s · ETA …` — phase counter and stage.
///
/// The chapter label is bracketed too, so every bracket is tried until the
/// `n/m` shape matches; `[page.png] failed to read` never does.
fn parse_stage_line(line: &str) -> Option<(usize, usize, Stage)> {
    for (index, _) in line.match_indices('[') {
        let rest = &line[index + 1..];
        let Some(close) = rest.find(']') else {
            continue;
        };
        let (count, tail) = rest.split_at(close);
        let Some((done, total)) = count.split_once('/') else {
            continue;
        };
        let (Ok(done), Ok(total)) = (done.trim().parse(), total.trim().parse()) else {
            continue;
        };
        let Some(stage) = tail
            .strip_prefix(']')
            .and_then(|tail| tail.split('·').next())
        else {
            continue;
        };
        let Ok(stage) = Stage::from_str(stage.trim()) else {
            continue;
        };
        return Some((done, total, stage));
    }
    None
}

/// Whether a stderr line is worth keeping as the failure message of a run
/// that exits non-zero (`translation failed: …`, `2 page(s) failed:`).
fn is_failure(line: &str) -> bool {
    let line = line.to_ascii_lowercase();
    line.contains("failed") || line.contains("error")
}

/// Applies a mutation to the running job and republishes it.
fn update_job(handle: &AppHandle<CefRuntime>, id: JobId, update: impl FnOnce(&mut Job)) {
    let processing = handle.state::<Processing>();
    let mut jobs = processing.jobs.lock();
    let Some(job) = jobs.get_mut(&id) else {
        return;
    };
    update(job);
    let job = job.clone();
    drop(jobs);
    handle.state::<JobChannel>().channel.publish(job);
}

/// How the child process ended.
enum Outcome {
    /// The stop token fired and the child was killed.
    Stopped,
    /// The child exited on its own.
    Status(std::process::ExitStatus),
    /// The pipe broke before an exit status was seen.
    Failed(String),
}

/// The child's flags for a picked model: a hosted provider is named
/// explicitly, a local pick falls back to the CLI's `auto` default when the
/// picker chose none, and `--no-vision` carries a pick that cannot take the
/// page image. Reasoning stays out of the flags: the catalog derives it for
/// a local model, and a hosted endpoint keeps its own default.
fn model_arguments(selection: &ModelSelection) -> Vec<std::ffi::OsString> {
    let mut arguments: Vec<std::ffi::OsString> = Vec::new();
    if selection.provider != Provider::Local {
        let provider: &'static str = selection.provider.into();
        arguments.push("--provider".into());
        arguments.push(provider.into());
    }
    if let Some(model) = &selection.model {
        arguments.push("--llm".into());
        arguments.push(model.as_str().into());
    }
    if let Some(quantization) = &selection.quantization {
        arguments.push("--quantization".into());
        arguments.push(quantization.as_str().into());
    }
    if !selection.vision {
        arguments.push("--no-vision".into());
    }
    arguments
}

/// Translates a folder (or chapter) with `koharu-batch` and reports it as a
/// regular job: the child owns the run, this side only mirrors its stderr.
/// `model` is the picker's choice; `None` translates like the CLI's own
/// default — the local `auto` pick.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn start_batch(
    handle: AppHandle<CefRuntime>,
    input: PathBuf,
    output: PathBuf,
    lang: String,
    model: Option<ModelSelection>,
    deterministic: bool,
    overwrite: bool,
    processing: State<'_, Processing>,
    job_channel: State<'_, JobChannel>,
) -> std::result::Result<JobId, Error> {
    if !input.exists() {
        return Err(anyhow::anyhow!("the input folder {} does not exist", input.display()).into());
    }
    let binary = batch_binary()?;

    let mut command = tokio::process::Command::new(&binary);
    command
        .arg("--input")
        .arg(&input)
        .arg("--output")
        .arg(&output)
        .arg("--lang")
        .arg(&lang)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(selection) = &model {
        command.args(model_arguments(selection));
    }
    if deterministic {
        command.arg("--deterministic");
    }
    if overwrite {
        command.arg("--overwrite");
    }

    let id = JobId::new();
    let stop = StopToken::default();
    {
        let mut stops = processing.stops.lock();
        if !stops.is_empty() {
            return Err(anyhow::anyhow!("another process is already running").into());
        }
        stops.insert(id, stop.clone());
    }
    let job = Job {
        id,
        state: JobState::Running,
        completed: 0,
        total: 0,
        page: None,
        stage: None,
        model: Some("koharu-batch".to_owned()),
        error: None,
    };
    processing.jobs.lock().insert(id, job.clone());
    job_channel.channel.publish(job);
    tracing::info!(
        target: "koharu_metrics",
        metric = "batch_start",
        lang = %lang,
        deterministic,
        overwrite,
    );

    let task_handle = handle.clone();
    drop(tokio::spawn(async move {
        let mut guard = JobGuard::new(task_handle.clone(), id);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                guard.finish(
                    false,
                    Some(format!("could not start {}: {error:#}", binary.display())),
                );
                return;
            }
        };
        let Some(stderr) = child.stderr.take() else {
            guard.finish(false, Some("koharu-batch did not expose stderr".to_owned()));
            return;
        };
        let mut lines = BufReader::new(stderr).lines();
        let mut progress = BatchProgress::default();
        let mut last_failure: Option<String> = None;
        let outcome = loop {
            if stop.stopped() {
                let _ = child.start_kill();
                let _ = child.wait().await;
                break Outcome::Stopped;
            }
            tokio::select! {
                line = lines.next_line() => match line {
                    Ok(Some(line)) => {
                        let failure = is_failure(&line);
                        let changed = progress.observe(&line);
                        if failure {
                            last_failure = Some(line);
                        }
                        if changed {
                            let completed = progress.completed;
                            let total = progress.total;
                            let stage = progress.stage;
                            let model = progress.model.clone();
                            update_job(&task_handle, id, |job| {
                                job.completed = completed;
                                job.total = total;
                                job.stage = stage;
                                if let Some(model) = model {
                                    job.model = Some(model);
                                }
                            });
                        }
                    }
                    Ok(None) => match child.wait().await {
                        Ok(status) => break Outcome::Status(status),
                        Err(error) => break Outcome::Failed(format!("could not wait for koharu-batch: {error}")),
                    },
                    Err(error) => {
                        if stop.stopped() {
                            let _ = child.start_kill();
                            let _ = child.wait().await;
                            break Outcome::Stopped;
                        }
                        let _ = child.start_kill();
                        let _ = child.wait().await;
                        break Outcome::Failed(format!("koharu-batch stderr broke: {error}"));
                    }
                },
                () = tokio::time::sleep(STOP_POLL) => {}
            }
        };
        let (stopped, error) = match outcome {
            Outcome::Stopped => (true, None),
            Outcome::Status(status) if status.success() => (false, None),
            Outcome::Status(status) => {
                let detail = last_failure.unwrap_or_else(|| match status.code() {
                    Some(code) => format!("koharu-batch exited with code {code}"),
                    None => "koharu-batch exited without a code".to_owned(),
                });
                (false, Some(detail))
            }
            Outcome::Failed(message) => (false, Some(message)),
        };
        tracing::info!(
            target: "koharu_metrics",
            metric = "batch_result",
            outcome = if stopped { "stopped" } else if error.is_some() { "failed" } else { "completed" },
        );
        guard.finish(stopped, error);
        // Dropping the guard retires the job and frees the slot for the next
        // run, panic included — same contract as the pipeline's jobs.
    }));
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags_of(selection: &ModelSelection) -> Vec<String> {
        model_arguments(selection)
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect()
    }

    fn selection(
        provider: Provider,
        model: Option<&str>,
        quantization: Option<&str>,
        vision: bool,
    ) -> ModelSelection {
        ModelSelection {
            provider,
            model: model.map(str::to_owned),
            quantization: quantization.map(str::to_owned),
            vision,
            reasoning: false,
        }
    }

    #[test]
    fn picked_models_map_onto_the_child_flags() {
        assert_eq!(
            flags_of(&selection(
                Provider::OpenAi,
                Some("gpt-5.6-luna"),
                None,
                true
            )),
            ["--provider", "openai", "--llm", "gpt-5.6-luna"]
        );
        assert_eq!(
            flags_of(&selection(Provider::DeepL, None, None, false)),
            ["--provider", "deepl", "--no-vision"]
        );
        assert_eq!(
            flags_of(&selection(
                Provider::Local,
                Some("gemma4-e4b-it"),
                Some("Q4_K_XL"),
                true
            )),
            ["--llm", "gemma4-e4b-it", "--quantization", "Q4_K_XL"]
        );
        assert!(
            model_arguments(&selection(Provider::Local, None, None, true)).is_empty(),
            "no flags is the CLI's own auto pick"
        );
    }

    #[test]
    fn stage_lines_survive_the_chapter_prefix() {
        let (done, total, stage) = parse_stage_line("  [ch1] [2/4] translation · 1.02s · ETA 3m")
            .expect("prefixed stage line");
        assert_eq!((done, total, stage), (2, 4, Stage::Translation));

        let (done, total, stage) =
            parse_stage_line("  [1/4] detection · 0.50s").expect("bare stage line");
        assert_eq!((done, total, stage), (1, 4, Stage::Detection));

        assert!(parse_stage_line("[page 01.png] failed to read: EOF").is_none());
        assert!(parse_stage_line("4 pages (3 to translate) -> out").is_none());
    }

    #[test]
    fn done_lines_only_match_the_final_report() {
        assert!(parse_done("  [page 01.png] done in 12.31s"));
        assert!(parse_done("[ch1] [page 02.png] done in 3.00s · ETA 1m"));
        assert!(!parse_done("  [1/4] translation · 1.02s"));
        assert!(!parse_done("  [page 01.png] translation failed: boom"));
    }

    #[test]
    fn pending_totals_accumulate_per_chapter() {
        assert_eq!(
            parse_pending("4 pages (3 to translate) -> target/out"),
            Some(3)
        );
        assert_eq!(
            parse_pending("[ch1] 4 pages (4 to translate) -> target/out"),
            Some(4)
        );
        assert_eq!(parse_pending("nothing to do; use --overwrite"), None);
    }

    #[test]
    fn progress_folds_the_run_lines_in_order() {
        let mut progress = BatchProgress::default();
        assert!(progress.observe("4 pages (3 to translate) -> out"));
        assert_eq!((progress.completed, progress.total), (0, 3));
        assert!(progress.observe("  [1/3] detection · 1.00s"));
        assert_eq!(progress.stage, Some(Stage::Detection));
        assert!(progress.observe("  [page 01.png] done in 9.00s"));
        assert_eq!((progress.completed, progress.total), (1, 3));
        assert!(!progress.observe("warning: calibration not saved"));
    }

    #[test]
    fn the_model_announcement_becomes_the_job_label() {
        assert_eq!(
            parse_model(
                "translation model: qwen3.5-0.8b Q4_K_XL (2.3 GiB, within 5.0 GiB, 0.6 GiB if not already stored)"
            ),
            Some("qwen3.5-0.8b Q4_K_XL")
        );
        assert_eq!(
            parse_model(
                "translation model: gemma4-e2b-it Q4_K_XL (4.7 GiB (measured), budget unknown, 1 GiB if not already stored)"
            ),
            Some("gemma4-e2b-it Q4_K_XL")
        );
        assert_eq!(
            parse_model(
                "translation model: gpt-5.6-luna (openai) — hosted; no local budget, download or calibration"
            ),
            Some("gpt-5.6-luna (openai)")
        );
        assert_eq!(
            parse_model(
                "translation model: DeepL (deepl) — hosted; no local budget, download or calibration"
            ),
            Some("DeepL (deepl)")
        );
        assert_eq!(parse_model("  [1/3] detection · 1.00s"), None);
        assert_eq!(parse_model("4 pages (3 to translate) -> out"), None);
    }

    #[test]
    fn the_announced_model_reaches_the_job_once() {
        let mut progress = BatchProgress::default();
        let line = "translation model: gemma4-e2b-it Q4_K_XL (4.0 GiB, within 5.6 GiB, 3.3 GiB if not already stored)";
        assert!(progress.observe(line), "the first announcement republishes");
        assert_eq!(progress.model.as_deref(), Some("gemma4-e2b-it Q4_K_XL"));
        assert!(!progress.observe(line), "the same label does not republish");
    }
}
