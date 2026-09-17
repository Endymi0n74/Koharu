//! `koharu-batch` translates a whole chapter — a folder of images or a CBZ —
//! in one command, page by page, with resume support and VRAM budgeting.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use koharu_config::Config;
use koharu_ml::Device;
use koharu_pipeline::batch::{bootstrap, cbz, pages};
use koharu_pipeline::{
    Committer, DetectionModel, Flux2KleinConfig, InpaintingModel, KoharuLayoutRFDetrSeg2XLConfig,
    OcrModel, Operation, Pipeline, PipelineConfig, Progress, Request, RoremMixedConfig, Scope,
    StageOutput, TranslationConfig,
};
use koharu_renderer::{RasterOptions, Renderer};
use koharu_scene::{AssetInput, AssetMetadata, AssetRole, At, PageDraft, Session};
use koharu_translator::preset;
use koharu_translator::{
    GenerationConfig, Language, ModelSelection, Provider, ProvidersConfig, TypographyProfile,
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
    input: PathBuf,

    /// Output folder (pages as images) or .cbz path (packaged chapter).
    #[arg(short, long, value_name = "OUTPUT")]
    output: PathBuf,

    /// Target language tag (fr-FR, en-US, …).
    #[arg(long, default_value = "fr-FR")]
    lang: Language,

    /// Local LLM id, or `auto` to pick the best one for the available VRAM.
    #[arg(long, default_value = "auto")]
    llm: String,

    /// Local LLM quantization (defaults to the model's first entry).
    #[arg(long)]
    quantization: Option<String>,

    /// Skip the VRAM budget check (not recommended).
    #[arg(long)]
    force: bool,

    /// Assume this much VRAM in MiB instead of querying the GPU.
    #[arg(long, value_name = "MIB")]
    vram_budget_mib: Option<u64>,

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
    koharu_pipeline::vram::total_bytes(device).map(preset::budget_from_total)
}

/// Resolves the local LLM: `auto` scans the preference list, an explicit id is
/// checked against the VRAM budget unless `--force` was passed.
fn resolve_model(arguments: &Arguments, budget: Option<u64>) -> Result<Resolved> {
    if arguments.llm == "auto" {
        let Some(budget) = budget else {
            bail!("--llm auto requires a queryable GPU (or --vram-budget <MiB> / --cpu with an explicit --llm)");
        };
        let Some(choice) = preset::resolve_auto(budget, true) else {
            bail!(
                "no local translation model fits the VRAM budget {}",
                gib(budget)
            );
        };
        return Ok(Resolved {
            model: choice.model.to_owned(),
            quantization: choice.quantization.to_owned(),
            estimate: choice.estimate,
        });
    }

    let Some(descriptor) = preset::descriptor_for(&arguments.llm) else {
        bail!(
            "unknown local model '{}' (use --list-models to see the catalog)",
            arguments.llm
        );
    };
    let vision = preset::supports_vision(descriptor);
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
        Some(budget) => match preset::check_budget(budget, &arguments.llm, Some(&quantization), vision)
        {
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
        None => preset::estimate_vram(descriptor, Some(&quantization), vision),
    };
    Ok(Resolved {
        model: arguments.llm.clone(),
        quantization,
        estimate,
    })
}

fn pipeline_config(arguments: &Arguments, resolved: &Resolved) -> PipelineConfig {
    PipelineConfig {
        detection: match arguments.detection {
            DetectionChoice::KoharuLayoutRFDetrSeg2XL => {
                DetectionModel::KoharuLayoutRFDetrSeg2XL(
                    KoharuLayoutRFDetrSeg2XLConfig::default(),
                )
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
                vision: true,
            },
            generation: GenerationConfig::default(),
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
    Directory(PathBuf, Vec<pages::PageSource>),
    Archive(PathBuf, Vec<pages::PageSource>),
}

impl InputPages {
    fn list(&self) -> &[pages::PageSource] {
        match self {
            Self::Directory(_, entries) | Self::Archive(_, entries) => entries,
        }
    }
}

fn list_input_pages(input: &Path) -> Result<InputPages> {
    if input.is_dir() {
        return Ok(InputPages::Directory(
            input.to_owned(),
            pages::list_directory_pages(input)?,
        ));
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
        InputPages::Directory(directory, _) => match &source.location {
            pages::Location::File(path) => fs::read(directory.join(path))
                .with_context(|| format!("failed to read {}", path.display())),
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

fn list_models() {
    println!(
        "{:<26} {:<7} {:<14} {}",
        "model", "vision", "quantization", "estimated VRAM"
    );
    for model in preset::vram_catalog(true) {
        for quantization in &model.quantizations {
            println!(
                "{:<26} {:<7} {:<14} {}",
                model.id,
                if model.vision { "yes" } else { "no" },
                quantization.id,
                quantization.estimate.display()
            );
        }
    }
}

fn encode_image(image: &image::ImageBuffer<image::Rgba<u8>, Vec<u8>>, format: FormatChoice) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    let target = match format {
        FormatChoice::Png => image::ImageFormat::Png,
        FormatChoice::Jpg => image::ImageFormat::Jpeg,
        FormatChoice::Webp => image::ImageFormat::WebP,
    };
    image.write_to(&mut std::io::Cursor::new(&mut encoded), target)?;
    Ok(encoded)
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    if arguments.list_models {
        list_models();
        return Ok(());
    }
    bootstrap::initialize_with_retry().await;

    let device = koharu_ml::device(arguments.cpu);
    let budget = detect_budget(&arguments, &device);
    let resolved = resolve_model(&arguments, budget)?;
    eprintln!(
        "translation model: {} {} ({}{})",
        resolved.model,
        resolved.quantization,
        resolved.estimate.display(),
        budget.map_or_else(
            || ", budget unknown".to_owned(),
            |budget| format!(" within {}", gib(budget))
        )
    );

    let input = list_input_pages(&arguments.input)?;
    let all_pages = input.list();
    let output_is_archive = arguments
        .output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cbz") || extension.eq_ignore_ascii_case("zip")
        });
    if output_is_archive && input.list().iter().any(|page| matches!(page.location, pages::Location::File(_))) && false {
        unreachable!("folder inputs cannot be rejected here");
    }

    let pending: Vec<&pages::PageSource> = all_pages
        .iter()
        .filter(|page| {
            if arguments.overwrite || output_is_archive {
                return true;
            }
            !output_page_path(&arguments.output, page.index, arguments.format.extension()).exists()
        })
        .collect();
    eprintln!(
        "{} pages ({} to translate) -> {}",
        all_pages.len(),
        pending.len(),
        arguments.output.display()
    );

    let config = pipeline_config(&arguments, &resolved);
    if arguments.dry_run {
        eprintln!("dry run: no model download, no page processed");
        for page in &pending {
            eprintln!("  would translate: {}", page.name);
        }
        return Ok(());
    }
    if pending.is_empty() {
        eprintln!("nothing to do; use --overwrite to re-translate existing pages");
        return Ok(());
    }

    let pipeline = Pipeline::from_config(
        Config::memory(config),
        Config::memory(ProvidersConfig::default()),
        device,
    )?;
    let mut session = Session::memory().await?;
    let renderer = Renderer::new()?;
    let mut archive_output = output_is_archive
        .then(|| cbz::ArchiveOutput::create(&arguments.output))
        .transpose()?;
    let mut failures = Vec::new();
    let total_pages = pending.len();
    let started = Instant::now();

    for page in &pending {
        let page_name = page.name.clone();
        let bytes = match read_page_bytes(&input, page) {
            Ok(bytes) => bytes,
            Err(error) => {
                eprintln!("[{page_name}] failed to read: {error:#}");
                failures.push((page_name, error.to_string()));
                continue;
            }
        };
        let decoded = match image::load_from_memory(&bytes) {
            Ok(decoded) => decoded,
            Err(error) => {
                eprintln!("[{page_name}] failed to decode: {error}");
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
        let page_id = session_page.expect("page ID is assigned by the edit");

        let snapshot = session.snapshot();
        let mut committer = SessionCommitter(&mut session);
        let report = match pipeline
            .execute(
                snapshot,
                Request {
                    operation: Operation::Full,
                    scope: Scope::Pages(vec![page_id]),
                    progress: Some(Arc::new(|event| {
                        if let Progress::Finished { stage, elapsed, .. } = event {
                            eprintln!("  {stage} finished in {:.2}s", elapsed.as_secs_f64());
                        }
                    })),
                    ..Request::default()
                },
                &mut committer,
            )
            .await
        {
            Ok(report) => report,
            Err(error) => {
                eprintln!("[{page_name}] translation failed: {error:#}");
                failures.push((page_name, error.to_string()));
                continue;
            }
        };

        let frame = renderer.render(&session.snapshot(), page_id).await?;
        let raster = renderer.rasterize(&frame, RasterOptions::default()).await?;
        let encoded = match encode_image(&raster.image, arguments.format) {
            Ok(encoded) => encoded,
            Err(error) => {
                eprintln!("[{page_name}] failed to encode: {error}");
                failures.push((page_name, error.to_string()));
                continue;
            }
        };
        match archive_output.as_mut() {
            Some(archive) => archive.add_page(&page_name, page.media_type, &encoded)?,
            None => {
                let target = output_page_path(
                    &arguments.output,
                    page.index,
                    arguments.format.extension(),
                );
                fs::write(&target, &encoded)
                    .with_context(|| format!("failed to write {}", target.display()))?;
            }
        }
        eprintln!("[{page_name}] done in {:.2}s", report.elapsed.as_secs_f64());
    }

    if let Some(archive) = archive_output {
        archive.finish()?;
    }

    if failures.is_empty() {
        eprintln!(
            "chapter translated in {:.2}s ({} pages)",
            started.elapsed().as_secs_f64(),
            total_pages
        );
        return Ok(());
    }
    eprintln!(
        "chapter finished with {} failure(s) in {:.2}s:",
        failures.len(),
        started.elapsed().as_secs_f64()
    );
    for (name, error) in &failures {
        eprintln!("  - {name}: {error}");
    }
    bail!("{} page(s) failed", failures.len())
}
