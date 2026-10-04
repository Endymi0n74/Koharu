//! Command line of `koharu-batch`: the flags it accepts, the local model they
//! resolve to on this machine's VRAM budget, and the pipeline configuration
//! that model implies.
//!
//! Kept in the library so `cargo test` covers it: unit tests inside a binary
//! target are not run by `cargo test --tests`, which is what CI executes.

use std::path::PathBuf;

use anyhow::{Context as _, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use koharu_ml::Device;
use koharu_translator::preset::{self, MeasuredPeaks};
use koharu_translator::{GenerationConfig, Language, ModelSelection, Provider, TypographyProfile};

use crate::{
    DetectionModel, Flux2KleinConfig, InpaintingModel, KoharuLayoutRFDetrSeg2XLConfig, OcrModel,
    PipelineConfig, QwenImageConfig, RoremMixedConfig, TranslationConfig, vram,
};

#[derive(Clone, Debug, Parser)]
#[command(
    version,
    about = "Translate a chapter — or a whole volume — (folders of images or CBZ) with Koharu's local pipeline",
    // The default flow (no subcommand) translates; the subcommands below are
    // standalone housekeeping. Mixing the two would silently drop one side —
    // `--input in models` translating nothing, `prune --delete --overwrite`
    // deleting anyway — so clap refuses the combination: a run's flags stand
    // alone, a subcommand's own options follow its name (`prune --store DIR`).
    args_conflicts_with_subcommands = true
)]
pub struct Arguments {
    /// The standalone actions (`models`, `prune`, `reset-calibration`): they
    /// translate no page and take none of the run's flags.
    #[command(subcommand)]
    pub command: Option<Command>,

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

    /// Translation backend: `local` (the catalog, default) or a hosted
    /// provider id from the app's provider settings (`openai`, `deepl`, …).
    /// A hosted run keeps detection/OCR/inpainting on this machine and sends
    /// only the translation over the network; the API key comes from the
    /// app's keyring, the endpoint settings from its configuration file.
    #[arg(long, default_value = "local", help_heading = "Model & VRAM")]
    pub provider: Provider,

    /// Translation model: with `--provider local`, a catalog id or `auto` —
    /// the strongest model that fits the available VRAM (a big pick is
    /// downloaded on the first run; a pick below the 2B quality floor
    /// refuses unless `--force`). With a hosted provider, its model id;
    /// `auto` asks for the service without one, which only the model-less
    /// providers (DeepL and friends) accept.
    #[arg(long, default_value = "auto")]
    pub llm: String,

    /// Local LLM quantization (defaults to the model's first entry).
    #[arg(long, help_heading = "Model & VRAM")]
    pub quantization: Option<String>,

    /// Translate text-only: never feed the page image to the LLM, skipping
    /// the vision projector's download and its VRAM.
    #[arg(long, help_heading = "Model & VRAM")]
    pub no_vision: bool,

    /// Skip the VRAM budget check and `auto`'s quality floor (not recommended).
    #[arg(long, help_heading = "Model & VRAM")]
    pub force: bool,

    /// Assume this much VRAM in MiB instead of querying the GPU.
    #[arg(long, value_name = "MIB", help_heading = "Model & VRAM")]
    pub vram_budget_mib: Option<u64>,

    /// Runtime store holding models and runtimes (`--store` wins, else
    /// `KOHARU_STORE`, else the app install's `store` directory when the
    /// binary lives next to it, else the OS cache).
    /// Global, so every subcommand that touches the store takes it too —
    /// after the name: `koharu-batch prune --store DIR`.
    #[arg(long, value_name = "DIR", global = true, help_heading = "Model & VRAM")]
    pub store: Option<PathBuf>,

    /// End-of-run report: writes `<BASE>.md` and `<BASE>.html`; pass `none`
    /// to disable. Defaults next to the output, named after it — an output
    /// `./out` yields `./out.md` and `./out.html`.
    #[arg(long, value_name = "BASE|none")]
    pub report: Option<String>,

    /// Write a machine-readable JSON summary of the run (per-chapter and
    /// per-page statuses, counts, failures) to this path.
    #[arg(long, value_name = "PATH")]
    pub json: Option<PathBuf>,

    /// Layout detection model: finds text regions, speech bubbles and
    /// segmentation masks. One implementation ships today.
    #[arg(
        long,
        value_enum,
        default_value = "koharu-layout-rfdetr-seg-2xl",
        help_heading = "Pipeline"
    )]
    pub detection: DetectionChoice,

    /// OCR model reading the source text from the detected regions
    /// (paddleocr-vl-1.6 is the default and handles mixed-script pages).
    #[arg(
        long,
        value_enum,
        default_value = "paddleocr-vl-1.6",
        help_heading = "Pipeline"
    )]
    pub ocr: OcrChoice,

    /// Inpainting model rebuilding the image behind the source text before
    /// the translation is rendered (lama is the fast default).
    #[arg(long, value_enum, default_value = "lama", help_heading = "Pipeline")]
    pub inpainting: InpaintingChoice,

    /// Sampling steps requested from stable-diffusion.cpp for Qwen Image
    /// inpainting (14 samples 12 at the default strength). Only used with
    /// --inpainting qwen-image.
    #[arg(
        long,
        value_name = "N",
        value_parser = clap::value_parser!(u32).range(1..),
        help_heading = "Pipeline"
    )]
    pub qwen_steps: Option<u32>,

    /// Extra instructions passed to the translator.
    #[arg(long, help_heading = "Pipeline")]
    pub translation_instructions: Option<String>,

    /// Re-translate pages whose output already exists.
    #[arg(long, help_heading = "Run control")]
    pub overwrite: bool,

    /// Extra attempts granted to a page whose stage fails, before it is
    /// dropped from the run (0 keeps the first failure).
    #[arg(long, default_value_t = 1, help_heading = "Run control")]
    pub retries: usize,

    /// Plan the run (pages, model, VRAM) without executing it.
    #[arg(long, help_heading = "Run control")]
    pub dry_run: bool,

    /// Report failures and the final summary only, without per-page progress.
    #[arg(long, help_heading = "Run control")]
    pub quiet: bool,

    /// Translate only these pages, in 1-based reading order (`1-10,15`).
    #[arg(long, value_name = "RANGE", help_heading = "Run control")]
    pub pages: Option<String>,

    /// Also take the pages held in subdirectories of the input folder.
    #[arg(long, help_heading = "Run control")]
    pub recursive: bool,

    /// Reproducible output: translate greedily (temperature 0) so rerunning
    /// the same chapter renders the same text on every run.
    #[arg(long, help_heading = "Reproducibility")]
    pub deterministic: bool,

    /// Reproducible GPU session: run the Torch models — layout detection and
    /// inpainting — in fp32 instead of bfloat16, removing the nondeterministic
    /// bf16 split-K reductions that are the only drift known to change OCR
    /// text. Roughly doubles the VRAM and compute of those two stages, and has
    /// no effect on the ggml-based OCR and translation models, which are
    /// already bit-stable, nor on CPU runs.
    #[arg(long, help_heading = "Reproducibility")]
    pub torch_fp32: bool,

    /// Store OCR text at PATH and reuse it for identical crops on later runs,
    /// so the translation stage reads the same text every time (OCR inference
    /// is not bit-stable on the GPU). Defaults to `.ocr-cache.json` inside
    /// --input, or beside it when --input is a .cbz/.zip archive.
    #[arg(
        long,
        value_name = "PATH",
        conflicts_with = "no_ocr_cache",
        help_heading = "Reproducibility"
    )]
    pub ocr_cache: Option<PathBuf>,

    /// Re-run OCR on every page without reading or writing an OCR cache.
    #[arg(long, help_heading = "Reproducibility")]
    pub no_ocr_cache: bool,

    /// Output image format when writing a folder.
    #[arg(long, value_enum, default_value = "png")]
    pub format: FormatChoice,

    /// Run without reading or writing the calibration file: built-in
    /// estimates only, nothing persisted.
    #[arg(long, help_heading = "Model & VRAM")]
    pub no_calibration: bool,

    /// Force CPU execution.
    #[arg(long, help_heading = "Model & VRAM")]
    pub cpu: bool,

    /// Run the pipeline again in the same process and report every page whose
    /// OCR text or translation differs between passes: the run-to-run drift
    /// the local models can produce, page by page (implies --overwrite
    /// semantics for its own outputs; the report lists unstable pages and the
    /// run exits 0 even when some drift — the signal is the report itself).
    #[arg(long, help_heading = "Reproducibility")]
    pub verify: bool,

    /// Total number of passes `--verify` runs (production pass included).
    /// 2 diffs each replay against the production pass; N >= 3 replays the
    /// pipeline N-1 times and votes per page: a page whose readings are all
    /// identical is stable, otherwise the report gives the frequency of each
    /// distinct reading as an estimate of that page's drift probability.
    /// Values below 2 are treated as 2.
    #[arg(
        long,
        value_name = "N",
        default_value_t = 2,
        help_heading = "Reproducibility"
    )]
    pub verify_passes: usize,
}

/// The standalone actions: housekeeping on the store and on the calibration
/// file. None of them translates a page — no `--input`, no `--output`, no
/// model to resolve — which is why they are subcommands instead of flags on
/// the run, and why clap refuses to mix them with a run's flags.
#[derive(Clone, Debug, Subcommand)]
pub enum Command {
    /// List the local model catalog: VRAM estimates, download sizes, and what
    /// previous runs measured on this machine.
    Models,

    /// List the store entries nothing in the code references anymore (models,
    /// datasets and runtimes left behind by removed catalog entries, probes
    /// or updates), each with its size. Nothing is deleted without `--delete`.
    Prune {
        /// Delete the listed entries instead of stopping at the listing.
        /// Nothing is lost permanently: every entry re-downloads automatically
        /// (size and SHA-256 verified) if a model needs it again.
        #[arg(long)]
        delete: bool,
    },

    /// Delete the VRAM calibration file; later runs fall back to the
    /// built-in reference estimates until new measurements are recorded.
    ResetCalibration,
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
    #[value(name = "qwen-image")]
    QwenImage,
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

/// A concrete translation choice: the backend that translates, what that
/// backend needs from the flags, and what the choice implies locally.
#[derive(Debug)]
pub struct Resolved {
    /// The catalog (local) or the hosted provider doing the translation.
    pub provider: Provider,
    /// Local: the catalog id. Hosted: the model id, or `None` for the
    /// services that take none.
    pub model: Option<String>,
    /// Local: the quantization id. Hosted: `None` — hosted weights are
    /// never quantized on this card.
    pub quantization: Option<String>,
    /// Footprint on this card: the local model's estimate, `0` bytes for a
    /// hosted model whose weights live on the provider's GPUs.
    pub estimate: preset::VramEstimate,
    /// Bytes the configuration needs in the store (weights and projector).
    pub download: u64,
    /// Whether the page image is fed to the model: a `--no-vision` run neither
    /// downloads nor loads the projector and translates from OCR text alone.
    pub vision: bool,
    /// Whether the model emits reasoning traces the backend must strip.
    pub reasoning: bool,
}

impl Resolved {
    /// Model name for reports and progress lines: the hosted service's own
    /// name when it takes no model id (`DeepL`), the id otherwise — a local
    /// run always has one.
    #[must_use]
    pub fn model_label(&self) -> String {
        self.model
            .clone()
            .unwrap_or_else(|| self.provider.name().to_owned())
    }

    /// Variant shown beside the model name: the quantization locally, the
    /// provider id for a hosted run, where the backend is the variant that
    /// matters.
    #[must_use]
    pub fn variant_label(&self) -> String {
        self.quantization.clone().unwrap_or_else(|| {
            let id: &'static str = self.provider.into();
            id.to_owned()
        })
    }

    /// VRAM estimate for reports: `None` for a hosted run — only this run's
    /// local vision stages touch the card, which the measured peak reports.
    #[must_use]
    pub fn estimate_label(&self) -> Option<String> {
        (self.provider == Provider::Local).then(|| self.estimate.display())
    }
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

/// Store root used when `--store` is not given: `KOHARU_STORE` when set to an
/// absolute path, else the `store` directory next to this executable when it
/// exists (a Koharu app installation), otherwise the operating-system cache
/// default.
#[must_use]
pub fn default_store_root() -> PathBuf {
    if let Some(directory) = std::env::var_os("KOHARU_STORE").filter(|value| !value.is_empty()) {
        return PathBuf::from(directory);
    }
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

/// Resolves the translation choice: a hosted provider as configured (no
/// budget, no download — its weights live elsewhere), or the local LLM —
/// `auto` scans the preference list and refuses a pick below the quality
/// floor unless `--force`, an explicit id is checked against the VRAM
/// budget unless `--force` was passed. Both local paths use the real
/// measurements recorded by previous runs when available.
pub fn resolve_model(
    arguments: &Arguments,
    budget: Option<u64>,
    measurements: &MeasuredPeaks,
) -> Result<Resolved> {
    if arguments.provider != Provider::Local {
        return resolve_hosted(arguments);
    }
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
        if let Some(parameters) = preset::parameters_billion(choice.model)
            && parameters < preset::AUTO_QUALITY_FLOOR_B
        {
            if !arguments.force {
                bail!(
                    "the strongest model fitting the VRAM budget {} is {} {} ({parameters}B), \
                     below the {}B quality floor for usable translations: free VRAM, pass an \
                     explicit --llm <id> (--force skips its budget check), translate on CPU \
                     (--cpu with --llm), or --force to accept the small automatic pick",
                    gib(budget),
                    choice.model,
                    choice.quantization,
                    preset::AUTO_QUALITY_FLOOR_B,
                );
            }
            eprintln!(
                "warning: --force accepts the small automatic pick {} {} ({parameters}B); \
                 expect repetitive translations until the GPU fits a larger model",
                choice.model, choice.quantization,
            );
        }
        return Ok(Resolved {
            provider: Provider::Local,
            model: Some(choice.model.to_owned()),
            quantization: Some(choice.quantization.to_owned()),
            estimate: choice.estimate,
            download: download_bytes(choice.model, choice.quantization, vision)?,
            vision,
            reasoning: preset::descriptor_for(choice.model).is_some_and(preset::is_reasoning),
        });
    }

    let Some(descriptor) = preset::descriptor_for(&arguments.llm) else {
        bail!(
            "unknown local model '{}' (use koharu-batch models to see the catalog)",
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
        provider: Provider::Local,
        model: Some(arguments.llm.clone()),
        quantization: Some(quantization),
        estimate,
        download,
        vision,
        reasoning: preset::is_reasoning(descriptor),
    })
}

/// Resolves a hosted choice: `--llm auto` means the service without a model
/// id — which only the model-less providers accept — and the local-only
/// flags that would silently do nothing are refused instead of ignored.
fn resolve_hosted(arguments: &Arguments) -> Result<Resolved> {
    if let Some(quantization) = &arguments.quantization {
        bail!(
            "--quantization {quantization} picks a local model's file format; {0} is hosted (drop it, or pass --provider local)",
            arguments.provider
        );
    }
    let model = match arguments.llm.as_str() {
        "auto" if arguments.provider.takes_model() => bail!(
            "--provider {0} requires --llm <model-id> (`auto` picks local models only)",
            arguments.provider
        ),
        "auto" => None,
        model => Some(model.to_owned()),
    };
    Ok(Resolved {
        provider: arguments.provider,
        model,
        quantization: None,
        estimate: preset::VramEstimate {
            bytes: 0,
            measured: false,
        },
        download: 0,
        vision: !arguments.no_vision,
        // No thinking configuration is ever sent to a hosted endpoint: its
        // own default applies (a local model derives this from the catalog,
        // where a wrong guess would be an API rejection).
        reasoning: false,
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
        ocr_cache: ocr_cache_path(arguments),
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
                provider: resolved.provider,
                model: resolved.model.clone(),
                quantization: resolved.quantization.clone(),
                vision: resolved.vision,
                reasoning: resolved.reasoning,
            },
            // `vision` on the generation gates the image both in the stage
            // (attaching the page crop) and in the translator itself.
            generation: GenerationConfig {
                vision: Some(resolved.vision),
                temperature: arguments.deterministic.then_some(0.0),
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
            InpaintingChoice::QwenImage => InpaintingModel::QwenImage(QwenImageConfig {
                num_inference_steps: arguments.qwen_steps,
                ..QwenImageConfig::default()
            }),
            InpaintingChoice::RoremMixed => {
                InpaintingModel::RoremMixed(RoremMixedConfig::default())
            }
        },
        processor: Default::default(),
    }
}

/// Where `--ocr-cache` persists OCR text: an explicit `--ocr-cache <PATH>`,
/// `.ocr-cache.json` inside an input directory (beside an input archive), or
/// `None` with `--no-ocr-cache`.
pub fn ocr_cache_path(arguments: &Arguments) -> Option<PathBuf> {
    if arguments.no_ocr_cache {
        return None;
    }
    if let Some(path) = &arguments.ocr_cache {
        return Some(path.clone());
    }
    let input = arguments.input.as_deref()?;
    let archive = input
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cbz") || extension.eq_ignore_ascii_case("zip")
        });
    Some(if archive {
        let mut path = input.as_os_str().to_owned();
        path.push(".ocr-cache.json");
        PathBuf::from(path)
    } else {
        input.join(".ocr-cache.json")
    })
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
    fn deterministic_translates_greedily() {
        let resolved = Resolved {
            provider: Provider::Local,
            model: Some("gemma4-e2b-it".to_owned()),
            quantization: Some("Q4_K_XL".to_owned()),
            estimate: preset::VramEstimate {
                bytes: 0,
                measured: false,
            },
            download: 0,
            vision: true,
            reasoning: false,
        };

        let plain = Arguments::parse_from(["koharu-batch", "--input", "in", "--output", "out"]);
        assert_eq!(
            pipeline_config(&plain, &resolved)
                .translation
                .generation
                .temperature,
            None
        );

        let det = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--deterministic",
        ]);
        assert_eq!(
            pipeline_config(&det, &resolved)
                .translation
                .generation
                .temperature,
            Some(0.0)
        );
    }

    #[test]
    fn a_hosted_provider_translates_without_local_budget() {
        let arguments = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--provider",
            "openai",
            "--llm",
            "gpt-5.6-luna",
            "--no-vision",
        ]);
        let resolved = resolve_model(&arguments, None, &MeasuredPeaks::new())
            .expect("a hosted run needs no budget or download");

        assert_eq!(resolved.provider, Provider::OpenAi);
        assert_eq!(resolved.model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(resolved.quantization, None);
        assert_eq!(resolved.download, 0);
        assert_eq!(resolved.estimate.bytes, 0);
        assert!(!resolved.vision, "--no-vision travels to the hosted pick");
        assert_eq!(resolved.estimate_label(), None);
        assert_eq!(resolved.variant_label(), "openai");

        let config = pipeline_config(&arguments, &resolved);
        assert_eq!(config.translation.model.provider, Provider::OpenAi);
        assert_eq!(
            config.translation.model.model.as_deref(),
            Some("gpt-5.6-luna")
        );
        assert_eq!(config.translation.model.quantization, None);
        assert!(!config.translation.model.vision);
    }

    #[test]
    fn model_less_services_run_without_a_model_and_chat_providers_do_not() {
        let deepl = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--provider",
            "deepl",
            // What the app sends for a service pick: its models are text-only.
            "--no-vision",
        ]);
        let resolved = resolve_model(&deepl, None, &MeasuredPeaks::new())
            .expect("DeepL translates as itself, no model id");
        assert_eq!(resolved.provider, Provider::DeepL);
        assert_eq!(resolved.model, None);
        assert_eq!(resolved.model_label(), "DeepL");
        assert!(!resolved.vision, "the service never sees the page image");

        let openai = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--provider",
            "openai",
        ]);
        let error = resolve_model(&openai, None, &MeasuredPeaks::new())
            .expect_err("a chat provider cannot translate without a model");
        assert!(error.to_string().contains("--llm <model-id>"), "{error:#}");
    }

    #[test]
    fn hosted_runs_refuse_local_only_flags_and_unknown_providers() {
        let quantized = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--provider",
            "openai",
            "--llm",
            "gpt-5.6-luna",
            "--quantization",
            "Q4_K_XL",
        ]);
        assert!(
            resolve_model(&quantized, None, &MeasuredPeaks::new()).is_err(),
            "a local-only flag must fail instead of silently doing nothing"
        );

        assert!(
            Arguments::try_parse_from([
                "koharu-batch",
                "--input",
                "in",
                "--output",
                "out",
                "--provider",
                "not-a-provider",
            ])
            .is_err(),
            "an unknown provider id is a usage error (exit code 2)"
        );
    }

    #[test]
    fn auto_refuses_a_pick_below_the_quality_floor_unless_forced() {
        let arguments = Arguments::parse_from(["koharu-batch", "--input", "in", "--output", "out"]);
        // 2.5 GiB sits above the 0.8B tail pick (≈2.3 GiB) and below every
        // 2B configuration (≈3.0 GiB), so only the sub-floor model fits.
        let crowded = 2 * 1024 * 1024 * 1024 + 512 * 1024 * 1024;
        let error = resolve_model(&arguments, Some(crowded), &MeasuredPeaks::new())
            .expect_err("an automatic pick below the floor refuses without --force");
        let message = error.to_string();
        assert!(message.contains("quality floor"), "{message}");
        assert!(message.contains("qwen3.5-0.8b"), "{message}");
        assert!(message.contains("--force"), "{message}");

        let forced = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--force",
        ]);
        let resolved = resolve_model(&forced, Some(crowded), &MeasuredPeaks::new())
            .expect("--force accepts the small automatic pick");
        assert_eq!(resolved.model.as_deref(), Some("qwen3.5-0.8b"));

        let resolved = resolve_model(
            &arguments,
            Some(6 * 1024 * 1024 * 1024),
            &MeasuredPeaks::new(),
        )
        .expect("6 GiB fits a model above the floor");
        assert_eq!(resolved.model.as_deref(), Some("gemma4-e4b-it"));
    }

    #[test]
    fn torch_fp32_is_opt_in() {
        let plain = Arguments::parse_from(["koharu-batch", "--input", "in", "--output", "out"]);
        assert!(!plain.torch_fp32);

        let forced = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--torch-fp32",
        ]);
        assert!(forced.torch_fp32);
    }

    #[test]
    fn qwen_steps_is_opt_in_and_parsed_as_a_step_count() {
        let plain = Arguments::parse_from(["koharu-batch", "--input", "in", "--output", "out"]);
        assert_eq!(plain.qwen_steps, None);

        let stepped = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--inpainting",
            "qwen-image",
            "--qwen-steps",
            "20",
        ]);
        assert_eq!(stepped.qwen_steps, Some(20));

        assert!(
            Arguments::try_parse_from([
                "koharu-batch",
                "--input",
                "in",
                "--output",
                "out",
                "--qwen-steps",
                "0",
            ])
            .is_err()
        );
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
        assert_eq!(arguments.provider, Provider::Local);
        assert_eq!(arguments.llm, "auto");
        assert_eq!(arguments.retries, 1);
        assert_eq!(arguments.format.extension(), "png");
        assert!(!arguments.no_vision);
        assert!(!arguments.no_calibration);
        assert!(
            arguments.command.is_none(),
            "no subcommand means the default flow: translate"
        );
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

    #[test]
    fn qwen_image_is_a_selectable_inpainting_processor() {
        let arguments = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--inpainting",
            "qwen-image",
        ]);

        assert!(matches!(arguments.inpainting, InpaintingChoice::QwenImage));
    }

    #[test]
    fn the_ocr_cache_defaults_inside_the_input_directory() {
        let arguments = Arguments::parse_from(["koharu-batch", "--input", "in", "--output", "out"]);

        assert_eq!(
            ocr_cache_path(&arguments),
            Some(PathBuf::from("in").join(".ocr-cache.json"))
        );
    }

    #[test]
    fn the_ocr_cache_lands_beside_an_input_archive() {
        let arguments =
            Arguments::parse_from(["koharu-batch", "--input", "volume.cbz", "--output", "out"]);

        assert_eq!(
            ocr_cache_path(&arguments),
            Some(PathBuf::from("volume.cbz.ocr-cache.json"))
        );
    }

    #[test]
    fn an_explicit_ocr_cache_path_wins_and_no_ocr_cache_disables_it() {
        let explicit = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--ocr-cache",
            "elsewhere.json",
        ]);
        assert_eq!(
            ocr_cache_path(&explicit),
            Some(PathBuf::from("elsewhere.json"))
        );

        let disabled = Arguments::parse_from([
            "koharu-batch",
            "--input",
            "in",
            "--output",
            "out",
            "--no-ocr-cache",
        ]);
        assert_eq!(ocr_cache_path(&disabled), None);

        assert!(
            Arguments::try_parse_from([
                "koharu-batch",
                "--input",
                "in",
                "--output",
                "out",
                "--ocr-cache",
                "cache.json",
                "--no-ocr-cache",
            ])
            .is_err(),
            "the two cache flags must conflict"
        );
    }

    #[test]
    fn the_one_shot_actions_are_subcommands() {
        assert!(matches!(
            Arguments::parse_from(["koharu-batch", "models"]).command,
            Some(Command::Models)
        ));

        // `--store` is global: it follows the subcommand name, the CI way.
        let listed = Arguments::parse_from(["koharu-batch", "prune", "--store", "somewhere"]);
        assert!(matches!(
            listed.command,
            Some(Command::Prune { delete: false })
        ));
        assert_eq!(
            listed.store.as_deref(),
            Some(std::path::Path::new("somewhere"))
        );

        let deleted = Arguments::parse_from(["koharu-batch", "prune", "--delete"]);
        assert!(matches!(
            deleted.command,
            Some(Command::Prune { delete: true })
        ));

        assert!(matches!(
            Arguments::parse_from(["koharu-batch", "reset-calibration"]).command,
            Some(Command::ResetCalibration)
        ));
    }

    #[test]
    fn a_run_and_an_action_never_share_a_command_line() {
        // On either side of the name, a run's flag next to a subcommand would
        // be silently ignored — or the action taken despite the run's flags —
        // so clap must refuse every combination instead of guessing.
        for invocation in [
            &["--input", "in", "--output", "out", "models"][..],
            &["--store", "somewhere", "prune"][..],
            &["prune", "--delete", "--overwrite"][..],
            &["models", "--no-calibration"][..],
            &["reset-calibration", "--dry-run"][..],
        ] {
            let mut argv = vec!["koharu-batch"];
            argv.extend_from_slice(invocation);
            assert!(
                Arguments::try_parse_from(&argv).is_err(),
                "{invocation:?} must be refused"
            );
        }
    }

    #[test]
    fn the_removed_one_shot_flags_stay_removed() {
        // No aliases, no backward compatibility (AGENTS.md): the migration is
        // forced, not suggested, so every pre-subcommand spelling must keep
        // failing instead of creeping back as a hidden shortcut.
        for invocation in [
            &["--list-models"][..],
            &["--prune"][..],
            &["--prune", "--prune-delete"][..],
            &["--reset-calibration"][..],
        ] {
            let mut argv = vec!["koharu-batch"];
            argv.extend_from_slice(invocation);
            assert!(
                Arguments::try_parse_from(&argv).is_err(),
                "{invocation:?} must stay refused"
            );
        }
    }
}
