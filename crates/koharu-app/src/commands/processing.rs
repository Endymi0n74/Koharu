use std::{collections::HashMap, fmt, sync::Arc};

use anyhow::{Context as _, Result};
use koharu_pipeline::{Committer, Progress, RunStatus, StageOutput, StopToken};
use koharu_scene::Snapshot;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager as _, State, ipc::Channel};
use tauri_runtime_cef::CefRuntime;
use uuid::Uuid;

use super::{ChannelExt as _, Error, canvas::CanvasChannel, project::CurrentProject};
use koharu_desktop::Desktop;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, Type)]
#[serde(transparent)]
pub struct JobId(Uuid);

impl JobId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for JobId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Clone, Debug, Serialize, Type)]
pub struct Job {
    pub id: JobId,
    pub state: JobState,
    #[specta(type = f64)]
    pub completed: usize,
    #[specta(type = f64)]
    pub total: usize,
    pub page: Option<koharu_scene::EntityId>,
    pub stage: Option<koharu_pipeline::Stage>,
    pub model: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Finished,
    Failed,
    Stopped,
}

#[derive(Default)]
pub(crate) struct Processing {
    pub(crate) stops: Mutex<HashMap<JobId, StopToken>>,
    pub(crate) jobs: Mutex<HashMap<JobId, Job>>,
    pub(crate) inpainting_mask: Mutex<Option<koharu_pipeline::InpaintingMask>>,
}

#[derive(Default)]
pub(crate) struct JobChannel {
    pub(crate) channel: Mutex<Option<Channel<Job>>>,
}

/// Removes a job from the running tables and reports its final state.
///
/// `outcome` is what the task observed; a missing outcome means the task ended
/// without one — a panic — and the job is retired as failed so the tables never
/// keep a job that will report nothing again.
fn retire_job(
    stops: &mut HashMap<JobId, StopToken>,
    jobs: &mut HashMap<JobId, Job>,
    id: JobId,
    outcome: Option<(JobState, Option<String>)>,
) -> Option<Job> {
    stops.remove(&id);
    let job = jobs.remove(&id)?;
    let (state, error) = outcome.unwrap_or((
        JobState::Failed,
        Some("the pipeline task ended without reporting a result".to_owned()),
    ));
    Some(Job {
        state,
        error,
        ..job
    })
}

/// Retires its job however the pipeline task ends, panic included.
///
/// A leaked entry would keep `process` refusing every later run with "another
/// process is already running" until the app restarts.
struct JobGuard {
    handle: AppHandle<CefRuntime>,
    id: JobId,
    outcome: Option<(JobState, Option<String>)>,
}

impl JobGuard {
    fn new(handle: AppHandle<CefRuntime>, id: JobId) -> Self {
        Self {
            handle,
            id,
            outcome: None,
        }
    }

    fn finish(&mut self, stopped: bool, error: Option<String>) {
        self.outcome = Some((
            if stopped {
                JobState::Stopped
            } else if error.is_some() {
                JobState::Failed
            } else {
                JobState::Finished
            },
            error,
        ));
    }
}

impl Drop for JobGuard {
    fn drop(&mut self) {
        let processing = self.handle.state::<Processing>();
        let mut stops = processing.stops.lock();
        let mut jobs = processing.jobs.lock();
        if let Some(job) = retire_job(&mut stops, &mut jobs, self.id, self.outcome.take()) {
            self.handle.state::<JobChannel>().channel.publish(job);
        }
    }
}

#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn process(
    handle: AppHandle<CefRuntime>,
    scope: koharu_pipeline::Scope,
    operation: koharu_pipeline::Operation,
    project: State<'_, CurrentProject>,
    processing: State<'_, Processing>,
    job_channel: State<'_, JobChannel>,
) -> std::result::Result<JobId, Error> {
    let snapshot = project
        .project
        .lock()
        .await
        .as_ref()
        .context("no project is open")?
        .snapshot();
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
        model: None,
        error: None,
    };
    processing.jobs.lock().insert(id, job.clone());
    job_channel.channel.publish(job);

    let pipeline = handle.state::<koharu_pipeline::Pipeline>().inner().clone();
    let task_handle = handle.clone();
    let inpainting_mask = processing.inpainting_mask.lock().take();
    drop(tokio::spawn(async move {
        let mut guard = JobGuard::new(task_handle.clone(), id);
        let progress = Arc::new(Mutex::new((0_usize, 0_usize)));
        let progress_handle = task_handle.clone();
        let mut request = koharu_pipeline::Request {
            operation,
            scope,
            stop: stop.clone(),
            progress: None,
            inpainting_mask,
        };
        request.progress = Some(Arc::new(move |event| {
            let update = match event {
                Progress::Started { pages, stages } => {
                    tracing::info!(
                        target: "koharu_metrics",
                        metric = "pipeline_start",
                        page_count = pages.len(),
                        stage_count = stages.len(),
                    );
                    let mut progress = progress.lock();
                    *progress = (0, pages.len().saturating_mul(stages.len()));
                    Some((0, progress.1, None, None, None))
                }
                Progress::Loading { page, stage, model } => {
                    tracing::info!(
                        target: "koharu_metrics",
                        metric = "stage_loading",
                        stage = %stage,
                        model,
                    );
                    let progress = progress.lock();
                    Some((progress.0, progress.1, Some(page), Some(stage), Some(model)))
                }
                Progress::Finished {
                    page,
                    stage,
                    model,
                    elapsed,
                } => {
                    if stage != koharu_pipeline::Stage::Translation {
                        tracing::info!(
                            target: "koharu_metrics",
                            metric = "model_run",
                            stage = %stage,
                            model,
                            duration_ms = elapsed.as_secs_f64() * 1000.0,
                        );
                    }
                    let mut progress = progress.lock();
                    progress.0 = progress.0.saturating_add(1).min(progress.1);
                    Some((progress.0, progress.1, Some(page), Some(stage), Some(model)))
                }
                Progress::Skipped { page, stage } => {
                    tracing::info!(
                        target: "koharu_metrics",
                        metric = "stage_skip",
                        stage = %stage,
                    );
                    let mut progress = progress.lock();
                    progress.0 = progress.0.saturating_add(1).min(progress.1);
                    Some((progress.0, progress.1, Some(page), Some(stage), None))
                }
                Progress::Running { stage, model, .. } => {
                    tracing::info!(
                        target: "koharu_metrics",
                        metric = "stage_running",
                        stage = %stage,
                        model,
                    );
                    None
                }
            };
            if let Some((completed, total, page, stage, model)) = update {
                let job = {
                    let processing = progress_handle.state::<Processing>();
                    let mut jobs = processing.jobs.lock();
                    jobs.get_mut(&id).map(|job| {
                        job.completed = completed;
                        job.total = total;
                        job.page = page;
                        job.stage = stage;
                        job.model = model;
                        job.clone()
                    })
                };
                if let Some(job) = job {
                    progress_handle.state::<JobChannel>().channel.publish(job);
                }
            }
        }));

        struct PipelineCommitter {
            handle: AppHandle<CefRuntime>,
        }

        #[async_trait::async_trait]
        impl Committer for PipelineCommitter {
            async fn commit(&mut self, output: StageOutput) -> Result<Snapshot> {
                let (commit, page) = {
                    let projects = self.handle.state::<CurrentProject>();
                    let mut projects = projects.project.lock().await;
                    let project = projects.as_mut().context("no project is open")?;
                    let Some(commit) = project.commit_rebased(output.patch).await? else {
                        return Ok(project.snapshot());
                    };
                    project.record_commit(&commit);
                    let page = project.active_page();
                    (commit, page)
                };
                let snapshot = commit.snapshot.clone();
                let desktop = self.handle.state::<Desktop>();
                desktop.synchronize(&commit.snapshot, page, &commit).await?;
                let canvas = desktop.canvas_state();
                self.handle.state::<CanvasChannel>().channel.publish(canvas);
                Ok(snapshot)
            }
        }

        let mut committer = PipelineCommitter {
            handle: task_handle.clone(),
        };
        let result = pipeline.execute(snapshot, request, &mut committer).await;
        let (stopped, error) = match result {
            Ok(report) => (report.status == RunStatus::Stopped, None),
            Err(error) => {
                tracing::error!(stage = ?error.stage, %error, "processing failed");
                (false, Some(format!("{error:#}")))
            }
        };
        tracing::info!(
            target: "koharu_metrics",
            metric = "pipeline_result",
            outcome = if stopped {
                "stopped"
            } else if error.is_some() {
                "failed"
            } else {
                "completed"
            },
        );
        guard.finish(stopped, error);
        // Dropping the guard retires the job and publishes its final state;
        // it runs on a panic too, so the tables stay usable either way.
    }));
    Ok(id)
}

#[tracing::instrument(
    target = "koharu_metrics",
    name = "pipeline_stop",
    skip_all,
    fields(state = "requested")
)]
#[tauri::command]
#[specta::specta]
pub(crate) async fn stop_job(
    job: JobId,
    processing: State<'_, Processing>,
) -> std::result::Result<(), Error> {
    let stops = processing.stops.lock();
    let stop = stops
        .get(&job)
        .with_context(|| format!("job {job} is not running"))?;
    stop.stop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(id: JobId) -> Job {
        Job {
            id,
            state: JobState::Running,
            completed: 2,
            total: 5,
            page: None,
            stage: None,
            model: None,
            error: None,
        }
    }

    #[test]
    fn a_task_that_ends_without_a_result_is_retired_as_failed() {
        let mut stops = HashMap::new();
        let mut jobs = HashMap::new();
        let id = JobId::new();
        stops.insert(id, StopToken::default());
        jobs.insert(id, running(id));

        let retired =
            retire_job(&mut stops, &mut jobs, id, None).expect("the running job is reported");

        assert!(stops.is_empty(), "a stop token is never left behind");
        assert!(
            jobs.is_empty(),
            "a job that will not report again is removed"
        );
        assert_eq!(retired.state, JobState::Failed);
        assert!(retired.error.is_some());
    }

    #[test]
    fn retiring_keeps_the_outcome_the_task_observed() {
        let mut stops = HashMap::new();
        let mut jobs = HashMap::new();
        let id = JobId::new();
        stops.insert(id, StopToken::default());
        jobs.insert(id, running(id));

        let retired = retire_job(&mut stops, &mut jobs, id, Some((JobState::Finished, None)))
            .expect("the running job is reported");

        assert_eq!(retired.state, JobState::Finished);
        assert!(retired.error.is_none());
        assert_eq!(retired.completed, 2, "the job keeps its progress");
    }

    #[test]
    fn retiring_an_unknown_job_publishes_nothing() {
        let mut stops = HashMap::new();
        let mut jobs = HashMap::new();
        assert!(retire_job(&mut stops, &mut jobs, JobId::new(), None).is_none());
    }
}
