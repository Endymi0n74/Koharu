//! One chapter: its pages imported into a project, the stages run over them,
//! the project left on disk.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use alphanumeric_sort::compare_path;
use anyhow::{Context as _, Result, bail, ensure};
use koharu_llm::profile::Term;
use koharu_pipeline::{Event, Outcome, Pipeline, PipelineConfig, Request, Stage, Target};
use koharu_scene::{Edit, Hash, Op, PageId, Session};
use parking_lot::Mutex;
use serde_json::json;

use crate::cli::Arguments;

/// What a folder's page images end with.
const EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

pub async fn run(arguments: &Arguments) -> Result<()> {
    let config = config(arguments)?;
    let session = import(&arguments.input, &arguments.output)?;
    let pages: Vec<PageId> = session
        .lock()
        .project()
        .pages()
        .map(|page| page.id)
        .collect();
    let target = match &arguments.pages {
        None => Target::Project,
        Some(range) => Target::Pages(chosen(range, &pages)?),
    };
    let request = Request {
        target,
        stages: stages(&arguments.stages)?,
        config: None,
    };
    let pipeline = Pipeline::new(config);
    let progress = tokio::spawn(progress(pipeline.events()));
    let run = pipeline
        .run(Arc::clone(&session), request)
        .context("the run wasn't queued")?;
    let history = pipeline
        .history(run, Duration::from_secs(arguments.wait))
        .await?;
    progress.abort();
    report(&history, arguments)
}

/// What the run told, as the terminal's last word on it.
fn report(history: &[Event], arguments: &Arguments) -> Result<()> {
    let finished = history
        .iter()
        .filter(|event| matches!(event, Event::Finished { .. }))
        .count();
    match history.last() {
        Some(Event::Ended {
            outcome: Outcome::Done,
            ..
        }) => {
            println!(
                "{finished} page stages done; project at {}",
                arguments.output.display()
            );
            Ok(())
        }
        Some(Event::Ended {
            outcome: Outcome::Failed { stage, reason, .. },
            ..
        }) => bail!("{stage} failed: {reason}"),
        Some(Event::Ended {
            outcome: Outcome::Stopped,
            ..
        }) => bail!("the run stopped after {finished} page stages"),
        _ => bail!(
            "the run didn't finish within {}s after {finished} page stages",
            arguments.wait
        ),
    }
}

/// One line per page stage the run finishes, so a chapter of thirty pages says
/// where it is.
async fn progress(mut events: tokio::sync::broadcast::Receiver<Event>) {
    while let Ok(event) = events.recv().await {
        match event {
            Event::Finished { page, stage, .. } => println!("{stage} done on {page}"),
            Event::Ended { .. } => break,
            _ => {}
        }
    }
}

/// The app's pipeline settings, as the flags name them. A model or a language
/// the pipeline doesn't know fails here, before any page is read.
fn config(arguments: &Arguments) -> Result<PipelineConfig> {
    let glossary = arguments
        .glossary
        .iter()
        .map(|pair| term(pair))
        .collect::<Result<Vec<_>>>()?;
    let settings = json!({
        "detector": { "model": arguments.detector },
        "recognizer": { "model": arguments.ocr },
        "inpainter": { "model": arguments.inpainting },
        "translator": {
            "model": arguments.translator,
            "target": arguments.target,
            "instructions": arguments.instructions.clone().unwrap_or_default(),
            "glossary": glossary,
        },
    });
    serde_json::from_value(settings)
        .context("a flag names a model or a language the pipeline doesn't know")
}

/// A `SOURCE=TARGET` pair of the command line.
fn term(pair: &str) -> Result<Term> {
    let (source, target) = pair
        .split_once('=')
        .with_context(|| format!("`{pair}` isn't SOURCE=TARGET"))?;
    let (source, target) = (source.trim(), target.trim());
    ensure!(
        !source.is_empty() && !target.is_empty(),
        "`{pair}` names no term"
    );
    Ok(Term {
        source: source.to_owned(),
        target: target.to_owned(),
        note: None,
    })
}

/// The stages to run: all of them, in page order, unless the flags name some.
fn stages(names: &[String]) -> Result<BTreeSet<Stage>> {
    if names.is_empty() {
        return Ok([
            Stage::Detect,
            Stage::Recognize,
            Stage::Inpaint,
            Stage::Translate,
        ]
        .into_iter()
        .collect());
    }
    let stages = names
        .iter()
        .map(|name| serde_json::from_value(json!(name)))
        .collect::<Result<BTreeSet<Stage>, _>>()
        .context("a flag names a stage the pipeline doesn't have")?;
    Ok(stages)
}

/// The pages a `1-10,15` range names, as the project holds them.
fn chosen(range: &str, pages: &[PageId]) -> Result<Vec<PageId>> {
    let mut chosen = Vec::new();
    for part in range
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        match part.split_once('-') {
            Some((first, last)) => {
                let (first, last) = (first.trim().parse()?, last.trim().parse()?);
                ensure!(first >= 1 && first <= last, "`{part}` isn't a range");
                for index in first..=last {
                    chosen.push(page(pages, index)?);
                }
            }
            None => chosen.push(page(pages, part.parse()?)?),
        }
    }
    ensure!(!chosen.is_empty(), "`{range}` names no page");
    Ok(chosen)
}

fn page(pages: &[PageId], index: usize) -> Result<PageId> {
    let after = index.checked_sub(1).context("pages count from 1")?;
    pages
        .get(after)
        .copied()
        .with_context(|| format!("there is no page {index}"))
}

/// A session holding `input`'s pages, written at `output`, whose pages were
/// added in name order.
fn import(input: &Path, output: &Path) -> Result<Arc<Mutex<Session>>> {
    let mut files = if input.is_dir() {
        page_files(input)?
    } else {
        vec![input.to_owned()]
    };
    files.sort_by(|left, right| compare_path(left, right));
    ensure!(!files.is_empty(), "{} holds no page image", input.display());
    if output.exists() {
        bail!("{} exists; remove it first", output.display());
    }
    let mut session =
        Session::create(output).with_context(|| format!("couldn't create {}", output.display()))?;
    let mut ops = Vec::with_capacity(files.len() * 2);
    for (index, file) in files.iter().enumerate() {
        let bytes = fs_err::read(file)?;
        let background = Some(Hash::of(&bytes));
        ops.push(Op::AddFile { bytes });
        ops.push(Op::AddPage {
            id: None,
            index,
            size: None,
            background,
        });
    }
    session
        .apply(Edit {
            label: "Import".to_owned(),
            ops,
        })
        .context("the pages weren't imported")?;
    Ok(Arc::new(Mutex::new(session)))
}

/// Every page image directly in `folder`, unordered.
fn page_files(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs_err::read_dir(folder)? {
        let path = entry?.path();
        if path.is_dir() {
            continue;
        }
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if EXTENSIONS.contains(&extension.as_str()) {
            files.push(path);
        }
    }
    Ok(files)
}
