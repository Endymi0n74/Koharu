//! Run orchestration of `koharu-batch`: input classification (one chapter or
//! a volume of them), phase-major execution, per-page finalization, reports
//! and the machine-readable `--json` summary.
//!
//! The logic lives in the library rather than in the binary so `cargo test`
//! runs its tests: unit tests inside a binary target are ignored by
//! `cargo test --tests`, which is what CI executes.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use koharu_config::Config;
use koharu_rasterizer::{RasterOptions, Rasterizer};
use koharu_renderer::Renderer;
use koharu_scene::{AssetInput, AssetMetadata, AssetRole, At, EntityId, PageDraft, Session};
use koharu_translator::preset::{MeasuredPeak, MeasuredPeaks};
use koharu_translator::{ProvidersConfig, preset};
use serde_json::{Value, json};

use super::cli::{self, Arguments, FormatChoice, gib};
use super::report::{PageOutcome, PageReport, RunReport, Thumbnails};
use super::{bootstrap, calibration, cbz, pages, report};
use crate::vram::VramSampler;
use crate::{Committer, Operation, Pipeline, Progress, Request, Scope, Stage, StageOutput};

struct SessionCommitter<'a>(&'a mut Session);

#[async_trait::async_trait]
impl Committer for SessionCommitter<'_> {
    async fn commit(&mut self, output: StageOutput) -> Result<koharu_scene::Snapshot> {
        Ok(self.0.commit(output.patch).await?.snapshot)
    }
}

#[derive(Debug)]
enum InputPages {
    Directory(Vec<pages::PageSource>),
    Archive(PathBuf, Vec<pages::PageSource>),
}

impl InputPages {
    fn list(&self) -> &[pages::PageSource] {
        match self {
            Self::Directory(entries) | Self::Archive(_, entries) => entries,
        }
    }
}

/// Whether `path` names a `.cbz`/`.zip` archive (compared without case).
fn is_archive_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cbz") || extension.eq_ignore_ascii_case("zip")
        })
}

/// One input of the run: a whole chapter (`label` empty) or one chapter of a
/// volume (`label` is its folder or archive name).
#[derive(Debug)]
struct ChapterSource {
    label: String,
    source: PathBuf,
    input: InputPages,
}

/// Classifies `input` and lists one entry per chapter to translate.
///
/// The rule: page images at the top level of a folder mean "this is one
/// chapter" (`--recursive` flattens the whole tree into one chapter instead);
/// a folder whose images only live in subfolders or `.cbz`/`.zip` archives is
/// a volume, one chapter per subfolder and per archive, in natural order.
///
/// Returns the sources plus the classification line to print — the line is
/// part of `--dry-run`: it explains the rule that decided the plan.
fn plan_sources(input: &Path, recursive: bool) -> Result<(Vec<ChapterSource>, String)> {
    if input.is_file() {
        if is_archive_path(input) {
            return Ok((
                vec![ChapterSource {
                    label: String::new(),
                    source: input.to_owned(),
                    input: InputPages::Archive(input.to_owned(), cbz::list_archive_pages(input)?),
                }],
                format!(
                    "input: single chapter — {} is a CBZ/ZIP archive",
                    input.display()
                ),
            ));
        }
        bail!("input must be a folder of images or a .cbz archive");
    }
    if !input.is_dir() {
        bail!("input {} does not exist", input.display());
    }
    if recursive {
        return Ok((
            vec![ChapterSource {
                label: String::new(),
                source: input.to_owned(),
                input: InputPages::Directory(pages::list_directory_pages(input, true)?),
            }],
            format!(
                "input: single chapter — --recursive flattens every page of {} into one chapter",
                input.display()
            ),
        ));
    }
    if pages::has_pages(input, false) {
        return Ok((
            vec![ChapterSource {
                label: String::new(),
                source: input.to_owned(),
                input: InputPages::Directory(pages::list_directory_pages(input, false)?),
            }],
            format!(
                "input: single chapter — {} holds page images at its top level",
                input.display()
            ),
        ));
    }
    // No page at the top level: the folder is a volume, one chapter per
    // subfolder that holds pages (at any depth) and per CBZ/ZIP archive.
    let mut chapters = Vec::new();
    for entry in
        fs::read_dir(input).with_context(|| format!("failed to read {}", input.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read {}", input.display()))?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        // Dot-prefixed entries are hidden files and folders in disguise.
        if name.starts_with('.') {
            continue;
        }
        // `file_type` does not follow symbolic links, so a link to a folder
        // can never send this scan in circles.
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            if let Ok(listing) = pages::list_directory_pages(&path, true) {
                chapters.push(ChapterSource {
                    label: name,
                    source: path.clone(),
                    input: InputPages::Directory(listing),
                });
            }
        } else if path.is_file()
            && is_archive_path(&path)
            && let Ok(listing) = cbz::list_archive_pages(&path)
        {
            let label = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or(name);
            chapters.push(ChapterSource {
                label,
                source: path.clone(),
                input: InputPages::Archive(path.clone(), listing),
            });
        }
    }
    chapters.sort_by(|left, right| pages::natural_cmp(&left.label, &right.label));
    dedup_labels(&mut chapters);
    if chapters.is_empty() {
        bail!(
            "no manga pages found in {} (expected page images at its top level, chapter subfolders, or .cbz archives)",
            input.display()
        );
    }
    let rule = format!(
        "input: volume — {} holds no page images at its top level; {} chapter(s) detected",
        input.display(),
        chapters.len()
    );
    Ok((chapters, rule))
}

/// Gives every chapter a unique label: a folder and an archive of the same
/// name must not mirror their outputs onto the same path.
fn dedup_labels(chapters: &mut [ChapterSource]) {
    let mut seen = HashSet::new();
    for chapter in chapters.iter_mut() {
        let base = chapter.label.clone();
        let mut suffix = 2;
        while !seen.insert(chapter.label.clone()) {
            chapter.label = format!("{base}-{suffix}");
            suffix += 1;
        }
    }
}

/// Output path of a chapter: the run's output for a plain single-chapter run,
/// mirrored under it for a volume — `<OUTPUT>/<label>/` for a folder output,
/// `<OUTPUT-STEM>/<label>.cbz` for an archive output.
fn chapter_output_path(output: &Path, output_is_archive: bool, label: &str) -> PathBuf {
    if label.is_empty() {
        return output.to_owned();
    }
    if output_is_archive {
        let extension = output
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("cbz");
        return output
            .with_extension("")
            .join(format!("{label}.{extension}"));
    }
    output.join(label)
}

/// Report base for an output path: a `.cbz`/`.zip` extension is dropped and
/// `.report` appended, so the report files land on `<path>.md` and
/// `<path>.html` while dots that are part of the name stay part of it — a
/// folder `vol.1` must not report into `vol.md`, and the chapters `ch.1` and
/// `ch.2` of a volume must not collide.
fn report_base_for(output: &Path) -> PathBuf {
    let mut base = if is_archive_path(output) {
        output.with_extension("").into_os_string()
    } else {
        output.as_os_str().to_owned()
    };
    base.push(".report");
    PathBuf::from(base)
}

/// Report base from an explicit `--report <BASE>`: one pair of files at
/// `BASE` for a single chapter, `BASE-<label>` for every chapter of a volume.
/// The `.report` suffix keeps dots in `BASE` or in the label from being
/// mistaken for the extension the writers replace.
fn report_base_specified(base: &str, label: &str) -> PathBuf {
    let mut path = if label.is_empty() {
        base.to_owned()
    } else {
        format!("{base}-{label}")
    };
    path.push_str(".report");
    PathBuf::from(path)
}

/// Reads one page's bytes: from disk for a folder chapter, from the
/// chapter's one open archive for a CBZ chapter.
fn read_page_bytes(
    reader: Option<&mut cbz::ArchiveReader>,
    source: &pages::PageSource,
) -> Result<Vec<u8>> {
    match &source.location {
        pages::Location::File(path) => {
            fs::read(path).with_context(|| format!("failed to read {}", path.display()))
        }
        pages::Location::Entry(index) => {
            let reader = reader.context("the chapter's archive is not open")?;
            reader.read_entry(*index)
        }
    }
}

fn output_page_path(output: &Path, index: usize, format: &str) -> PathBuf {
    output.join(format!("page-{index:04}.{format}"))
}

/// Handles `--reset-calibration`: deletes the calibration file so later runs
/// fall back to the built-in reference estimates.
fn reset_calibration(path: Option<&Path>) -> Result<()> {
    let Some(path) = path else {
        bail!("no calibration path could be resolved (no home directory?)");
    };
    match fs::remove_file(path) {
        Ok(()) => eprintln!("calibration file removed: {}", path.display()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("no calibration file at {}", path.display());
        }
        Err(error) => {
            return Err(error).with_context(|| format!("failed to remove {}", path.display()));
        }
    }
    Ok(())
}

/// Maximum width or height of a report thumbnail.
const THUMBNAIL_EDGE: u32 = 220;

/// Encodes one downscaled JPEG image as a `data:` URI for the HTML report.
fn thumbnail_data_uri(image: &image::DynamicImage) -> Result<String> {
    let thumbnail = image.thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE).to_rgb8();
    let mut jpeg = Vec::new();
    thumbnail.write_to(
        &mut std::io::Cursor::new(&mut jpeg),
        image::ImageFormat::Jpeg,
    )?;
    Ok(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(jpeg)
    ))
}

/// Run-level facts shared by every page row of the report.
struct RunMetadata {
    model: String,
    quantization: String,
    vram_estimate: String,
    language: String,
    input: String,
    output: String,
    device: String,
    started_at: String,
}

/// Run VRAM peak as a report line, measured at report-writing time: the run's
/// own footprint above the GPU baseline, plus the whole-GPU peak. When the run
/// added nothing (resume with no page to translate), only the whole-GPU peak
/// is shown.
fn vram_peak_line(sampler: Option<&VramSampler>) -> Option<String> {
    let sampler = sampler?;
    let whole_gpu = sampler.whole_gpu_peak_bytes()?;
    let whole_gib = whole_gpu as f64 / (1024.0 * 1024.0 * 1024.0);
    match sampler.peak_bytes() {
        Some(0) | None => Some(format!("pic GPU entier {whole_gib:.1} GiB")),
        Some(run_peak) => {
            let run_gib = run_peak as f64 / (1024.0 * 1024.0 * 1024.0);
            Some(format!(
                "{run_gib:.1} GiB (pic GPU entier {whole_gib:.1} GiB)"
            ))
        }
    }
}

/// Writes the Markdown and HTML reports for the run.
fn write_run_report(
    base: &Path,
    pages: Vec<PageReport>,
    total_elapsed: Duration,
    metadata: &RunMetadata,
    vram_sampler: Option<&VramSampler>,
) -> Result<()> {
    let run_report = RunReport {
        pages,
        total_elapsed,
        model: metadata.model.clone(),
        quantization: metadata.quantization.clone(),
        vram_estimate: Some(metadata.vram_estimate.clone()),
        vram_peak: vram_peak_line(vram_sampler),
        language: metadata.language.clone(),
        input: metadata.input.clone(),
        output: metadata.output.clone(),
        device: metadata.device.clone(),
        started_at: metadata.started_at.clone(),
    };
    let markdown_path = base.with_extension("md");
    let html_path = base.with_extension("html");
    fs::write(&markdown_path, report::to_markdown(&run_report))
        .with_context(|| format!("failed to write {}", markdown_path.display()))?;
    fs::write(&html_path, report::to_html(&run_report))
        .with_context(|| format!("failed to write {}", html_path.display()))?;
    Ok(())
}

/// Announces the two report files of a finished run.
fn announce_report(base: &Path) {
    eprintln!(
        "report written: {} and {}",
        base.with_extension("md").display(),
        base.with_extension("html").display()
    );
}

/// Rewrites the report in place without failing the run: a crash or an
/// interruption then costs the current page at most, never the data already
/// produced. Nothing is written when `--report none` disabled it.
fn save_report(
    base: Option<&Path>,
    pages_report: &[Option<PageReport>],
    elapsed: Duration,
    metadata: &RunMetadata,
    vram_sampler: Option<&VramSampler>,
) {
    let Some(base) = base else {
        return;
    };
    let pages = pages_report.iter().flatten().cloned().collect();
    if let Err(error) = write_run_report(base, pages, elapsed, metadata, vram_sampler) {
        eprintln!("warning: interim report not updated: {error:#}");
    }
}

/// Writes and announces the final report of one chapter. `None` when the
/// chapter's report was disabled with `--report none`.
fn finish_chapter_report(
    chapter: &Chapter,
    elapsed: Duration,
    vram_sampler: Option<&VramSampler>,
) -> Result<()> {
    let Some(base) = &chapter.report_base else {
        return Ok(());
    };
    let pages = chapter
        .pages_report
        .iter()
        .flatten()
        .cloned()
        .collect::<Vec<_>>();
    write_run_report(
        base,
        pages,
        elapsed,
        chapter
            .metadata
            .as_ref()
            .expect("metadata is resolved before any report is written"),
        vram_sampler,
    )?;
    announce_report(base);
    Ok(())
}

fn list_models(measurements: &MeasuredPeaks) {
    println!(
        "{:<26} {:<7} {:<14} {:<22} {:<14} measured here",
        "model", "vision", "quantization", "estimate", "download"
    );
    for model in preset::vram_catalog_with(true, measurements) {
        let descriptor = preset::descriptor_for(&model.id);
        for quantization in &model.quantizations {
            let measured = measurements
                .peak_bytes(&model.id, &quantization.id, model.vision)
                .map(gib)
                .unwrap_or_else(|| "—".to_owned());
            let download = descriptor.map_or_else(
                || "—".to_owned(),
                |descriptor| {
                    gib(preset::download_bytes(
                        descriptor,
                        Some(&quantization.id),
                        model.vision,
                    ))
                },
            );
            println!(
                "{:<26} {:<7} {:<14} {:<22} {:<14} {}",
                model.id,
                if model.vision { "yes" } else { "no" },
                quantization.id,
                quantization.estimate.display(),
                download,
                measured
            );
        }
    }
}

fn encode_image(
    image: &image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
    format: FormatChoice,
) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    let target = match format {
        FormatChoice::Png => image::ImageFormat::Png,
        FormatChoice::Jpg => image::ImageFormat::Jpeg,
        FormatChoice::Webp => image::ImageFormat::WebP,
    };
    image.write_to(&mut std::io::Cursor::new(&mut encoded), target)?;
    Ok(encoded)
}

/// Renders and rasterizes one finished page. Failures are the page's own:
/// they must never abort the run, which would cost the report.
async fn render_page(
    renderer: &Renderer,
    rasterizer: &Rasterizer,
    session: &Session,
    page: EntityId,
) -> Result<image::ImageBuffer<image::Rgba<u8>, Vec<u8>>> {
    let frame = renderer.render(&session.snapshot(), page).await?;
    Ok(rasterizer
        .rasterize(&frame.raster_frame()?, RasterOptions::default())?
        .image)
}

/// One page admitted to the phase-major run: its session entity plus the
/// report data accumulated across the pipeline stages.
struct LoadedPage {
    /// Index of the page in the input listing (its report slot).
    index: usize,
    page_id: EntityId,
    name: String,
    before_uri: Option<String>,
    /// Active wall time spent on this page (load plus every phase run).
    elapsed: Duration,
    stage_timings: Arc<Mutex<Vec<(String, Duration)>>>,
    /// Largest per-phase GPU peak above baseline attributed to this page.
    vram_peak: Option<u64>,
}

/// Compact duration for progress lines: `42s`, `3m 05s`, `1h 12m`.
fn human_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3_599 => format!("{}m {:02}s", seconds / 60, seconds % 60),
        _ => format!("{}h {:02}m", seconds / 3_600, (seconds % 3_600) / 60),
    }
}

/// Pace-based time left for the rest of a phase, empty once it is over.
fn phase_eta(elapsed: Duration, completed: usize, total: usize) -> String {
    if completed == 0 || completed >= total {
        return String::new();
    }
    let remaining = elapsed.mul_f64((total - completed) as f64 / completed as f64);
    format!(" · ETA {}", human_duration(remaining))
}

/// One chapter of the run: its input listing, the output it mirrors into,
/// and the state the pipeline builds up while its pages are processed.
struct Chapter {
    /// Folder or archive name of the chapter; empty for a plain
    /// single-chapter run, which keeps today's messages and paths unchanged.
    label: String,
    /// `"[label] "` progress prefix for a volume chapter, empty otherwise.
    prefix: String,
    /// Input path of this chapter (the `--input` path for a single chapter).
    source: PathBuf,
    input: InputPages,
    /// Folder of pages or `.cbz` path this chapter writes to.
    output: PathBuf,
    output_is_archive: bool,
    report_base: Option<PathBuf>,
    /// Resolved once the model is picked; carries this chapter's paths.
    metadata: Option<RunMetadata>,
    /// Pages still to translate, in reading order.
    pending: Vec<pages::PageSource>,
    /// Report rows by page index; `None` for pages outside `--pages`.
    pages_report: Vec<Option<PageReport>>,
    failures: Vec<(String, String)>,
    /// `pending.len()` when the chapter was planned (final summary).
    total_pending: usize,
    /// Pages admitted to the session, filled by the load step.
    loaded: Vec<LoadedPage>,
    session: Option<Session>,
    archive: Option<cbz::ArchiveOutput>,
    /// Entries of an existing output archive carried over by a resumed run.
    carried: Vec<cbz::Carried>,
}

/// Classifies the input, mirrors every chapter's output path, applies
/// `--pages` and resume, and prints the plan lines
/// (`N pages (M to translate) -> …`).
fn prepare_chapters(
    arguments: &Arguments,
    input_path: &Path,
    output: &Path,
) -> Result<Vec<Chapter>> {
    let (sources, rule) = plan_sources(input_path, arguments.recursive)?;
    // The classification line is what `--dry-run` shows to explain the plan.
    eprintln!("{rule}");
    let output_is_archive = is_archive_path(output);
    let mut chapters = Vec::with_capacity(sources.len());
    for source in sources {
        let prefix = if source.label.is_empty() {
            String::new()
        } else {
            format!("[{}] ", source.label)
        };
        let chapter_output = chapter_output_path(output, output_is_archive, &source.label);
        if output_is_archive {
            if let Some(parent) = chapter_output
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
        } else {
            fs::create_dir_all(&chapter_output)
                .with_context(|| format!("failed to create {}", chapter_output.display()))?;
        }
        let report_base = match arguments.report.as_deref() {
            Some("none") => None,
            Some(base) => Some(report_base_specified(base, &source.label)),
            None => Some(report_base_for(&chapter_output)),
        };
        let all_pages = source.input.list();

        // Pages already present in an existing output archive: a resumed CBZ
        // run carries their entries over instead of translating them again.
        let carried: Vec<cbz::Carried> = if output_is_archive && !arguments.overwrite {
            let stems = all_pages
                .iter()
                .map(|page| cbz::entry_stem(&page.name))
                .collect::<Vec<_>>();
            match cbz::carried_entries(&chapter_output, &stems) {
                Ok(carried) => {
                    if !carried.is_empty() {
                        eprintln!(
                            "{prefix}resuming {} page(s) already translated in {}",
                            carried.len(),
                            chapter_output.display()
                        );
                    }
                    carried
                }
                Err(error) => {
                    eprintln!(
                        "{prefix}warning: {} cannot be resumed ({error:#}); every page will be translated",
                        chapter_output.display()
                    );
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        let resumed: HashSet<usize> = carried.iter().map(|entry| entry.page).collect();
        let resume_skip = |page: &pages::PageSource| {
            !arguments.overwrite
                && if output_is_archive {
                    resumed.contains(&page.index)
                } else {
                    output_page_path(&chapter_output, page.index, arguments.format.extension())
                        .exists()
                }
        };
        // `--pages` narrows the run to a range: pages outside it are left out
        // of the report entirely rather than reported as skipped. In a volume
        // the range applies to every chapter, so it must fit the smallest one.
        let selection: Option<HashSet<usize>> = arguments
            .pages
            .as_deref()
            .map(|specification| cli::parse_page_range(specification, all_pages.len()))
            .transpose()?
            .map(|indices| indices.into_iter().collect());
        let in_scope = |page: &pages::PageSource| {
            selection
                .as_ref()
                .is_none_or(|indices| indices.contains(&page.index))
        };
        let pages_report: Vec<Option<PageReport>> = all_pages
            .iter()
            .map(|page| {
                (in_scope(page) && resume_skip(page)).then(|| PageReport {
                    index: page.index,
                    name: page.name.clone(),
                    outcome: PageOutcome::Skipped,
                })
            })
            .collect();
        let pending: Vec<pages::PageSource> = all_pages
            .iter()
            .filter(|page| in_scope(page) && !resume_skip(page))
            .cloned()
            .collect();
        eprintln!(
            "{prefix}{} pages ({} to translate) -> {}",
            all_pages.len(),
            pending.len(),
            chapter_output.display()
        );
        if selection.is_some() {
            let scoped = all_pages.iter().filter(|page| in_scope(page)).count();
            eprintln!(
                "{prefix}--pages keeps {scoped} of {} page(s)",
                all_pages.len()
            );
        }
        chapters.push(Chapter {
            label: source.label,
            prefix,
            source: source.source,
            input: source.input,
            output: chapter_output,
            output_is_archive,
            report_base,
            metadata: None,
            total_pending: pending.len(),
            pending,
            pages_report,
            failures: Vec::new(),
            loaded: Vec::new(),
            session: None,
            archive: None,
            carried,
        });
    }
    Ok(chapters)
}

/// Reads every pending page of the chapter into its own session. A read or
/// decode failure reports the page and drops it from the run; it never
/// aborts the chapter (which would cost the report).
async fn load_chapter(chapter: &mut Chapter) -> Result<()> {
    if chapter.output_is_archive {
        chapter.archive = Some(cbz::ArchiveOutput::create(
            &chapter.output,
            std::mem::take(&mut chapter.carried),
        )?);
    }
    // One open handle for the whole chapter: `ZipArchive::new` parses the
    // central directory, which a per-page open would repeat for every page.
    let mut reader = match &chapter.input {
        InputPages::Directory(_) => None,
        InputPages::Archive(path, _) => Some(cbz::ArchiveReader::open(path)?),
    };
    chapter.session = Some(Session::memory().await?);
    let prefix = chapter.prefix.clone();
    for page in &chapter.pending {
        let load_started = Instant::now();
        let page_name = page.name.clone();
        let bytes = match read_page_bytes(reader.as_mut(), page) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("{prefix}[{page_name}] failed to read: {error:#}");
                chapter.pages_report[page.index] = Some(PageReport::failed(
                    page.index,
                    page_name.clone(),
                    format!("read failed: {error:#}"),
                ));
                chapter.failures.push((page_name, error.to_string()));
                continue;
            }
        };
        let (decoded, before_uri) = match image::load_from_memory(&bytes) {
            Ok(decoded) => {
                let uri = thumbnail_data_uri(&decoded)
                    .map_err(|error| eprintln!("{prefix}[{page_name}] thumbnail skipped: {error}"))
                    .ok();
                (decoded, uri)
            }
            Err(error) => {
                eprintln!("{prefix}[{page_name}] failed to decode: {error}");
                chapter.pages_report[page.index] = Some(PageReport::failed(
                    page.index,
                    page_name.clone(),
                    format!("decode failed: {error}"),
                ));
                chapter
                    .failures
                    .push((page_name, "decode failed".to_owned()));
                continue;
            }
        };

        let session = chapter
            .session
            .as_mut()
            .expect("the chapter session is created before its pages are loaded");
        let mut session_page = None;
        let patch = session.snapshot().patch(|edit| {
            let id = edit.add_page(
                PageDraft::new(
                    &page_name,
                    f64::from(decoded.width()),
                    f64::from(decoded.height()),
                ),
                At::End,
            )?;
            edit.set_asset(
                id,
                &AssetRole::new("source")?,
                AssetInput::new(
                    Arc::<[u8]>::from(bytes.as_slice()),
                    page.media_type,
                    AssetMetadata {
                        width: Some(decoded.width()),
                        height: Some(decoded.height()),
                        attributes: BTreeMap::new(),
                    },
                ),
            )?;
            session_page = Some(id);
            Ok(())
        });
        let patch = match patch {
            Ok(patch) => patch,
            Err(error) => {
                eprintln!("{prefix}[{page_name}] failed to edit the page: {error:#}");
                chapter.pages_report[page.index] = Some(PageReport::failed(
                    page.index,
                    page_name.clone(),
                    format!("session edit failed: {error:#}"),
                ));
                chapter.failures.push((page_name, error.to_string()));
                continue;
            }
        };
        if let Err(error) = session.commit(patch).await {
            eprintln!("{prefix}[{page_name}] failed to commit the page: {error:#}");
            chapter.pages_report[page.index] = Some(PageReport::failed(
                page.index,
                page_name.clone(),
                format!("session commit failed: {error:#}"),
            ));
            chapter.failures.push((page_name, error.to_string()));
            continue;
        }
        chapter.loaded.push(LoadedPage {
            index: page.index,
            page_id: session_page.expect("page ID is assigned by the edit"),
            name: page_name,
            before_uri,
            elapsed: load_started.elapsed(),
            stage_timings: Arc::new(Mutex::new(Vec::new())),
            vram_peak: None,
        });
    }
    Ok(())
}

/// The state every vision phase of one chapter shares.
///
/// Keeping it together keeps a phase call readable and within clippy's
/// argument limit; a phase also checkpoints the chapter's report when it ends.
struct Phases<'a> {
    session: &'a mut Session,
    pipeline: &'a Pipeline,
    vram_sampler: Option<&'a VramSampler>,
    pages_report: &'a mut [Option<PageReport>],
    failures: &'a mut Vec<(String, String)>,
    retries: usize,
    quiet: bool,
    report_base: Option<&'a Path>,
    metadata: &'a RunMetadata,
    started: Instant,
    prefix: String,
}

impl Phases<'_> {
    /// Runs one pipeline stage across every surviving page, in reading order.
    ///
    /// The stage's model loads once and stays resident for the whole phase; a
    /// phase failure reports the page and drops it from the later phases, so
    /// the per-page fault isolation of a per-page run is kept. `retries` extra
    /// attempts are granted to a page before it is dropped, so a transient
    /// failure does not cost a page the run already paid for.
    async fn run(&mut self, stage: Stage, pages: &mut Vec<LoadedPage>) {
        if !self.quiet {
            eprintln!("{}{stage}: {} page(s)", self.prefix, pages.len());
        }
        let total = pages.len();
        let mut completed = 0_usize;
        let phase_started = Instant::now();
        let mut index = 0;
        while index < pages.len() {
            let mut attempt = 0_usize;
            loop {
                let (page_id, timings) = {
                    let page = &pages[index];
                    (page.page_id, Arc::clone(&page.stage_timings))
                };
                if let Some(sampler) = self.vram_sampler {
                    sampler.reset_window();
                }
                let attempt_started = Instant::now();
                let outcome = {
                    let snapshot = self.session.snapshot();
                    let mut committer = SessionCommitter(&mut *self.session);
                    self.pipeline
                        .execute(
                            snapshot,
                            Request {
                                operation: Operation::Only { stage },
                                scope: Scope::Pages(vec![page_id]),
                                progress: Some(Arc::new(move |event| {
                                    if let Progress::Finished { stage, elapsed, .. } = event {
                                        timings
                                            .lock()
                                            .expect("stage timings mutex")
                                            .push((stage.to_string(), elapsed));
                                    }
                                })),
                                ..Request::default()
                            },
                            &mut committer,
                        )
                        .await
                };
                let attempt_elapsed = attempt_started.elapsed();
                {
                    let page = &mut pages[index];
                    page.elapsed += attempt_elapsed;
                    if let Some(sampler) = self.vram_sampler
                        && let Some(delta) = sampler.window_delta_bytes()
                    {
                        page.vram_peak = Some(page.vram_peak.map_or(delta, |peak| peak.max(delta)));
                    }
                }
                match outcome {
                    Ok(_) => {
                        index += 1;
                        completed += 1;
                        if !self.quiet {
                            eprintln!(
                                "  {}[{completed}/{total}] {stage} · {:.2}s{}",
                                self.prefix,
                                attempt_elapsed.as_secs_f64(),
                                phase_eta(phase_started.elapsed(), completed, total)
                            );
                        }
                        break;
                    }
                    Err(error) if attempt < self.retries => {
                        attempt += 1;
                        let name = &pages[index].name;
                        eprintln!(
                            "{}[{name}] {stage} failed (attempt {} of {}): {error:#}",
                            self.prefix,
                            attempt + 1,
                            self.retries + 1
                        );
                    }
                    Err(error) => {
                        let page = pages.remove(index);
                        eprintln!("{}[{}] {stage} failed: {error:#}", self.prefix, page.name);
                        self.pages_report[page.index] = Some(PageReport::failed(
                            page.index,
                            page.name.clone(),
                            format!("{stage} failed: {error:#}"),
                        ));
                        self.failures.push((page.name.clone(), error.to_string()));
                        break;
                    }
                }
            }
        }
        save_report(
            self.report_base,
            self.pages_report,
            self.started.elapsed(),
            self.metadata,
            self.vram_sampler,
        );
    }
}

/// One page's finalization handed to the worker thread: everything after
/// rendering (encode, write, thumbnail) runs there so it overlaps the next
/// page's translation instead of idling the GPU behind it.
struct FinalizeJob {
    index: usize,
    name: String,
    /// Progress prefix of the chapter, for the worker's own warnings.
    prefix: String,
    raster: image::ImageBuffer<image::Rgba<u8>, Vec<u8>>,
    /// Page time accumulated so far (load, phases, translation, render).
    elapsed: Duration,
    stages: Vec<(String, Duration)>,
    before_uri: Option<String>,
    vram_peak: Option<u64>,
}

/// The worker's verdict on one page; the report row is built from it.
struct Finalized {
    index: usize,
    name: String,
    /// Total wall time for the page, including the worker's own stretch.
    elapsed: Duration,
    stages: Vec<(String, Duration)>,
    before_uri: Option<String>,
    vram_peak: Option<u64>,
    /// `Ok(thumbnail)` — `None` when the thumbnail could not be encoded (a
    /// warning, not a failure); `Err((step, detail))` when the page could not
    /// be encoded or written.
    result: Result<Option<String>, (&'static str, String)>,
}

enum FinalizerMessage {
    Page(FinalizeJob),
    /// Publish the chapter's archive; sent once, after the last page.
    Finish,
    /// Drop the chapter's archive unpublished; sent when the run aborts.
    Abort,
}

/// The worker thread a chapter's finalization runs on: a page's encode,
/// write, and thumbnail happen while the next page translates.
struct Finalizer {
    sender: Option<mpsc::Sender<FinalizerMessage>>,
    results: mpsc::Receiver<Finalized>,
    handle: Option<std::thread::JoinHandle<Result<()>>>,
}

impl Finalizer {
    fn start(output: PathBuf, format: FormatChoice, archive: Option<cbz::ArchiveOutput>) -> Self {
        let (job_sender, job_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            finalizer_thread(output, format, archive, job_receiver, result_sender)
        });
        Self {
            sender: Some(job_sender),
            results: result_receiver,
            handle: Some(handle),
        }
    }

    /// Hands one rendered page to the worker.
    fn push(&mut self, job: FinalizeJob) -> Result<()> {
        self.sender
            .as_ref()
            .context("the output writer already stopped")?
            .send(FinalizerMessage::Page(job))
            .context("the output writer stopped unexpectedly")
    }

    /// Takes the pages the worker has already finished.
    fn poll(&self) -> Vec<Finalized> {
        let mut finished = Vec::new();
        while let Ok(finalized) = self.results.try_recv() {
            finished.push(finalized);
        }
        finished
    }

    /// Asks the worker to publish the archive, collects every remaining page,
    /// and joins the thread. The second half of the returned tuple is the
    /// archive publication itself; it is reported only after the pages are
    /// recorded, so a publication failure cannot cost their report rows.
    fn finish(&mut self) -> (Vec<Finalized>, Result<()>) {
        if let Some(sender) = self.sender.take() {
            // A FIFO channel: the worker sees Finish after every page.
            let _ = sender.send(FinalizerMessage::Finish);
            drop(sender);
        }
        let mut finished = Vec::new();
        while let Ok(finalized) = self.results.recv() {
            finished.push(finalized);
        }
        let published = self
            .join()
            .context("failed to publish the chapter's output");
        (finished, published)
    }

    fn join(&mut self) -> Result<()> {
        let Some(handle) = self.handle.take() else {
            return Ok(());
        };
        handle
            .join()
            .map_err(|_| anyhow::anyhow!("the output writer thread panicked"))?
    }
}

impl Drop for Finalizer {
    fn drop(&mut self) {
        // An aborted run must leave no half-published archive: signal the
        // worker to drop it (which removes the partial file) and wait for it.
        if let Some(sender) = self.sender.take() {
            let _ = sender.send(FinalizerMessage::Abort);
            drop(sender);
        }
        let _ = self.join();
    }
}

/// The worker loop: one page at a time, in the order the main thread sent
/// them, then — on `Finish` — the archive publication.
fn finalizer_thread(
    output: PathBuf,
    format: FormatChoice,
    mut archive: Option<cbz::ArchiveOutput>,
    jobs: mpsc::Receiver<FinalizerMessage>,
    results: mpsc::Sender<Finalized>,
) -> Result<()> {
    while let Ok(message) = jobs.recv() {
        match message {
            FinalizerMessage::Page(job) => {
                let finalized = finalize_page(&output, format, archive.as_mut(), job);
                if results.send(finalized).is_err() {
                    return Ok(());
                }
            }
            FinalizerMessage::Finish => {
                if let Some(archive) = archive.take() {
                    archive.finish()?;
                }
                return Ok(());
            }
            FinalizerMessage::Abort => return Ok(()),
        }
    }
    // The main side vanished without a decision: the archive simply drops,
    // which removes its partial file, exactly like an interrupted run before.
    Ok(())
}

/// Encodes, writes, and thumbnails one rendered page on the worker thread.
fn finalize_page(
    output: &Path,
    format: FormatChoice,
    archive: Option<&mut cbz::ArchiveOutput>,
    job: FinalizeJob,
) -> Finalized {
    let started = Instant::now();
    let result = match encode_image(&job.raster, format) {
        Ok(encoded) => {
            let written = match archive {
                Some(archive) => {
                    archive.add_page(job.index, &job.name, format.extension(), &encoded)
                }
                None => {
                    let target = output_page_path(output, job.index, format.extension());
                    fs::write(&target, &encoded)
                        .with_context(|| format!("failed to write {}", target.display()))
                }
            };
            match written {
                Ok(()) => Ok(
                    thumbnail_data_uri(&image::DynamicImage::ImageRgba8(job.raster))
                        .map_err(|error| {
                            eprintln!(
                                "{}[{}] thumbnail (after) skipped: {error}",
                                job.prefix, job.name
                            )
                        })
                        .ok(),
                ),
                Err(error) => Err(("write", format!("{error:#}"))),
            }
        }
        Err(error) => Err(("encode", format!("{error:#}"))),
    };
    Finalized {
        index: job.index,
        name: job.name,
        elapsed: job.elapsed + started.elapsed(),
        stages: job.stages,
        before_uri: job.before_uri,
        vram_peak: job.vram_peak,
        result,
    }
}

/// Translation plus per-page finalization for one chapter.
struct Translation<'a> {
    pipeline: &'a Pipeline,
    renderer: &'a Renderer,
    rasterizer: &'a Rasterizer,
    vram_sampler: Option<&'a VramSampler>,
    format: FormatChoice,
    retries: usize,
    quiet: bool,
    started: Instant,
    /// Pages fully finalized so far in this chapter (progress lines).
    processed: usize,
    /// Pages the chapter's translation started with.
    total: usize,
    translation_started: Instant,
}

impl Translation<'_> {
    /// Translates the chapter's surviving pages in reading order; each page
    /// is rendered and written right after its own translation so outputs
    /// still appear progressively. Every failure stays the page's own: a
    /// render, encode, write or session failure drops the page but never the
    /// run, so the report is always written with what actually happened.
    async fn run(&mut self, chapter: &mut Chapter) -> Result<()> {
        let Some(session) = chapter.session.as_mut() else {
            return Ok(());
        };
        let prefix = chapter.prefix.clone();
        let quiet = self.quiet;
        let report_base = chapter.report_base.as_deref();
        let metadata = chapter
            .metadata
            .as_ref()
            .expect("metadata is resolved before the run starts");
        let mut remaining = std::mem::take(&mut chapter.loaded);
        if remaining.is_empty() {
            return Ok(());
        }
        self.processed = 0;
        self.total = remaining.len();
        self.translation_started = Instant::now();
        // Everything after rendering moves to the worker: the next page's
        // translation overlaps this page's encode, write, and thumbnail.
        let mut finalizer =
            Finalizer::start(chapter.output.clone(), self.format, chapter.archive.take());
        while !remaining.is_empty() {
            let mut attempt = 0_usize;
            let translated = loop {
                let (page_id, timings) = {
                    let page = &remaining[0];
                    (page.page_id, Arc::clone(&page.stage_timings))
                };
                if let Some(sampler) = self.vram_sampler {
                    sampler.reset_window();
                }
                let attempt_started = Instant::now();
                let outcome = {
                    let snapshot = session.snapshot();
                    let mut committer = SessionCommitter(&mut *session);
                    self.pipeline
                        .execute(
                            snapshot,
                            Request {
                                operation: Operation::Only {
                                    stage: Stage::Translation,
                                },
                                scope: Scope::Pages(vec![page_id]),
                                progress: Some(Arc::new(move |event| {
                                    if let Progress::Finished { stage, elapsed, .. } = event {
                                        if !quiet {
                                            eprintln!(
                                                "  {stage} finished in {:.2}s",
                                                elapsed.as_secs_f64()
                                            );
                                        }
                                        timings
                                            .lock()
                                            .expect("stage timings mutex")
                                            .push((stage.to_string(), elapsed));
                                    }
                                })),
                                ..Request::default()
                            },
                            &mut committer,
                        )
                        .await
                };
                {
                    let page = &mut remaining[0];
                    page.elapsed += attempt_started.elapsed();
                    if let Some(sampler) = self.vram_sampler
                        && let Some(delta) = sampler.window_delta_bytes()
                    {
                        page.vram_peak = Some(page.vram_peak.map_or(delta, |peak| peak.max(delta)));
                    }
                }
                match outcome {
                    Ok(_) => {
                        self.processed += 1;
                        break Some(remaining.remove(0));
                    }
                    Err(error) if attempt < self.retries => {
                        attempt += 1;
                        eprintln!(
                            "{prefix}[{}] translation failed (attempt {} of {}): {error:#}",
                            remaining[0].name,
                            attempt + 1,
                            self.retries + 1
                        );
                    }
                    Err(error) => {
                        let page = remaining.remove(0);
                        eprintln!("{prefix}[{}] translation failed: {error:#}", page.name);
                        chapter.pages_report[page.index] = Some(PageReport::failed(
                            page.index,
                            page.name.clone(),
                            format!("translation failed: {error:#}"),
                        ));
                        chapter
                            .failures
                            .push((page.name.clone(), error.to_string()));
                        self.processed += 1;
                        self.checkpoint(report_base, &chapter.pages_report, metadata);
                        break None;
                    }
                }
            };
            let Some(page) = translated else {
                continue;
            };
            let page_name = page.name.clone();

            let finalize_started = Instant::now();
            let raster =
                match render_page(self.renderer, self.rasterizer, session, page.page_id).await {
                    Ok(raster) => raster,
                    Err(error) => {
                        eprintln!("{prefix}[{page_name}] failed to render: {error:#}");
                        chapter.pages_report[page.index] = Some(PageReport::failed(
                            page.index,
                            page_name.clone(),
                            format!("render failed: {error:#}"),
                        ));
                        chapter.failures.push((page_name, error.to_string()));
                        self.checkpoint(report_base, &chapter.pages_report, metadata);
                        continue;
                    }
                };
            // Rendering stays on this thread (it borrows the session); every
            // step after it moves to the worker and overlaps the translation
            // of the next page.
            let job = FinalizeJob {
                index: page.index,
                name: page_name,
                prefix: prefix.clone(),
                raster,
                elapsed: page.elapsed + finalize_started.elapsed(),
                stages: page
                    .stage_timings
                    .lock()
                    .expect("stage timings mutex")
                    .clone(),
                before_uri: page.before_uri,
                vram_peak: page.vram_peak,
            };
            for finalized in finalizer.poll() {
                self.record(
                    &prefix,
                    &mut chapter.pages_report,
                    &mut chapter.failures,
                    finalized,
                );
                self.checkpoint(report_base, &chapter.pages_report, metadata);
            }
            finalizer.push(job)?;
        }
        // Drain the worker: every remaining page gets its report row, then
        // the chapter's archive is published.
        let (finished, published) = finalizer.finish();
        for finalized in finished {
            self.record(
                &prefix,
                &mut chapter.pages_report,
                &mut chapter.failures,
                finalized,
            );
            self.checkpoint(report_base, &chapter.pages_report, metadata);
        }
        published?;
        Ok(())
    }

    /// Records one page the worker finished: its report row, its progress
    /// line, and — for a failure — the run's failure list.
    fn record(
        &mut self,
        prefix: &str,
        pages_report: &mut [Option<PageReport>],
        failures: &mut Vec<(String, String)>,
        finalized: Finalized,
    ) {
        let Finalized {
            index,
            name,
            elapsed,
            stages,
            before_uri,
            vram_peak,
            result,
        } = finalized;
        let outcome = match result {
            Ok(after_uri) => {
                let thumbnails = match (before_uri, after_uri) {
                    (Some(before), Some(after)) => Some(Thumbnails { before, after }),
                    _ => None,
                };
                if !self.quiet {
                    eprintln!(
                        "{prefix}[{name}] done in {:.2}s{}",
                        elapsed.as_secs_f64(),
                        phase_eta(
                            self.translation_started.elapsed(),
                            self.processed,
                            self.total
                        )
                    );
                }
                PageOutcome::Translated {
                    elapsed,
                    stages,
                    thumbnails,
                    vram_peak: vram_peak.map(gib),
                }
            }
            Err((step, detail)) => {
                match step {
                    "encode" => eprintln!("{prefix}[{name}] failed to encode: {detail}"),
                    _ => eprintln!("{prefix}[{name}] failed to write the output: {detail}"),
                }
                failures.push((name.clone(), detail.clone()));
                PageOutcome::Failed(format!("{step} failed: {detail}"))
            }
        };
        pages_report[index] = Some(PageReport {
            index,
            name,
            outcome,
        });
    }

    /// Rewrites the chapter's report in place without failing the run. The
    /// report's own fields are passed explicitly: the session borrow taken
    /// above rules out borrowing the whole chapter again here.
    fn checkpoint(
        &self,
        report_base: Option<&Path>,
        pages_report: &[Option<PageReport>],
        metadata: &RunMetadata,
    ) {
        save_report(
            report_base,
            pages_report,
            self.started.elapsed(),
            metadata,
            self.vram_sampler,
        );
    }
}

/// `(translated, skipped, failed, planned)` counts of a chapter.
fn tally(chapter: &Chapter, dry_run: bool) -> (usize, usize, usize, usize) {
    let mut translated = 0;
    let mut skipped = 0;
    let mut failed = 0;
    for row in chapter.pages_report.iter().flatten() {
        match row.outcome {
            PageOutcome::Translated { .. } => translated += 1,
            PageOutcome::Skipped => skipped += 1,
            PageOutcome::Failed(_) => failed += 1,
        }
    }
    let planned = if dry_run { chapter.pending.len() } else { 0 };
    (translated, skipped, failed, planned)
}

fn counts_value(counts: (usize, usize, usize, usize)) -> Value {
    json!({
        "translated": counts.0,
        "skipped": counts.1,
        "failed": counts.2,
        "planned": counts.3,
    })
}

/// One page row of the `--json` summary.
fn page_value(row: &PageReport) -> Value {
    let (status, elapsed, stages, vram_peak, error) = match &row.outcome {
        PageOutcome::Translated {
            elapsed,
            stages,
            vram_peak,
            ..
        } => (
            "translated",
            Some(elapsed.as_secs_f64()),
            stages
                .iter()
                .map(|(stage, duration)| {
                    json!({ "stage": stage, "seconds": duration.as_secs_f64() })
                })
                .collect::<Vec<_>>(),
            vram_peak.clone(),
            None,
        ),
        PageOutcome::Skipped => ("skipped", None, Vec::new(), None, None),
        PageOutcome::Failed(message) => ("failed", None, Vec::new(), None, Some(message.as_str())),
    };
    json!({
        "index": row.index + 1,
        "name": row.name,
        "status": status,
        "elapsed_seconds": elapsed,
        "stages": stages,
        "vram_peak": vram_peak,
        "error": error,
    })
}

/// One not-yet-translated page of a `--dry-run` summary.
fn planned_page(page: &pages::PageSource) -> Value {
    json!({
        "index": page.index + 1,
        "name": page.name,
        "status": "planned",
        "elapsed_seconds": null,
        "stages": [],
        "vram_peak": null,
        "error": null,
    })
}

/// Facts the `--json` summary reports about the run itself.
struct JsonSummary<'a> {
    path: Option<&'a Path>,
    metadata: &'a RunMetadata,
    volume: bool,
    dry_run: bool,
    started: Instant,
}

/// Writes the machine-readable summary; without `--json` this is a no-op.
fn write_json_summary(summary: &JsonSummary<'_>, chapters: &[Chapter]) -> Result<()> {
    let Some(path) = summary.path else {
        return Ok(());
    };
    let mut chapter_values = Vec::new();
    let mut totals = (0_usize, 0_usize, 0_usize, 0_usize);
    for chapter in chapters {
        let metadata = chapter.metadata.as_ref().unwrap_or(summary.metadata);
        let mut rows: Vec<Value> = chapter
            .pages_report
            .iter()
            .flatten()
            .map(page_value)
            .collect();
        if summary.dry_run {
            rows.extend(chapter.pending.iter().map(planned_page));
            rows.sort_by_key(|row| row["index"].as_u64());
        }
        let counts = tally(chapter, summary.dry_run);
        totals = (
            totals.0 + counts.0,
            totals.1 + counts.1,
            totals.2 + counts.2,
            totals.3 + counts.3,
        );
        chapter_values.push(json!({
            "label": chapter.label,
            "input": metadata.input,
            "output": metadata.output,
            "counts": counts_value(counts),
            "pages": rows,
        }));
    }
    let failures = chapters
        .iter()
        .flat_map(|chapter| {
            chapter
                .failures
                .iter()
                .map(move |(page, error)| {
                    json!({ "chapter": chapter.label, "page": page, "error": error })
                })
        })
        .collect::<Vec<_>>();
    let document = json!({
        "ok": failures.is_empty(),
        "volume": summary.volume,
        "dry_run": summary.dry_run,
        "started_at": summary.metadata.started_at,
        "elapsed_seconds": summary.started.elapsed().as_secs_f64(),
        "input": summary.metadata.input,
        "output": summary.metadata.output,
        "language": summary.metadata.language,
        "model": {
            "id": summary.metadata.model,
            "quantization": summary.metadata.quantization,
            "vram_estimate": summary.metadata.vram_estimate,
            "device": summary.metadata.device,
        },
        "counts": counts_value(totals),
        "chapters": chapter_values,
        "failures": failures,
    });
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(&document)?;
    text.push('\n');
    fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))?;
    eprintln!("json summary written: {}", path.display());
    Ok(())
}

/// Runs the whole batch command and returns the process exit code: `0` only
/// when every page of every chapter succeeded, `1` otherwise. Terminating the
/// process is the binary's job — see `exit_with` there.
pub async fn run(arguments: Arguments) -> Result<i32> {
    let calibration_path = calibration::default_path();
    if arguments.reset_calibration {
        reset_calibration(calibration_path.as_deref())?;
        return Ok(0);
    }
    let measurements = if arguments.no_calibration {
        MeasuredPeaks::new()
    } else {
        calibration_path
            .as_deref()
            .map(calibration::load)
            .unwrap_or_default()
    };
    if arguments.list_models {
        list_models(&measurements);
        return Ok(0);
    }
    let run_started_at = report::timestamp_now();
    let Some(input_path) = arguments.input.as_deref() else {
        bail!("--input is required (folder of images or a .cbz archive)");
    };
    let Some(output) = arguments.output.clone() else {
        bail!("--output is required (folder for page images, or a .cbz path)");
    };
    let store = arguments
        .store
        .clone()
        .unwrap_or_else(cli::default_store_root);
    koharu_runtime::Store::configure(&store).with_context(|| {
        format!(
            "failed to configure the runtime store at {}",
            store.display()
        )
    })?;

    // Classification first: the plan lines (and the rule behind them) are
    // printed before the model is resolved, exactly like the page listing of
    // a single-chapter run always was.
    let mut chapters = prepare_chapters(&arguments, input_path, &output)?;
    let volume = chapters.iter().any(|chapter| !chapter.label.is_empty());

    let device = koharu_ml::device(arguments.cpu);
    let device_description = device.description.clone();
    let vram_sampler = VramSampler::start_default(&device);
    let budget = cli::detect_budget(&arguments, &device);
    let resolved = cli::resolve_model(&arguments, budget, &measurements)?;
    if arguments.no_calibration {
        eprintln!("(--no-calibration: reference estimates, nothing recorded)");
    } else if measurements.is_empty() {
        eprintln!("(no calibration file yet; estimates come from the reference table)");
    } else {
        eprintln!(
            "(calibrated on {} configuration(s) from previous runs)",
            measurements.len()
        );
    }
    eprintln!(
        "translation model: {} {} ({}{}, {} if not already stored)",
        resolved.model,
        resolved.quantization,
        resolved.estimate.display(),
        budget.map_or_else(
            || ", budget unknown".to_owned(),
            |budget| format!(" within {}", gib(budget))
        ),
        gib(resolved.download)
    );
    if arguments.llm == "auto" && resolved.download >= cli::LARGE_DOWNLOAD_BYTES {
        eprintln!(
            "warning: --llm auto picked {} {} because this GPU fits it; the first real run downloads about {} into {} (one time). Pass --llm <id> to translate with a smaller model, or --list-models to compare sizes.",
            resolved.model,
            resolved.quantization,
            gib(resolved.download),
            store.display()
        );
    }

    let config = cli::pipeline_config(&arguments, &resolved);
    // The run-level metadata feeds the `--json` summary; every chapter gets
    // its own copy with its own input/output paths for its report.
    let run_metadata = RunMetadata {
        model: resolved.model.clone(),
        quantization: resolved.quantization.clone(),
        vram_estimate: resolved.estimate.display(),
        language: arguments.lang.tag().to_owned(),
        input: input_path.display().to_string(),
        output: output.display().to_string(),
        device: device_description.clone(),
        started_at: run_started_at.clone(),
    };
    for chapter in &mut chapters {
        chapter.metadata = Some(RunMetadata {
            model: resolved.model.clone(),
            quantization: resolved.quantization.clone(),
            vram_estimate: resolved.estimate.display(),
            language: arguments.lang.tag().to_owned(),
            input: chapter.source.display().to_string(),
            output: chapter.output.display().to_string(),
            device: device_description.clone(),
            started_at: run_started_at.clone(),
        });
    }
    let started = Instant::now();
    let json_summary = JsonSummary {
        path: arguments.json.as_deref(),
        metadata: &run_metadata,
        volume,
        dry_run: arguments.dry_run,
        started,
    };
    if arguments.dry_run {
        eprintln!("dry run: no runtime download, no page processed");
        for chapter in &chapters {
            for page in &chapter.pending {
                eprintln!("  {}would translate: {}", chapter.prefix, page.name);
            }
        }
        write_json_summary(&json_summary, &chapters)?;
        return Ok(0);
    }
    if chapters.iter().all(|chapter| chapter.pending.is_empty()) {
        eprintln!("nothing to do; use --overwrite to re-translate existing pages");
        for chapter in &chapters {
            finish_chapter_report(chapter, started.elapsed(), vram_sampler.as_ref())?;
        }
        write_json_summary(&json_summary, &chapters)?;
        return Ok(0);
    }
    // A volume can hold chapters that are fully translated while others are
    // not: their reports are written now, they take no part in the run.
    for chapter in chapters.iter().filter(|chapter| chapter.pending.is_empty()) {
        eprintln!(
            "{}nothing to do; use --overwrite to re-translate existing pages",
            chapter.prefix
        );
        finish_chapter_report(chapter, started.elapsed(), vram_sampler.as_ref())?;
    }

    bootstrap::initialize_with_retry().await;
    let pipeline = Pipeline::from_config(
        Config::memory(config),
        Config::memory(ProvidersConfig::default()),
        device,
    )?;
    let renderer = Renderer::new()?;
    let rasterizer = Rasterizer::new()?;

    for chapter in chapters
        .iter_mut()
        .filter(|chapter| !chapter.pending.is_empty())
    {
        load_chapter(chapter).await?;
    }

    // Phase-major execution across the whole run — the whole volume in
    // volume mode: every page joins its chapter's session first, then each
    // stage runs across every surviving page of every chapter before the
    // next stage starts. Models therefore load once per stage instead of
    // once per page (and once per chapter boundary) — on a small card the
    // vision and language models cannot coexist and used to be evicted and
    // reloaded around every page. Fault isolation is preserved: a phase
    // failure drops that page from the later phases only.
    for stage in [Stage::Detection, Stage::Ocr, Stage::Inpainting] {
        for chapter in chapters.iter_mut() {
            if chapter.loaded.is_empty() {
                continue;
            }
            let Some(session) = chapter.session.as_mut() else {
                continue;
            };
            let mut phases = Phases {
                session,
                pipeline: &pipeline,
                vram_sampler: vram_sampler.as_ref(),
                pages_report: &mut chapter.pages_report,
                failures: &mut chapter.failures,
                retries: arguments.retries,
                quiet: arguments.quiet,
                report_base: chapter.report_base.as_deref(),
                metadata: chapter
                    .metadata
                    .as_ref()
                    .expect("metadata is resolved before the run starts"),
                started,
                prefix: chapter.prefix.clone(),
            };
            phases.run(stage, &mut chapter.loaded).await;
        }
    }

    // Translation runs last (the chapter context needs every earlier page's
    // text, and the language model loads once the vision models have been
    // evicted), one chapter after the other so each page is rendered and
    // written right after its own translation.
    let mut translation = Translation {
        pipeline: &pipeline,
        renderer: &renderer,
        rasterizer: &rasterizer,
        vram_sampler: vram_sampler.as_ref(),
        format: arguments.format,
        retries: arguments.retries,
        quiet: arguments.quiet,
        started,
        processed: 0,
        total: 0,
        translation_started: started,
    };
    for chapter in chapters.iter_mut() {
        if !chapter.pending.is_empty() {
            translation.run(chapter).await?;
            // Publish the chapter's outputs, then its final report: a crash
            // from here on still leaves the interim report on disk.
            if let Some(archive) = chapter.archive.take() {
                archive.finish()?;
            }
            finish_chapter_report(chapter, started.elapsed(), vram_sampler.as_ref())?;
        }
    }

    if let Some(sampler) = &vram_sampler {
        sampler.stop();
    }

    // Persist the measured footprint of this run so future runs and budget
    // checks stay calibrated on this machine.
    if !arguments.no_calibration
        && let (Some(path), Some(sampler)) = (&calibration_path, &vram_sampler)
        && let Some(bytes) = sampler.peak_bytes()
    {
        let peak = MeasuredPeak {
            model: resolved.model.clone(),
            quantization: resolved.quantization.clone(),
            vision: resolved.vision,
            bytes,
        };
        match calibration::record(path, peak) {
            Ok(_) => eprintln!("calibration updated: {}", path.display()),
            Err(error) => eprintln!("warning: calibration not saved: {error:#}"),
        }
    }

    write_json_summary(&json_summary, &chapters)?;

    let total_failures: usize = chapters.iter().map(|chapter| chapter.failures.len()).sum();
    if volume {
        eprintln!(
            "volume translated in {:.2}s ({} chapter(s), {} page(s))",
            started.elapsed().as_secs_f64(),
            chapters.len(),
            chapters
                .iter()
                .map(|chapter| chapter.total_pending)
                .sum::<usize>()
        );
        for chapter in &chapters {
            let (translated, skipped, failed, _) = tally(chapter, false);
            eprintln!(
                "  {}{translated} translated, {skipped} skipped, {failed} failed",
                chapter.prefix
            );
        }
        if total_failures > 0 {
            eprintln!("{total_failures} page(s) failed:");
            for chapter in &chapters {
                for (name, error) in &chapter.failures {
                    eprintln!("  - {}{name}: {error}", chapter.prefix);
                }
            }
        }
    } else {
        let chapter = chapters
            .first()
            .expect("the input always yields at least one chapter");
        if total_failures == 0 {
            eprintln!(
                "chapter translated in {:.2}s ({} pages)",
                started.elapsed().as_secs_f64(),
                chapter.total_pending
            );
        } else {
            eprintln!(
                "chapter finished with {} failure(s) in {:.2}s:",
                total_failures,
                started.elapsed().as_secs_f64()
            );
            for (name, error) in &chapter.failures {
                eprintln!("  - {name}: {error}");
            }
        }
    }
    if total_failures > 0 {
        return Err(anyhow::anyhow!("{total_failures} page(s) failed"));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    #[test]
    fn durations_stay_short_enough_for_a_progress_line() {
        assert_eq!(human_duration(Duration::from_secs(42)), "42s");
        assert_eq!(human_duration(Duration::from_secs(185)), "3m 05s");
        assert_eq!(human_duration(Duration::from_secs(4_500)), "1h 15m");
    }

    #[test]
    fn the_eta_follows_the_phase_pace_and_stops_at_the_end() {
        assert_eq!(phase_eta(Duration::ZERO, 0, 10), "");
        assert_eq!(
            phase_eta(Duration::from_secs(30), 2, 10),
            " · ETA 2m 00s",
            "eight pages left at fifteen seconds each"
        );
        assert_eq!(phase_eta(Duration::from_secs(30), 10, 10), "");
    }

    fn write_cbz(path: &Path, entries: &[&str]) {
        let file = fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        for name in entries {
            writer
                .start_file(
                    *name,
                    zip::write::FileOptions::<()>::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(b"x").unwrap();
        }
        writer.finish().unwrap();
    }

    fn labels(chapters: &[ChapterSource]) -> Vec<&str> {
        chapters
            .iter()
            .map(|chapter| chapter.label.as_str())
            .collect()
    }

    #[test]
    fn top_level_images_keep_the_input_a_single_chapter() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("p1.png"), b"x").unwrap();
        fs::create_dir(directory.path().join("ch2")).unwrap();
        fs::write(directory.path().join("ch2").join("p1.png"), b"x").unwrap();

        let (chapters, rule) = plan_sources(directory.path(), false).unwrap();
        assert_eq!(chapters.len(), 1);
        assert!(chapters[0].label.is_empty());
        assert!(rule.contains("single chapter"), "{rule}");
        assert!(rule.contains("top level"), "{rule}");
    }

    #[test]
    fn a_folder_without_top_level_images_is_a_volume() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["ch2", "ch1"] {
            fs::create_dir(directory.path().join(name)).unwrap();
            fs::write(directory.path().join(name).join("p1.png"), b"x").unwrap();
        }

        let (chapters, rule) = plan_sources(directory.path(), false).unwrap();
        assert_eq!(labels(&chapters), ["ch1", "ch2"], "natural order");
        assert!(rule.contains("volume"), "{rule}");
        assert!(rule.contains("2 chapter(s)"), "{rule}");
        for chapter in &chapters {
            assert!(matches!(chapter.input, InputPages::Directory(_)));
        }
    }

    #[test]
    fn archives_beside_subfolders_become_chapters_of_the_volume() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("ch1")).unwrap();
        fs::write(directory.path().join("ch1").join("p1.png"), b"x").unwrap();
        write_cbz(&directory.path().join("ch0.cbz"), &["p1.png", "p2.png"]);

        let (chapters, rule) = plan_sources(directory.path(), false).unwrap();
        assert_eq!(labels(&chapters), ["ch0", "ch1"]);
        assert!(rule.contains("volume"), "{rule}");
        match &chapters[0].input {
            InputPages::Archive(path, listing) => {
                assert_eq!(path, &directory.path().join("ch0.cbz"));
                assert_eq!(listing.len(), 2);
            }
            InputPages::Directory(_) => panic!("the archive chapter lists archive pages"),
        }
    }

    #[test]
    fn recursive_flattens_the_whole_tree_into_one_chapter() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("p2.png"), b"x").unwrap();
        fs::create_dir(directory.path().join("sub")).unwrap();
        fs::write(directory.path().join("sub").join("p1.png"), b"x").unwrap();

        let (chapters, rule) = plan_sources(directory.path(), true).unwrap();
        assert_eq!(chapters.len(), 1);
        assert!(chapters[0].label.is_empty());
        assert!(rule.contains("--recursive"), "{rule}");
        assert_eq!(chapters[0].input.list().len(), 2);
    }

    #[test]
    fn a_folder_with_no_pages_anywhere_is_rejected_and_explains_the_rule() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("notes.txt"), b"x").unwrap();
        let error = plan_sources(directory.path(), false).unwrap_err();
        assert!(
            error.to_string().contains("no manga pages found"),
            "{error:#}"
        );
        assert!(
            format!("{error:#}").contains("chapter subfolders"),
            "the message names what would have been picked up"
        );
    }

    #[test]
    fn labels_of_a_folder_and_an_archive_of_the_same_name_never_collide() {
        let mut chapters = vec![
            ChapterSource {
                label: "ch1".to_owned(),
                source: PathBuf::from("ch1"),
                input: InputPages::Directory(Vec::new()),
            },
            ChapterSource {
                label: "ch1".to_owned(),
                source: PathBuf::from("ch1.cbz"),
                input: InputPages::Directory(Vec::new()),
            },
        ];
        dedup_labels(&mut chapters);
        assert_eq!(labels(&chapters), ["ch1", "ch1-2"]);
    }

    #[test]
    fn volume_outputs_mirror_under_the_output_path() {
        assert_eq!(
            chapter_output_path(Path::new("out"), false, ""),
            Path::new("out"),
            "a single chapter writes exactly where the user asked"
        );
        assert_eq!(
            chapter_output_path(Path::new("out"), false, "ch1"),
            Path::new("out").join("ch1")
        );
        assert_eq!(
            chapter_output_path(Path::new("out.cbz"), true, "ch1"),
            Path::new("out").join("ch1.cbz")
        );
        assert_eq!(
            chapter_output_path(Path::new("out.zip"), true, "ch1"),
            Path::new("out").join("ch1.zip")
        );
    }

    #[test]
    fn report_bases_keep_the_dots_of_the_output_name() {
        assert_eq!(report_base_for(Path::new("out")), Path::new("out.report"));
        assert_eq!(
            report_base_for(Path::new("out.cbz")),
            Path::new("out.report")
        );
        // The writers replace the last extension, so these bases produce
        // `<path>.md` without truncating the name itself.
        assert_eq!(
            report_base_for(Path::new("vol.1")).with_extension("md"),
            Path::new("vol.1.md")
        );
        assert_eq!(
            report_base_for(&Path::new("vol.1").join("ch.2.cbz")).with_extension("html"),
            Path::new("vol.1").join("ch.2.html")
        );
        assert_eq!(report_base_specified("run", ""), Path::new("run.report"));
        assert_eq!(
            report_base_specified("run", "ch.1").with_extension("md"),
            Path::new("run-ch.1.md"),
            "explicit bases stay collision-free across chapters"
        );
    }

    fn sample_metadata() -> RunMetadata {
        RunMetadata {
            model: "gemma4-e4b-it".to_owned(),
            quantization: "Q4_K_P".to_owned(),
            vram_estimate: "5.6 GiB".to_owned(),
            language: "fr-FR".to_owned(),
            input: "volume".to_owned(),
            output: "out".to_owned(),
            device: "NVIDIA GeForce RTX 3070 (CUDA)".to_owned(),
            started_at: "2026-09-25 08:12:00 UTC".to_owned(),
        }
    }

    fn sample_chapter() -> Chapter {
        Chapter {
            label: "ch1".to_owned(),
            prefix: "[ch1] ".to_owned(),
            source: PathBuf::from("volume/ch1"),
            input: InputPages::Directory(Vec::new()),
            output: PathBuf::from("out/ch1"),
            output_is_archive: false,
            report_base: None,
            metadata: Some(RunMetadata {
                output: "out/ch1".to_owned(),
                ..sample_metadata()
            }),
            pending: Vec::new(),
            pages_report: vec![
                Some(PageReport {
                    index: 0,
                    name: "p1.png".to_owned(),
                    outcome: PageOutcome::Translated {
                        elapsed: Duration::from_millis(4250),
                        stages: vec![("detection".to_owned(), Duration::from_millis(700))],
                        thumbnails: None,
                        vram_peak: Some("5.9 GiB".to_owned()),
                    },
                }),
                Some(PageReport::failed(1, "p2.png", "ocr failed: boom")),
            ],
            failures: vec![("p2.png".to_owned(), "ocr failed: boom".to_owned())],
            total_pending: 2,
            loaded: Vec::new(),
            session: None,
            archive: None,
            carried: Vec::new(),
        }
    }

    #[test]
    fn the_json_summary_reports_every_page_status() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("summary.json");
        let metadata = sample_metadata();
        let chapters = [sample_chapter()];
        let summary = JsonSummary {
            path: Some(&path),
            metadata: &metadata,
            volume: true,
            dry_run: false,
            started: Instant::now(),
        };

        write_json_summary(&summary, &chapters).unwrap();

        let written = fs::read_to_string(&path).unwrap();
        let document: Value = serde_json::from_str(&written).unwrap();
        assert_eq!(document["ok"], false, "a failed page fails the run");
        assert_eq!(document["volume"], true);
        assert_eq!(document["dry_run"], false);
        assert_eq!(document["language"], "fr-FR");
        assert_eq!(document["model"]["id"], "gemma4-e4b-it");
        assert_eq!(document["counts"]["translated"], 1);
        assert_eq!(document["counts"]["failed"], 1);
        assert_eq!(document["chapters"].as_array().unwrap().len(), 1);
        assert_eq!(document["chapters"][0]["label"], "ch1");
        assert_eq!(document["chapters"][0]["output"], "out/ch1");
        let rows = document["chapters"][0]["pages"].as_array().unwrap();
        assert_eq!(rows[0]["index"], 1, "pages are numbered in reading order");
        assert_eq!(rows[0]["status"], "translated");
        assert_eq!(rows[0]["stages"][0]["stage"], "detection");
        assert_eq!(rows[1]["status"], "failed");
        assert_eq!(rows[1]["error"], "ocr failed: boom");
        assert_eq!(document["failures"][0]["chapter"], "ch1");
        assert_eq!(document["failures"][0]["page"], "p2.png");
    }

    #[test]
    fn a_dry_run_json_lists_the_planned_pages() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("plan.json");
        let metadata = sample_metadata();
        let mut chapters = [sample_chapter()];
        // A dry run never processes anything: only resumed pages and the
        // pages it plans to translate appear in the summary.
        chapters[0].failures.clear();
        chapters[0].pages_report = vec![Some(PageReport {
            index: 0,
            name: "p1.png".to_owned(),
            outcome: PageOutcome::Skipped,
        })];
        chapters[0].pending = vec![pages::PageSource {
            index: 1,
            name: "p2.png".to_owned(),
            media_type: "image/png",
            location: pages::Location::File(PathBuf::from("p2.png")),
        }];
        let summary = JsonSummary {
            path: Some(&path),
            metadata: &metadata,
            volume: true,
            dry_run: true,
            started: Instant::now(),
        };

        write_json_summary(&summary, &chapters).unwrap();

        let document: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(document["ok"], true, "a dry run never fails pages");
        assert_eq!(document["counts"]["planned"], 1);
        assert_eq!(document["counts"]["skipped"], 1);
        let rows = document["chapters"][0]["pages"].as_array().unwrap();
        assert_eq!(rows.len(), 2, "one resumed row plus the planned page");
        assert_eq!(rows[1]["status"], "planned");
    }

    fn sample_job(index: usize) -> FinalizeJob {
        FinalizeJob {
            index,
            name: format!("p{}.png", index + 1),
            prefix: String::new(),
            raster: image::ImageBuffer::from_pixel(4, 4, image::Rgba([255_u8, 0, 0, 255])),
            elapsed: Duration::from_secs(1),
            stages: vec![("detection".to_owned(), Duration::from_millis(5))],
            before_uri: None,
            vram_peak: None,
        }
    }

    #[test]
    fn the_finalizer_writes_the_page_and_reports_it() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("out");
        fs::create_dir_all(&output).unwrap();
        let mut finalizer = Finalizer::start(output.clone(), FormatChoice::Png, None);

        finalizer.push(sample_job(0)).unwrap();
        let (finished, published) = finalizer.finish();
        published.unwrap();

        assert_eq!(finished.len(), 1, "the page's verdict comes back");
        let finalized = &finished[0];
        assert!(finalized.result.as_ref().unwrap().is_some(), "thumbnail");
        assert_eq!(finalized.stages.len(), 1);
        assert!(output_page_path(&output, 0, "png").exists());
    }

    #[test]
    fn an_aborted_finalizer_leaves_no_partial_archive() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("out.cbz");
        let archive = cbz::ArchiveOutput::create(&target, Vec::new()).unwrap();
        let mut finalizer = Finalizer::start(target.clone(), FormatChoice::Png, Some(archive));

        finalizer.push(sample_job(0)).unwrap();
        // Dropping without Finish must not publish a half-written archive.
        drop(finalizer);

        assert!(!target.exists());
        assert!(
            !directory.path().join("out.cbz.partial").exists(),
            "the partial file is cleaned up"
        );
    }

    #[test]
    fn without_the_json_flag_nothing_is_written() {
        let metadata = sample_metadata();
        let chapters = [sample_chapter()];
        let summary = JsonSummary {
            path: None,
            metadata: &metadata,
            volume: false,
            dry_run: false,
            started: Instant::now(),
        };
        write_json_summary(&summary, &chapters).unwrap();
    }
}
