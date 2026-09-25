//! Command line of `koharu-batch`: the flags it accepts, the local model they
//! resolve to on this machine's VRAM budget, and the pipeline configuration
//! that model implies.
//!
//! Kept in the library so `cargo test` covers it: unit tests inside a binary
//! target are not run by `cargo test --tests`, which is what CI executes.

use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use clap::{Parser, ValueEnum};
use koharu_ml::Device;
use koharu_translator::preset::{self, MeasuredPeaks};
use koharu_translator::{GenerationConfig, Language, ModelSelection, Provider, TypographyProfile};

use crate::{
    DetectionModel, Flux2KleinConfig, InpaintingModel, KoharuLayoutRFDetrSeg2XLConfig, OcrModel,
    PipelineConfig, RoremMixedConfig, TranslationConfig, vram,
};

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Translate a chapter — or a whole volume — (folders of images or CBZ) with Koharu's local pipeline"
)]
pub struct Arguments {
    /// Folder of images or a .cbz file to translate.
    #[arg(short, long, value_name = "INPUT")]
    pub input: Option<PathBuf>,

    /// Output folder (pages as images) or .cbz path (packaged chapter).
    #[arg(short, long, value_name = "OUTPUT")]
    pub output: Option<PathBuf>,

    /// Target language tag (fr-FR, en-US, …). English (en-US) is a safe
    /// fallback: every catalog model handles it well, and JA/RU → EN is
    /// typically the strongest pair for the small local models.
    #[arg(long, default_value = "fr-FR")]
    pub lang: Language,

    /// Local LLM id, or `auto` to pick the strongest model that fits the
    /// available VRAM (a big pick is downloaded on the first run).
    #[arg(long, default_value = "auto")]
    pub llm: String,

    /// Local LLM quantization (defaults to the model's first entry).
    #[arg(long)]
    pub quantization: Option<String>,

    /// Translate text-only: never feed the page image to the LLM, skipping
    /// the vision projector's download and its VRAM.
    #[arg(long)]
    pub no_vision: bool,

    /// Skip the VRAM budget check (not recommended).
    #[arg(long)]
    pub force: bool,

    /// Assume this much VRAM in MiB instead of querying the GPU.
    #[arg(long, value_name = "MIB")]
    pub vram_budget_mib: Option<u64>,

    /// Runtime store holding models and runtimes (defaults to the app
    /// install's `store` directory when the binary lives next to it).
    #[arg(long, value_name = "DIR")]
    pub store: Option<PathBuf>,

    /// End-of-run report: writes `<BASE>.md` and `<BASE>.html`; pass `none`
    /// to disable. Defaults to `<OUTPUT-STEM>-report` next to the output.
    #[arg(long, value_name = "BASE|none")]
    pub report: Option<String>,

    /// Write a machine-readable JSON summary of the run (per-chapter and
    /// per-page statuses, counts, failures) to this path.
    #[arg(long, value_name = "PATH")]
    pub json: Option<PathBuf>,

    /// Delete the VRAM calibration file and exit; later runs fall back to the
    /// built-in reference estimates until new measurements are recorded.
    #[arg(long)]
    pub reset_calibration: bool,

    /// Run without reading or writing the calibration file: built-in
    /// estimates only, nothing persisted.
    #[arg(long)]
    pub no_calibration: bool,

    #[arg(long, value_enum, default_value = "koharu-layout-rfdetr-seg-2xl")]
    pub detection: DetectionChoice,

    #[arg(long, value_enum, default_value = "paddleocr-vl-1.6")]
    pub ocr: OcrChoice,

    #[arg(long, value_enum, default_value = "lama")]
    pub inpainting: InpaintingChoice,

    /// Extra instructions passed to the translator.
    #[arg(long)]
    pub translation_instructions: Option<String>,

    /// Output image format when writing a folder.
    #[arg(long, value_enum, default_value = "png")]
    pub format: FormatChoice,

    /// List local models with their VRAM estimates and exit.
    #[arg(long)]
    pub list_models: bool,

    /// Re-translate pages whose output already exists.
    #[arg(long)]
    pub overwrite: bool,

    /// Extra attempts granted to a page whose stage fails, before it is
    /// dropped from the run (0 keeps the first failure).
    #[arg(long, default_value_t = 1)]
    pub retries: usize,

    /// Plan the run (pages, model, VRAM) without executing it.
    #[arg(long)]
    pub dry_run: bool,

    /// Report failures and the final summary only, without per-page progress.
    #[arg(long)]
    pub quiet: bool,

    /// Translate only these pages, in 1-based reading order (`1-10,15`).
    #[arg(long, value_name = "RANGE")]
    pub pages: Option<String>,

    /// Also take the pages held in subdirectories of the input folder.
    #[arg(long)]
    pub recursive: bool,

    /// Force CPU execution.
    #[arg(long)]
    pub cpu: bool,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum DetectionChoice {
    #[value(name = "koharu-layout-rfdetr-seg-2xl")]
    KoharuLayoutRFDetrSeg2XL,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum OcrChoice {
    #[value(name = "paddleocr-vl-1.6")]
    PaddleOcrVl1_6,
    #[value(name = "manga-ocr")]
    MangaOcr,
    #[value(name = "baberu-ocr")]
    BaberuOcr,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum InpaintingChoice {
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
pub enum FormatChoice {
    #[value(name = "png")]
    Png,
    #[value(name = "jpg")]
    Jpg,
    #[value(name = "webp")]
    Webp,
}

impl FormatChoice {
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpg => "jpg",
            Self::Webp => "webp",
        }
    }
}

/// A concrete local model choice with its VRAM footprint.
pub struct Resolved {
    pub model: String,
    pub quantization: String,
    pub estimate: preset::VramEstimate,
    /// Bytes the configuration needs in the store (weights and projector).
    pub download: u64,
    /// Whether the page image is fed to the model: a `--no-vision` run neither
    /// downloads nor loads the projector and translates from OCR text alone.
    pub vision: bool,
    /// Whether the model emits reasoning traces the backend must strip.
    pub reasoning: bool,
}

/// Download size above which an automatic pick warns instead of starting a
/// multi-gigabyte download silently: `auto` now scales up with the GPU, so a
/// large card can select a model the user never asked for.
pub const LARGE_DOWNLOAD_BYTES: u64 = 8 * 1024 * 1024 * 1024;

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
#[must_use]
pub fn default_store_root() -> PathBuf {
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

#[must_use]
pub fn gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

/// Usable VRAM budget in bytes, from the GPU or `--vram-budget`; `None` when
/// unknown (CPU run or no telemetry), in which case `auto` cannot work.
#[must_use]
pub fn detect_budget(arguments: &Arguments, device: &Device) -> Option<u64> {
    if let Some(mib) = arguments.vram_budget_mib {
        return Some(preset::budget_from_total(mib * 1024 * 1024));
    }
    if arguments.cpu {
        return None;
    }
    let budget = vram::total_bytes(device).map(preset::budget_from_total)?;
    // The margin on the total only covers a typical desktop; the compositor,
    // browser tabs, and every other process already own part of the card at
    // startup, so never budget past what is actually free right now.
    let free = vram::free_bytes(device).unwrap_or(u64::MAX);
    Some(budget.min(free))
}

/// Resolves the local LLM: `auto` scans the preference list, an explicit id is
/// checked against the VRAM budget unless `--force` was passed. Both use the
/// real measurements recorded by previous runs when available.
pub fn resolve_model(
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

/// Zero-based reading-order indices selected by a `--pages` specification
/// such as `1-10,15`. An empty or out-of-range selection is an error: quietly
/// translating nothing would look like a success.
#[must_use = "the selection is what the run translates"]
pub fn parse_page_range(specification: &str, total: usize) -> Result<Vec<usize>> {
    let mut selected = Vec::new();
    for part in specification
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        let (from, to) = part.split_once('-').unwrap_or((part, part));
        let start: usize = from
            .trim()
            .parse()
            .with_context(|| format!("'{part}' is not a page range"))?;
        let end: usize = to
            .trim()
            .parse()
            .with_context(|| format!("'{part}' is not a page range"))?;
        if start == 0 || end == 0 || start > end {
            bail!("page range '{part}' must be 1-based and ordered (for example 1-10)");
        }
        if end > total {
            bail!("page range '{part}' is out of range: the input has {total} page(s)");
        }
        selected.extend(start..=end);
    }
    if selected.is_empty() {
        bail!("--pages selects no page");
    }
    selected.sort_unstable();
    selected.dedup();
    Ok(selected.into_iter().map(|page| page - 1).collect())
}

/// Pipeline configuration implied by the resolved model and the flags.
#[must_use]
pub fn pipeline_config(arguments: &Arguments, resolved: &Resolved) -> PipelineConfig {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_written_under_the_requested_format() {
        assert_eq!(FormatChoice::Png.extension(), "png");
        assert_eq!(FormatChoice::Jpg.extension(), "jpg");
        assert_eq!(FormatChoice::Webp.extension(), "webp");
    }

    #[test]
    fn a_page_range_is_one_based_ordered_and_bounded() {
        assert_eq!(parse_page_range("1-3,7", 10).unwrap(), [0, 1, 2, 6]);
        assert_eq!(parse_page_range("2", 10).unwrap(), [1]);
        assert_eq!(
            parse_page_range("1-10,5", 10).unwrap(),
            (0..10).collect::<Vec<_>>(),
            "overlapping ranges are merged"
        );
        for invalid in ["", "0-2", "5-2", "9-20", "one"] {
            assert!(
                parse_page_range(invalid, 10).is_err(),
                "{invalid:?} must be rejected"
            );
        }
    }

    #[test]
    fn quiet_and_dry_run_are_off_by_default() {
        let plain = Arguments::parse_from(["koharu-batch", "--input", "in", "--output", "out"]);
        assert!(!plain.quiet);
        assert!(!plain.dry_run);

        let silent = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out.cbz",
            "--quiet",
        ]);
        assert!(silent.quiet);
        assert_eq!(
            silent.output.as_deref(),
            Some(std::path::Path::new("out.cbz"))
        );
    }

    #[test]
    fn cli_defaults_target_french_without_calibration_or_vision() {
        let arguments = Arguments::parse_from(["koharu-batch", "--input", "in", "--output", "out"]);
        assert_eq!(arguments.lang.tag(), "fr-FR");
        assert_eq!(arguments.llm, "auto");
        assert_eq!(arguments.retries, 1);
        assert_eq!(arguments.format.extension(), "png");
        assert!(!arguments.no_vision);
        assert!(!arguments.no_calibration);
    }

    #[test]
    fn json_summary_is_opt_in_and_resolves_a_path() {
        let plain = Arguments::parse_from(["koharu-batch", "--input", "in", "--output", "out"]);
        assert!(plain.json.is_none());

        let traced = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out.cbz",
            "--json",
            "summary.json",
        ]);
        assert_eq!(
            traced.json.as_deref(),
            Some(std::path::Path::new("summary.json"))
        );
    }

    #[test]
    fn an_explicit_retry_count_overrides_the_default() {
        let arguments = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--retries",
            "3",
        ]);
        assert_eq!(arguments.retries, 3);

        let forced = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out.cbz",
            "--no-vision",
            "--format",
            "jpg",
        ]);
        assert!(forced.no_vision);
        assert_eq!(forced.format.extension(), "jpg");
    }
}
