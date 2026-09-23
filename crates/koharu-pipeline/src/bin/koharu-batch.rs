//! `koharu-batch` translates a whole chapter — a folder of images or a CBZ —
//! in one command, running each pipeline stage across every page so loaded
//! models stay resident for the whole chapter, with resume support and VRAM
//! budgeting.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use base64::Engine as _;
use clap::{Parser, ValueEnum};
use koharu_config::Config;
use koharu_ml::Device;
use koharu_pipeline::batch::calibration;
use koharu_pipeline::batch::report::{PageOutcome, PageReport, RunReport, Thumbnails};
use koharu_pipeline::batch::{bootstrap, cbz, pages, report};
use koharu_pipeline::{
    Committer, DetectionModel, Flux2KleinConfig, InpaintingModel, KoharuLayoutRFDetrSeg2XLConfig,
    OcrModel, Operation, Pipeline, PipelineConfig, Progress, Request, RoremMixedConfig, Scope,
    Stage, StageOutput, TranslationConfig, vram::VramSampler,
};
use koharu_rasterizer::{RasterOptions, Rasterizer};
use koharu_renderer::Renderer;
use koharu_scene::{AssetInput, AssetMetadata, AssetRole, At, EntityId, PageDraft, Session};
use koharu_translator::preset::{MeasuredPeak, MeasuredPeaks};
use koharu_translator::{
    GenerationConfig, Language, ModelSelection, Provider, ProvidersConfig, TypographyProfile,
    preset,
};

struct SessionCommitter<'a>(&'a mut Session);

#[async_trait::async_trait]
impl Committer for SessionCommitter<'_> {
    async fn commit(&mut self, output: StageOutput) -> Result<koharu_scene::Snapshot> {
        Ok(self.0.commit(output.patch).await?.snapshot)
    }
}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Translate a chapter (folder of images or CBZ) with Koharu's local pipeline"
)]
struct Arguments {
    /// Folder of images or a .cbz file to translate.
    #[arg(short, long, value_name = "INPUT")]
    input: Option<PathBuf>,

    /// Output folder (pages as images) or .cbz path (packaged chapter).
    #[arg(short, long, value_name = "OUTPUT")]
    output: Option<PathBuf>,

    /// Target language tag (fr-FR, en-US, …). English (en-US) is a safe
    /// fallback: every catalog model handles it well, and JA/RU → EN is
    /// typically the strongest pair for the small local models.
    #[arg(long, default_value = "fr-FR")]
    lang: Language,

    /// Local LLM id, or `auto` to pick the strongest model that fits the
    /// available VRAM (a big pick is downloaded on the first run).
    #[arg(long, default_value = "auto")]
    llm: String,

    /// Local LLM quantization (defaults to the model's first entry).
    #[arg(long)]
    quantization: Option<String>,

    /// Translate text-only: never feed the page image to the LLM, skipping
    /// the vision projector's download and its VRAM.
    #[arg(long)]
    no_vision: bool,

    /// Skip the VRAM budget check (not recommended).
    #[arg(long)]
    force: bool,

    /// Assume this much VRAM in MiB instead of querying the GPU.
    #[arg(long, value_name = "MIB")]
    vram_budget_mib: Option<u64>,

    /// Runtime store holding models and runtimes (defaults to the app
    /// install's `store` directory when the binary lives next to it).
    #[arg(long, value_name = "DIR")]
    store: Option<PathBuf>,

    /// End-of-run report: writes `<BASE>.md` and `<BASE>.html`; pass `none`
    /// to disable. Defaults to `<OUTPUT-STEM>-report` next to the output.
    #[arg(long, value_name = "BASE|none")]
    report: Option<String>,

    /// Delete the VRAM calibration file and exit; later runs fall back to the
    /// built-in reference estimates until new measurements are recorded.
    #[arg(long)]
    reset_calibration: bool,

    /// Run without reading or writing the calibration file: built-in
    /// estimates only, nothing persisted.
    #[arg(long)]
    no_calibration: bool,

    #[arg(long, value_enum, default_value = "koharu-layout-rfdetr-seg-2xl")]
    detection: DetectionChoice,

    #[arg(long, value_enum, default_value = "paddleocr-vl-1.6")]
    ocr: OcrChoice,

    #[arg(long, value_enum, default_value = "lama")]
    inpainting: InpaintingChoice,

    /// Extra instructions passed to the translator.
    #[arg(long)]
    translation_instructions: Option<String>,

    /// Output image format when writing a folder.
    #[arg(long, value_enum, default_value = "png")]
    format: FormatChoice,

    /// List local models with their VRAM estimates and exit.
    #[arg(long)]
    list_models: bool,

    /// Re-translate pages whose output already exists.
    #[arg(long)]
    overwrite: bool,

    /// Plan the run (pages, model, VRAM) without executing it.
    #[arg(long)]
    dry_run: bool,

    /// Force CPU execution.
    #[arg(long)]
    cpu: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DetectionChoice {
    #[value(name = "koharu-layout-rfdetr-seg-2xl")]
    KoharuLayoutRFDetrSeg2XL,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum OcrChoice {
    #[value(name = "paddleocr-vl-1.6")]
    PaddleOcrVl1_6,
    #[value(name = "manga-ocr")]
    MangaOcr,
    #[value(name = "baberu-ocr")]
    BaberuOcr,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum InpaintingChoice {
    #[value(name = "lama")]
    LaMa,
    #[value(name = "aot-inpainting")]
    AotInpainting,
    #[value(name = "flux2-klein")]
    Flux2Klein,
    #[value(name = "rorem-mixed")]
    RoremMixed,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum FormatChoice {
    #[value(name = "png")]
    Png,
    #[value(name = "jpg")]
    Jpg,
    #[value(name = "webp")]
    Webp,
}

impl FormatChoice {
    fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Webp => "webp",
        }
    }
}

/// A concrete local model choice with its VRAM footprint.
struct Resolved {
    model: String,
    quantization: String,
    estimate: preset::VramEstimate,
    /// Bytes the configuration needs in the store (weights and projector).
    download: u64,
    /// Whether the page image is fed to the model: a `--no-vision` run neither
    /// downloads nor loads the projector and translates from OCR text alone.
    vision: bool,
    /// Whether the model emits reasoning traces the backend must strip.
    reasoning: bool,
}

/// Download size above which an automatic pick warns instead of starting a
/// multi-gigabyte download silently: `auto` now scales up with the GPU, so a
/// large card can select a model the user never asked for.
const LARGE_DOWNLOAD_BYTES: u64 = 8 * 1024 * 1024 * 1024;

/// Bytes `model`/`quantization` needs in the store before the first page can be
/// translated.
fn download_bytes(model: &str, quantization: &str, vision: bool) -> Result<u64> {
    let descriptor = preset::descriptor_for(model)
        .ok_or_else(|| anyhow::anyhow!("unknown local model '{model}'"))?;
    Ok(preset::download_bytes(
        descriptor,
        Some(quantization),
        vision,
    ))
}

/// Store root used when `--store` is not given: the `store` directory next to
/// this executable when it exists (a Koharu app installation), otherwise the
/// operating-system cache default.
fn default_store_root() -> PathBuf {
    if let Some(directory) = dirs::executable_dir() {
        let candidate = directory.join("store");
        if candidate.is_dir() {
            return candidate;
        }
    }
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("koharu")
        .join("packages")
}

fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// Usable VRAM budget in bytes, from the GPU or `--vram-budget`; `None` when
/// unknown (CPU run or no telemetry), in which case `auto` cannot work.
fn detect_budget(arguments: &Arguments, device: &Device) -> Option<u64> {
    if let Some(mib) = arguments.vram_budget_mib {
        return Some(preset::budget_from_total(mib * 1024 * 1024));
    }
    if arguments.cpu {
        return None;
    }
    let budget = koharu_pipeline::vram::total_bytes(device).map(preset::budget_from_total)?;
    // The margin on the total only covers a typical desktop; the compositor,
    // browser tabs, and every other process already own part of the card at
    // startup, so never budget past what is actually free right now.
    let free = koharu_pipeline::vram::free_bytes(device).unwrap_or(u64::MAX);
    Some(budget.min(free))
}

/// Resolves the local LLM: `auto` scans the preference list, an explicit id is
/// checked against the VRAM budget unless `--force` was passed. Both use the
/// real measurements recorded by previous runs when available.
fn resolve_model(
    arguments: &Arguments,
    budget: Option<u64>,
    measurements: &MeasuredPeaks,
) -> Result<Resolved> {
    if arguments.llm == "auto" {
        let Some(budget) = budget else {
            bail!(
                "--llm auto requires a queryable GPU (or --vram-budget <MiB> / --cpu with an explicit --llm)"
            );
        };
        let vision = !arguments.no_vision;
        let Some(choice) = preset::resolve_auto_with(budget, vision, measurements) else {
            bail!(
                "no local translation model fits the VRAM budget {}",
                gib(budget)
            );
        };
        return Ok(Resolved {
            model: choice.model.to_owned(),
            quantization: choice.quantization.to_owned(),
            estimate: choice.estimate,
            download: download_bytes(choice.model, choice.quantization, vision)?,
            vision,
            reasoning: preset::descriptor_for(choice.model).is_some_and(preset::is_reasoning),
        });
    }

    let Some(descriptor) = preset::descriptor_for(&arguments.llm) else {
        bail!(
            "unknown local model '{}' (use --list-models to see the catalog)",
            arguments.llm
        );
    };
    let vision = preset::supports_vision(descriptor) && !arguments.no_vision;
    let quantization = arguments
        .quantization
        .clone()
        .unwrap_or_else(|| preset::default_quantization(descriptor).to_owned());
    if !preset::has_quantization(descriptor, &quantization) {
        bail!(
            "unknown quantization '{quantization}' for '{}'",
            arguments.llm
        );
    }
    let estimate = match budget {
        Some(budget) => match preset::check_budget_with(
            budget,
            &arguments.llm,
            Some(&quantization),
            vision,
            measurements,
        ) {
            Ok(estimate) => estimate,
            Err(preset::BudgetCheck::UnknownModel) => {
                bail!("unknown local model '{}'", arguments.llm)
            }
            Err(preset::BudgetCheck::Exceeded(exceeded)) if arguments.force => {
                eprintln!("warning: --force overrides the VRAM budget ({exceeded})");
                exceeded.estimate
            }
            Err(preset::BudgetCheck::Exceeded(exceeded)) => bail!("{exceeded}"),
        },
        None => preset::estimate_vram_with(descriptor, Some(&quantization), vision, measurements),
    };
    let download = download_bytes(&arguments.llm, &quantization, vision)?;
    Ok(Resolved {
        model: arguments.llm.clone(),
        quantization,
        estimate,
        download,
        vision,
        reasoning: preset::is_reasoning(descriptor),
    })
}

fn pipeline_config(arguments: &Arguments, resolved: &Resolved) -> PipelineConfig {
    PipelineConfig {
        detection: match arguments.detection {
            DetectionChoice::KoharuLayoutRFDetrSeg2XL => {
                DetectionModel::KoharuLayoutRFDetrSeg2XL(KoharuLayoutRFDetrSeg2XLConfig::default())
            }
        },
        ocr: match arguments.ocr {
            OcrChoice::PaddleOcrVl1_6 => OcrModel::PaddleOcrVl1_6,
            OcrChoice::MangaOcr => OcrModel::MangaOcr,
            OcrChoice::BaberuOcr => OcrModel::BaberuOcr,
        },
        translation: TranslationConfig {
            model: ModelSelection {
                provider: Provider::Local,
                model: Some(resolved.model.clone()),
                quantization: Some(resolved.quantization.clone()),
                vision: resolved.vision,
                reasoning: resolved.reasoning,
            },
            // `vision` on the generation gates the image both in the stage
            // (attaching the page crop) and in the translator itself.
            generation: GenerationConfig {
                vision: Some(resolved.vision),
                ..GenerationConfig::default()
            },
            target_language: arguments.lang,
            instructions: arguments.translation_instructions.clone(),
            typography: TypographyProfile::default(),
        },
        inpainting: match arguments.inpainting {
            InpaintingChoice::LaMa => InpaintingModel::LaMa {},
            InpaintingChoice::AotInpainting => InpaintingModel::AotInpainting {},
            InpaintingChoice::Flux2Klein => {
                InpaintingModel::Flux2Klein(Flux2KleinConfig::default())
            }
            InpaintingChoice::RoremMixed => {
                InpaintingModel::RoremMixed(RoremMixedConfig::default())
            }
        },
        processor: Default::default(),
    }
}

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

fn list_input_pages(input: &Path) -> Result<InputPages> {
    if input.is_dir() {
        return Ok(InputPages::Directory(pages::list_directory_pages(input)?));
    }
    if input.is_file() {
        let extension = input
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        if matches!(extension.as_deref(), Some("cbz" | "zip")) {
            return Ok(InputPages::Archive(
                input.to_owned(),
                cbz::list_archive_pages(input)?,
            ));
        }
        bail!("input must be a folder of images or a .cbz archive");
    }
    bail!("input {} does not exist", input.display())
}

fn read_page_bytes(input: &InputPages, source: &pages::PageSource) -> Result<Vec<u8>> {
    match input {
        InputPages::Directory(_) => match &source.location {
            pages::Location::File(path) => {
                fs::read(path).with_context(|| format!("failed to read {}", path.display()))
            }
            pages::Location::Entry(_) => bail!("page {} has an inconsistent source", source.name),
        },
        InputPages::Archive(archive, _) => match &source.location {
            pages::Location::Entry(index) => Ok(cbz::read_entry(archive, *index)?.0),
            pages::Location::File(_) => bail!("page {} has an inconsistent source", source.name),
        },
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
    eprintln!(
        "report written: {} and {}",
        markdown_path.display(),
        html_path.display()
    );
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

/// One page admitted to the phase-major run: its session entity plus the
/// report data accumulated across the pipeline stages.
struct LoadedPage {
    /// Index of the page in the input listing (its report slot).
    index: usize,
    page_id: EntityId,
    name: String,
    media_type: &'static str,
    before_uri: Option<String>,
    /// Active wall time spent on this page (load plus every phase run).
    elapsed: Duration,
    stage_timings: Arc<Mutex<Vec<(String, Duration)>>>,
    /// Largest per-phase GPU peak above baseline attributed to this page.
    vram_peak: Option<u64>,
}

/// Runs one pipeline stage across every surviving page, in reading order.
///
/// The stage's model loads once and stays resident for the whole phase; a
/// phase failure reports the page and drops it from the later phases, keeping
/// the per-page fault isolation of a per-page run.
async fn run_phase(
    stage: Stage,
    pages: &mut Vec<LoadedPage>,
    session: &mut Session,
    pipeline: &Pipeline,
    vram_sampler: Option<&VramSampler>,
    pages_report: &mut [Option<PageReport>],
    failures: &mut Vec<(String, String)>,
) {
    eprintln!("{}: {} page(s)", stage, pages.len());
    let mut index = 0;
    while index < pages.len() {
        let (page_id, timings) = {
            let page = &pages[index];
            (page.page_id, Arc::clone(&page.stage_timings))
        };
        if let Some(sampler) = vram_sampler {
            sampler.reset_window();
        }
        let phase_started = Instant::now();
        let outcome = {
            let snapshot = session.snapshot();
            let mut committer = SessionCommitter(&mut *session);
            pipeline
                .execute(
                    snapshot,
                    Request {
                        operation: Operation::Only { stage },
                        scope: Scope::Pages(vec![page_id]),
                        progress: Some(Arc::new(move |event| {
                            if let Progress::Finished { stage, elapsed, .. } = event {
                                eprintln!("  {stage} finished in {:.2}s", elapsed.as_secs_f64());
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
        match outcome {
            Ok(_) => {
                let page = &mut pages[index];
                page.elapsed += phase_started.elapsed();
                if let Some(sampler) = vram_sampler
                    && let Some(delta) = sampler.window_delta_bytes()
                {
                    page.vram_peak = Some(page.vram_peak.map_or(delta, |peak| peak.max(delta)));
                }
                index += 1;
            }
            Err(error) => {
                let page = pages.remove(index);
                eprintln!("[{}] {} failed: {error:#}", page.name, stage);
                pages_report[page.index] = Some(PageReport::failed(
                    page.index,
                    page.name.clone(),
                    format!("{} failed: {error:#}", stage),
                ));
                failures.push((page.name.clone(), error.to_string()));
            }
        }
    }
}

/// Ends the process with `code`, skipping native teardown entirely.
///
/// After a full GPU run, process shutdown segfaults inside the CUDA driver
/// (`nvcuda64.dll` fault, observed in both debug and release) and would
/// replace the documented exit code with `0xC0000005` — turning a successful
/// chapter into a failure for any script checking the exit status. Every
/// output file is already written and stderr is unbuffered at the call
/// sites, so nothing depends on CRT destructors or DLL detach handlers;
/// terminating directly is what the kernel does after any such crash anyway.
fn exit_with(code: i32) -> ! {
    #[cfg(windows)]
    unsafe {
        TerminateProcess(GetCurrentProcess(), code as u32);
    }
    // Only reachable if the kernel refused to terminate this process.
    std::process::exit(code)
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut core::ffi::c_void;
    fn TerminateProcess(process: *mut core::ffi::c_void, exit_code: u32) -> i32;
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let calibration_path = calibration::default_path();
    if arguments.reset_calibration {
        reset_calibration(calibration_path.as_deref())?;
        return Ok(());
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
        return Ok(());
    }
    let run_started_at = report::timestamp_now();
    let Some(input_path) = arguments.input.as_deref() else {
        bail!("--input is required (folder of images or a .cbz archive)");
    };
    let Some(output) = arguments.output.clone() else {
        bail!("--output is required (folder for page images, or a .cbz path)");
    };
    let store = arguments.store.clone().unwrap_or_else(default_store_root);
    koharu_runtime::Store::configure(&store).with_context(|| {
        format!(
            "failed to configure the runtime store at {}",
            store.display()
        )
    })?;

    let input = list_input_pages(input_path)?;
    let all_pages = input.list();
    let output_is_archive = output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cbz") || extension.eq_ignore_ascii_case("zip")
        });
    if output_is_archive {
        if let Some(parent) = output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
    } else {
        fs::create_dir_all(&output)
            .with_context(|| format!("failed to create {}", output.display()))?;
    }

    let resume_skip = |page: &pages::PageSource| {
        !arguments.overwrite
            && !output_is_archive
            && output_page_path(&output, page.index, arguments.format.extension()).exists()
    };
    let mut pages_report: Vec<Option<PageReport>> = all_pages
        .iter()
        .map(|page| {
            resume_skip(page).then(|| PageReport {
                index: page.index,
                name: page.name.clone(),
                outcome: PageOutcome::Skipped,
            })
        })
        .collect();
    let pending: Vec<&pages::PageSource> =
        all_pages.iter().filter(|page| !resume_skip(page)).collect();
    eprintln!(
        "{} pages ({} to translate) -> {}",
        all_pages.len(),
        pending.len(),
        output.display()
    );

    let device = koharu_ml::device(arguments.cpu);
    let device_description = device.description.clone();
    let vram_sampler = VramSampler::start_default(&device);
    let budget = detect_budget(&arguments, &device);
    let resolved = resolve_model(&arguments, budget, &measurements)?;
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
    if arguments.llm == "auto" && resolved.download >= LARGE_DOWNLOAD_BYTES {
        eprintln!(
            "warning: --llm auto picked {} {} because this GPU fits it; the first real run downloads about {} into {} (one time). Pass --llm <id> to translate with a smaller model, or --list-models to compare sizes.",
            resolved.model,
            resolved.quantization,
            gib(resolved.download),
            store.display()
        );
    }

    let config = pipeline_config(&arguments, &resolved);
    let metadata = RunMetadata {
        model: resolved.model.clone(),
        quantization: resolved.quantization.clone(),
        vram_estimate: resolved.estimate.display(),
        language: arguments.lang.tag().to_owned(),
        input: input_path.display().to_string(),
        output: output.display().to_string(),
        device: device_description,
        started_at: run_started_at,
    };
    let report_base = match arguments.report.as_deref() {
        Some("none") => None,
        Some(base) => Some(PathBuf::from(base)),
        None => Some(output.with_extension("report")),
    };
    let started = Instant::now();
    if arguments.dry_run {
        eprintln!("dry run: no runtime download, no page processed");
        for page in &pending {
            eprintln!("  would translate: {}", page.name);
        }
        return Ok(());
    }
    if pending.is_empty() {
        eprintln!("nothing to do; use --overwrite to re-translate existing pages");
        if let Some(base) = &report_base {
            write_run_report(
                base,
                pages_report.into_iter().flatten().collect(),
                started.elapsed(),
                &metadata,
                vram_sampler.as_ref(),
            )?;
        }
        return Ok(());
    }

    bootstrap::initialize_with_retry().await;
    let pipeline = Pipeline::from_config(
        Config::memory(config),
        Config::memory(ProvidersConfig::default()),
        device,
    )?;
    let mut session = Session::memory().await?;
    let renderer = Renderer::new()?;
    let rasterizer = Rasterizer::new()?;
    let mut archive_output = output_is_archive
        .then(|| cbz::ArchiveOutput::create(&output))
        .transpose()?;
    let mut failures = Vec::new();
    let total_pages = pending.len();

    // Phase-major execution: every page is added to the session first, then
    // each stage runs across all surviving pages before the next stage starts.
    // Models therefore load once per chapter instead of once per page — on a
    // small card the vision and language models cannot coexist and used to be
    // evicted and reloaded around every page. Fault isolation is preserved: a
    // phase failure drops that page from the later phases only.
    let mut loaded: Vec<LoadedPage> = Vec::new();
    for page in &pending {
        let load_started = Instant::now();
        let page_name = page.name.clone();
        let bytes = match read_page_bytes(&input, page) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("[{page_name}] failed to read: {error:#}");
                pages_report[page.index] = Some(PageReport::failed(
                    page.index,
                    page_name.clone(),
                    format!("read failed: {error:#}"),
                ));
                failures.push((page_name, error.to_string()));
                continue;
            }
        };
        let (decoded, before_uri) = match image::load_from_memory(&bytes) {
            Ok(decoded) => {
                let uri = thumbnail_data_uri(&decoded)
                    .map_err(|error| eprintln!("[{page_name}] thumbnail skipped: {error}"))
                    .ok();
                (decoded, uri)
            }
            Err(error) => {
                eprintln!("[{page_name}] failed to decode: {error}");
                pages_report[page.index] = Some(PageReport::failed(
                    page.index,
                    page_name.clone(),
                    format!("decode failed: {error}"),
                ));
                failures.push((page_name, "decode failed".to_owned()));
                continue;
            }
        };

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
        })?;
        session.commit(patch).await?;
        loaded.push(LoadedPage {
            index: page.index,
            page_id: session_page.expect("page ID is assigned by the edit"),
            name: page_name,
            media_type: page.media_type,
            before_uri,
            elapsed: load_started.elapsed(),
            stage_timings: Arc::new(Mutex::new(Vec::new())),
            vram_peak: None,
        });
    }

    run_phase(
        Stage::Detection,
        &mut loaded,
        &mut session,
        &pipeline,
        vram_sampler.as_ref(),
        &mut pages_report,
        &mut failures,
    )
    .await;
    run_phase(
        Stage::Ocr,
        &mut loaded,
        &mut session,
        &pipeline,
        vram_sampler.as_ref(),
        &mut pages_report,
        &mut failures,
    )
    .await;
    run_phase(
        Stage::Inpainting,
        &mut loaded,
        &mut session,
        &pipeline,
        vram_sampler.as_ref(),
        &mut pages_report,
        &mut failures,
    )
    .await;

    // Translation runs last (the chapter context needs every earlier page's
    // text, and the language model loads once the vision models have been
    // evicted), and each page is rendered and written right after its own
    // translation so outputs still appear progressively during this phase.
    let mut remaining = loaded;
    while !remaining.is_empty() {
        let (page_id, timings) = {
            let page = &remaining[0];
            (page.page_id, Arc::clone(&page.stage_timings))
        };
        if let Some(sampler) = &vram_sampler {
            sampler.reset_window();
        }
        let phase_started = Instant::now();
        let outcome = {
            let snapshot = session.snapshot();
            let mut committer = SessionCommitter(&mut session);
            pipeline
                .execute(
                    snapshot,
                    Request {
                        operation: Operation::Only {
                            stage: Stage::Translation,
                        },
                        scope: Scope::Pages(vec![page_id]),
                        progress: Some(Arc::new(move |event| {
                            if let Progress::Finished { stage, elapsed, .. } = event {
                                eprintln!("  {stage} finished in {:.2}s", elapsed.as_secs_f64());
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
        if let Err(error) = &outcome {
            let page = remaining.remove(0);
            eprintln!("[{}] translation failed: {error:#}", page.name);
            pages_report[page.index] = Some(PageReport::failed(
                page.index,
                page.name.clone(),
                format!("translation failed: {error:#}"),
            ));
            failures.push((page.name.clone(), error.to_string()));
            continue;
        }
        let mut page = remaining.remove(0);
        page.elapsed += phase_started.elapsed();
        if let Some(sampler) = &vram_sampler
            && let Some(delta) = sampler.window_delta_bytes()
        {
            page.vram_peak = Some(page.vram_peak.map_or(delta, |peak| peak.max(delta)));
        }
        let page_name = page.name.clone();

        let finalize_started = Instant::now();
        let frame = renderer.render(&session.snapshot(), page.page_id).await?;
        let raster = rasterizer.rasterize(&frame.raster_frame()?, RasterOptions::default())?;
        let encoded = match encode_image(&raster.image, arguments.format) {
            Ok(encoded) => encoded,
            Err(error) => {
                eprintln!("[{page_name}] failed to encode: {error}");
                pages_report[page.index] = Some(PageReport::failed(
                    page.index,
                    page_name.clone(),
                    format!("encode failed: {error}"),
                ));
                failures.push((page_name, error.to_string()));
                continue;
            }
        };
        match archive_output.as_mut() {
            Some(archive) => archive.add_page(&page_name, page.media_type, &encoded)?,
            None => {
                let target = output_page_path(&output, page.index, arguments.format.extension());
                fs::write(&target, &encoded)
                    .with_context(|| format!("failed to write {}", target.display()))?;
            }
        }
        let after_uri = thumbnail_data_uri(&image::DynamicImage::ImageRgba8(raster.image.clone()))
            .map_err(|error| eprintln!("[{page_name}] thumbnail (after) skipped: {error}"))
            .ok();
        let page_vram_peak = page.vram_peak.map(gib);
        let elapsed = page.elapsed + finalize_started.elapsed();
        eprintln!("[{page_name}] done in {:.2}s", elapsed.as_secs_f64());
        let thumbnails = match (page.before_uri, after_uri) {
            (Some(before), Some(after)) => Some(Thumbnails { before, after }),
            _ => None,
        };
        pages_report[page.index] = Some(PageReport {
            index: page.index,
            name: page_name,
            outcome: PageOutcome::Translated {
                elapsed,
                stages: page
                    .stage_timings
                    .lock()
                    .expect("stage timings mutex")
                    .clone(),
                thumbnails,
                vram_peak: page_vram_peak,
            },
        });
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

    if let Some(archive) = archive_output {
        archive.finish()?;
    }

    if let Some(base) = &report_base {
        write_run_report(
            base,
            pages_report.into_iter().flatten().collect(),
            started.elapsed(),
            &metadata,
            vram_sampler.as_ref(),
        )?;
    }

    if failures.is_empty() {
        eprintln!(
            "chapter translated in {:.2}s ({} pages)",
            started.elapsed().as_secs_f64(),
            total_pages
        );
        // Exit explicitly: the native teardown (CUDA driver) segfaults after a
        // full run and would replace this success code with 0xC0000005.
        exit_with(0);
    }
    eprintln!(
        "chapter finished with {} failure(s) in {:.2}s:",
        failures.len(),
        started.elapsed().as_secs_f64()
    );
    for (name, error) in &failures {
        eprintln!("  - {name}: {error}");
    }
    let error = anyhow::anyhow!("{} page(s) failed", failures.len());
    eprintln!("Error: {error:?}");
    // Same reason as above: report the real failure code, not a teardown crash.
    exit_with(1);
}
