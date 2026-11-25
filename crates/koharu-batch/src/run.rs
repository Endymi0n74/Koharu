//! One chapter: its pages imported into a project, the stages run over them,
//! the project left on disk.

use std::{
    collections::BTreeSet,
    io::Read as _,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use alphanumeric_sort::compare_path;
use anyhow::{Context as _, Result, bail, ensure};
use koharu_llm::profile::Term;
use koharu_pipeline::{Event, Outcome, Pipeline, PipelineConfig, Request, Stage, Target};
use koharu_renderer::Renderer;
use koharu_scene::{Edit, Hash, Op, PageId, Session};
use parking_lot::Mutex;
use serde_json::json;

use crate::{cli::Run, preset};

/// What a page image ends with.
const EXTENSIONS: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

// The project file name stored inside the output folder.
const PROJECT_FILE: &str = "project.khr";

// Render and save a translated page image to the output folder.
fn render_page(session: &Session, output_dir: &Path, page_id: PageId, index: usize) -> Result<()> {
    let rendered = Renderer::get().render(session, page_id)?.rasterize()?;
    let out_path = output_dir.join(format!("page{}.png", index + 1));
    rendered.image.save(&out_path)?;
    Ok(())
}

pub async fn run(arguments: &Run) -> Result<()> {
    let config = config(arguments)?;
    let output_dir = &arguments.output;
    if output_dir.is_file() {
        bail!(
            "{} is a file, not a folder; pick a folder for the translated images",
            output_dir.display()
        );
    }
    if !output_dir.exists() {
        fs_err::create_dir_all(output_dir)?;
    }
    let (session, resuming) = prepare(arguments, output_dir)?;
    let pages: Vec<PageId> = session
        .lock()
        .project()
        .pages()
        .map(|page| page.id)
        .collect();
    let (target, selected) = match &arguments.pages {
        None => (Target::Project, pages.len()),
        Some(range) => {
            let chosen = chosen(range, &pages)?;
            let selected = chosen.len();
            (Target::Pages(chosen), selected)
        }
    };
    let stages = if resuming && arguments.stages.is_empty() {
        BTreeSet::from([Stage::Translate])
    } else {
        stages(&arguments.stages)?
    };
    // The app's dialog counts this line with the per-stage ones below, so it
    // stays one line before them.
    println!("{} page stages to run", selected * stages.len());
    let request = Request {
        target,
        stages,
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
    // The task ends on the run's `Ended` event, which `history` has already
    // seen: aborting it here would drop the stage lines queued behind them.
    if matches!(history.last(), Some(Event::Ended { .. })) {
        let _ = progress.await;
    } else {
        progress.abort();
    }
    // Export translated pages to the output folder.
    let session_lock = session.lock();
    let project = session_lock.project();
    for (index, page) in project.pages().enumerate() {
        let page_id = page.id;
        render_page(&session_lock, output_dir, page_id, index)?;
    }
    report(&history, arguments)
}

/// The project to work on: the existing output when `--overwrite` resumes it,
/// or a new one imported from `--input`.
fn prepare(arguments: &Run, output_dir: &Path) -> Result<(Arc<Mutex<Session>>, bool)> {
    let project_path = output_dir.join(PROJECT_FILE);
    let resuming = arguments.overwrite && project_path.exists();
    let session = if resuming {
        let session = Session::open(&project_path)
            .with_context(|| format!("couldn't open {}", project_path.display()))?;
        Arc::new(Mutex::new(session))
    } else {
        let input = arguments
            .input
            .as_deref()
            .context("--input is required unless --overwrite opens an existing --output")?;
        import(input, &project_path)?
    };
    Ok((session, resuming))
}

/// What the run told, as the terminal's last word on it.
fn report(history: &[Event], arguments: &Run) -> Result<()> {
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
fn config(arguments: &Run) -> Result<PipelineConfig> {
    let translator = translator(arguments)?;
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
            "model": translator,
            "target": arguments.target,
            "instructions": arguments.instructions.clone().unwrap_or_default(),
            "glossary": glossary,
            "temperature": arguments.deterministic.then_some(0.0),
        },
    });
    serde_json::from_value(settings)
        .context("a flag names a model or a language the pipeline doesn't know")
}

/// The translation model to load: the flag's reference, or `auto`'s strongest
/// model that fits the budget; and the budget's refusal of a model that cannot.
fn translator(arguments: &Run) -> Result<String> {
    let budget = arguments
        .vram_budget
        .map(|mebibytes| mebibytes * 1024 * 1024);
    let reference = match arguments.translator.as_str() {
        "auto" => {
            // No GPU query here: 0.92 publishes free memory only when a load
            // fails, so `auto` budgets 8 GiB unless the flags say otherwise.
            let budget = budget.unwrap_or(preset::budget_from_total(8 * 1024 * 1024 * 1024));
            let choice = preset::resolve(budget, false).with_context(|| {
                format!(
                    "no catalog model fits the {} MiB budget",
                    budget / 1024 / 1024
                )
            })?;
            eprintln!(
                "{} estimated at {}",
                choice.reference,
                choice.estimate.display()
            );
            choice.reference
        }
        reference => reference.to_owned(),
    };
    if let Some(budget) = budget
        && let Some((model, quantization, estimate)) = preset::of_reference(&reference, false)
        && estimate.bytes > budget
    {
        bail!(
            "{model} at {quantization} is estimated at {}, over the {} MiB budget: name a smaller --translator or raise --vram-budget",
            estimate.display(),
            budget / 1024 / 1024
        );
    }
    Ok(reference)
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

/// A session holding `input`'s pages, written at `output`, its pages added in
/// reading order.
fn import(input: &Path, output: &Path) -> Result<Arc<Mutex<Session>>> {
    let pages = pages(input)?;
    ensure!(!pages.is_empty(), "{} holds no page image", input.display());
    if output.exists() {
        bail!(
            "{} exists; use --overwrite to re-translate it",
            output.display()
        );
    }
    let mut session =
        Session::create(output).with_context(|| format!("couldn't create {}", output.display()))?;
    let mut ops = Vec::with_capacity(pages.len() * 2);
    for (index, bytes) in pages.into_iter().enumerate() {
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

/// The chapter's pages, in reading order: a folder's images, a `.cbz` or
/// `.zip` of them, or one image.
fn pages(input: &Path) -> Result<Vec<Vec<u8>>> {
    if input.is_dir() {
        let mut files = page_files(input)?;
        files.sort_by(|left, right| compare_path(left, right));
        return files.iter().map(|file| Ok(fs_err::read(file)?)).collect();
    }
    if matches!(extension(input).as_str(), "cbz" | "zip") {
        return archive(input);
    }
    Ok(vec![fs_err::read(input)?])
}

/// The page images a `.cbz` holds, in reading order.
fn archive(input: &Path) -> Result<Vec<Vec<u8>>> {
    let file = fs_err::File::open(input)?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("{} isn't a readable archive", input.display()))?;
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name()?.to_string();
        if !EXTENSIONS.contains(&extension(Path::new(&name)).as_str()) {
            continue;
        }
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        entries.push((name, bytes));
    }
    entries.sort_by(|(left, _), (right, _)| compare_path(Path::new(left), Path::new(right)));
    Ok(entries.into_iter().map(|(_, bytes)| bytes).collect())
}

/// Every page image directly in `folder`, unordered.
fn page_files(folder: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in fs_err::read_dir(folder)? {
        let path = entry?.path();
        if path.is_dir() {
            continue;
        }
        if EXTENSIONS.contains(&extension(&path).as_str()) {
            files.push(path);
        }
    }
    Ok(files)
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;
    use crate::cli::{Arguments, Command};

    fn parse(arguments: &[&str]) -> Run {
        let parsed = Arguments::parse_from(arguments.iter().copied());
        let Command::Run(run) = parsed.command else {
            panic!("the arguments name a run");
        };
        run
    }

    #[test]
    fn deterministic_translates_at_temperature_zero() {
        let arguments = parse(&[
            "koharu-batch",
            "run",
            "--input",
            "pages",
            "--output",
            "chapter_output",
            "--deterministic",
        ]);
        let config = config(&arguments).expect("the default model and language are known");
        assert_eq!(config.translator.temperature, Some(0.0));
    }

    #[test]
    fn render_page_writes_each_page_at_its_own_size() {
        if koharu_renderer::Gpu::get().is_err() {
            eprintln!("skipped without a GPU");
            return;
        }
        let directory = tempfile::tempdir().expect("a temporary folder");
        let output_dir = directory.path().join("pages");
        fs_err::create_dir_all(&output_dir).expect("created output folder");
        let mut session = Session::create(directory.path().join(PROJECT_FILE)).expect("a project");
        let ops = [
            json!({"op": "add_page", "id": "$first", "index": 0, "size": {"width": 40, "height": 30}}),
            json!({"op": "add_page", "id": "$second", "index": 1, "size": {"width": 20, "height": 60}}),
            json!({"op": "add_layer", "page": "$first", "index": 0, "layer": {
                "kind": "shape", "path": "M0 0 H40 V30 H0 Z", "fill": [1.0, 0.0, 0.0, 1.0]}}),
        ]
        .into_iter()
        .map(|op| serde_json::from_value(op).expect("an op"))
        .collect();
        let change = session
            .apply(Edit {
                label: "pages".to_owned(),
                ops,
            })
            .expect("an edit");
        let [first, second] = change.pages.added[..] else {
            panic!("two pages");
        };
        render_page(&session, &output_dir, first, 0).expect("the first page");
        render_page(&session, &output_dir, second, 1).expect("the second page");
        let drawn = image::open(output_dir.join("page1.png"))
            .expect("the first PNG")
            .to_rgba8();
        assert_eq!((drawn.width(), drawn.height()), (40, 30));
        let pixel = drawn.get_pixel(20, 15).0;
        assert!(pixel[3] == 255 && pixel[0] > 200, "{pixel:?}");
        let empty = image::open(output_dir.join("page2.png"))
            .expect("the second PNG")
            .to_rgba8();
        assert_eq!((empty.width(), empty.height()), (20, 60));
    }

    #[test]
    fn overwrite_opens_an_existing_project_without_an_input() {
        let directory = tempfile::tempdir().expect("a temporary folder");
        let output_dir = directory.path().join("chapter_output");
        fs_err::create_dir_all(&output_dir).expect("created output folder");
        let project_path = output_dir.join(PROJECT_FILE);
        Session::create(&project_path).expect("an empty project");
        let arguments = parse(&[
            "koharu-batch",
            "run",
            "--output",
            output_dir.to_str().expect("the path is UTF-8"),
            "--overwrite",
        ]);
        let (session, resuming) = prepare(&arguments, &output_dir).expect("the project opens");
        assert!(resuming);
        assert_eq!(session.lock().project().pages().count(), 0);
    }
}
