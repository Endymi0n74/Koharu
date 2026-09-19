//! VRAM budgeting for local translation models.
//!
//! Peaks measured on the fork's reference machine (Ryzen 7600 / RTX 3070 8 GB,
//! CUDA, 8-segment Japanese→French prompt under the constrained JSON schema)
//! are recorded in [`MEASURED_PEAKS`]; everything else is estimated from the
//! parameter count and quantization. Estimates trade precision for coverage:
//! the goal is refusing models that cannot fit, not predicting exact peaks.

use super::catalog::LocalModelDescriptor;
use super::catalog::MODELS;
use crate::QuantizationDefinition;

const GIB: u64 = 1024 * 1024 * 1024;

/// Context and compute buffers observed around the measured weights footprint.
const CONTEXT_OVERHEAD_BYTES: u64 = GIB;
/// Vision projector footprint (mtmd) added on top of the text-only peak.
const VISION_PROJECTOR_BYTES: u64 = (0.9 * GIB as f64) as u64;
/// Fraction of the physical VRAM usable before Windows/compositor overhead
/// makes llama.cpp thrash.
const BUDGET_MARGIN: f64 = 0.9;

/// No-vision peaks of the fork's reference machine, in mebibytes, keyed by
/// (model id, quantization id).
///
/// The underlying runs were made *with* the vision projector and are recorded
/// in `~/.koharu/vram-calibration.toml`; each entry here is that measurement
/// minus [`VISION_PROJECTOR_BYTES`], so the estimator reproduces the raw
/// measurement once it adds the projector back for a vision configuration.
/// See `docs/fork.md` ("Choosing a translation model for 8 GB of VRAM").
const MEASURED_PEAKS: &[(&str, &str, u64)] = &[
    ("gemma4-e2b-it", "Q4_K_XL", 3100),
    ("gemma4-e4b-uncensored", "Q4_K_P", 5528),
    ("gemma4-e4b-it", "Q4_K_XL", 3446),
    ("ministral-3-8b-instruct", "Q4_K_M", 6600),
    ("gemma4-12b-it", "Q4_K_XL", 7700),
];

/// Approximate GGUF bytes per parameter for each quantization family.
fn bytes_per_parameter(quantization: &str) -> f64 {
    let id = quantization.to_ascii_uppercase();
    if id.starts_with("BF16") || id.starts_with("F16") {
        2.0
    } else if id.starts_with("Q8") {
        0.95
    } else if id.starts_with("Q6") {
        0.75
    } else if id.starts_with("Q5") {
        0.68
    } else if id.starts_with("Q4") || id.starts_with("IQ4") {
        0.57
    } else if id.starts_with("Q3") || id.starts_with("IQ3") {
        0.42
    } else {
        0.32
    }
}

/// Extracts the largest `…b` parameter marker from a model id
/// (`gemma4-e2b-it` → 2, `qwen3.5-2b` → 2, `gemma4-26b-a4b-it` → 26).
fn parameters_billion(id: &str) -> Option<f64> {
    id.split(['-', '_', '/'])
        .filter_map(|segment| {
            let marker: String = segment
                .chars()
                .skip_while(|character| !character.is_ascii_digit() && *character != '.')
                .collect();
            let digits = marker.strip_suffix(['b', 'B'])?;
            digits.parse::<f64>().ok()
        })
        .fold(None, |largest: Option<f64>, value| {
            largest.map_or(Some(value), |largest| Some(largest.max(value)))
        })
}

/// Estimated VRAM footprint of one model configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramEstimate {
    pub bytes: u64,
    /// Whether the peak was measured on the reference machine rather than estimated.
    pub measured: bool,
}

impl VramEstimate {
    /// Human-readable footprint, e.g. `4.8 GiB (measured)`.
    #[must_use]
    pub fn display(&self) -> String {
        format!(
            "{:.1} GiB{}",
            self.bytes as f64 / GIB as f64,
            if self.measured { " (measured)" } else { "" }
        )
    }
}

fn measured_peak(descriptor: &LocalModelDescriptor, quantization: &str) -> Option<u64> {
    MEASURED_PEAKS
        .iter()
        .find(|&&(model, quant, _)| model == descriptor.id && quant == quantization)
        .map(|&(_, _, mebibytes)| mebibytes * 1024 * 1024)
}

/// Peak VRAM a run actually used, reported back by the CLI and persisted
/// across runs so estimates stay calibrated on the machine that runs them.
///
/// One sample is the peak bytes in use above the GPU baseline while a full
/// pipeline ran a model/quantization (with or without the vision projector).
/// It therefore covers the whole chapter pipeline (detection, OCR, inpainting,
/// and the LLM), which is the footprint a budget must actually hold.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MeasuredPeak {
    pub model: String,
    pub quantization: String,
    pub vision: bool,
    /// Peak bytes above the GPU baseline (desktop usage excluded).
    pub bytes: u64,
}

impl MeasuredPeak {
    /// Key of a calibration entry.
    fn key(&self) -> (&str, &str, bool) {
        (&self.model, &self.quantization, self.vision)
    }
}

/// Real measurements collected from previous `koharu-batch` runs, keyed by
/// `(model, quantization, vision)`; a real sample always wins over the
/// built-in reference table and the formula estimate.
#[derive(Clone, Debug, Default)]
pub struct MeasuredPeaks {
    entries: Vec<MeasuredPeak>,
}

impl MeasuredPeaks {
    /// An empty table (only built-in and formula estimates apply).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records (or raises) the peak observed for one configuration.
    pub fn record(&mut self, peak: MeasuredPeak) {
        match self
            .entries
            .iter_mut()
            .find(|entry| entry.key() == peak.key())
        {
            Some(entry) => entry.bytes = entry.bytes.max(peak.bytes),
            None => self.entries.push(peak),
        }
    }

    /// Merges another table, keeping the highest observation per key.
    pub fn merge(&mut self, other: &MeasuredPeaks) {
        for peak in &other.entries {
            self.record(peak.clone());
        }
    }

    /// Whether any real measurement was loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of recorded configurations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Peak bytes observed for a configuration, if any.
    #[must_use]
    pub fn peak_bytes(&self, model: &str, quantization: &str, vision: bool) -> Option<u64> {
        self.entries
            .iter()
            .find(|entry| {
                entry.model == model && entry.quantization == quantization && entry.vision == vision
            })
            .map(|entry| entry.bytes)
    }

    /// Iterates over every recorded configuration.
    pub fn iter(&self) -> impl Iterator<Item = &MeasuredPeak> {
        self.entries.iter()
    }
}

/// Estimates the VRAM peak of `descriptor` at `quantization`, including the
/// vision projector when `vision` is requested and the model supports it.
#[must_use]
pub fn estimate_vram(
    descriptor: &LocalModelDescriptor,
    quantization: Option<&str>,
    vision: bool,
) -> VramEstimate {
    estimate_vram_with(descriptor, quantization, vision, &MeasuredPeaks::new())
}

/// [`estimate_vram`] against a table of real measurements: a recorded peak
/// for the exact configuration wins over the built-in reference table.
#[must_use]
pub fn estimate_vram_with(
    descriptor: &LocalModelDescriptor,
    quantization: Option<&str>,
    vision: bool,
    measurements: &MeasuredPeaks,
) -> VramEstimate {
    let quantization = quantization.or_else(|| {
        descriptor
            .quantizations
            .first()
            .map(|definition| definition.id)
    });
    if let Some(quantization) = quantization
        && let Some(bytes) = measurements.peak_bytes(descriptor.id, quantization, vision)
    {
        return VramEstimate {
            bytes,
            measured: true,
        };
    }
    let base = quantization.and_then(|quantization| measured_peak(descriptor, quantization));
    if let Some(base) = base {
        return VramEstimate {
            bytes: base + if vision { VISION_PROJECTOR_BYTES } else { 0 },
            measured: true,
        };
    }

    VramEstimate {
        bytes: weights_bytes(descriptor, quantization)
            + CONTEXT_OVERHEAD_BYTES
            + projector_bytes(descriptor, vision),
        measured: false,
    }
}

/// Approximate bytes of the model files a configuration has to download: the
/// GGUF weights plus the vision projector when one is needed.
///
/// A configuration that was measured on this machine derives its weight size
/// from that peak, which tracks the real repository better than the parameter
/// formula does; everything else falls back to the parameter count. The result
/// is a planning figure, not the exact size reported by the repository.
#[must_use]
pub fn download_bytes(
    descriptor: &LocalModelDescriptor,
    quantization: Option<&str>,
    vision: bool,
) -> u64 {
    weights_bytes(descriptor, quantization) + projector_bytes(descriptor, vision)
}

/// Model weights in bytes: a measured peak minus the runtime overhead, or the
/// parameter count times the quantization's bytes per parameter.
fn weights_bytes(descriptor: &LocalModelDescriptor, quantization: Option<&str>) -> u64 {
    if let Some(measured) =
        quantization.and_then(|quantization| measured_peak(descriptor, quantization))
    {
        return measured.saturating_sub(CONTEXT_OVERHEAD_BYTES);
    }
    let parameters = parameters_billion(descriptor.id).unwrap_or(8.0);
    let bytes_per_parameter = quantization.map_or(0.57, bytes_per_parameter);
    (parameters * 1_000_000_000.0 * bytes_per_parameter) as u64
}

fn projector_bytes(descriptor: &LocalModelDescriptor, vision: bool) -> u64 {
    u64::from(vision && descriptor.projector.is_some()) * VISION_PROJECTOR_BYTES
}

/// A local model configuration that fits a VRAM budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoChoice {
    pub model: &'static str,
    pub quantization: &'static str,
    pub estimate: VramEstimate,
}

/// Preference order for `--llm auto`: the strongest model that fits the budget
/// wins, so a large card is not held back by the 8 GB recommendation. Within
/// one size the uncensored variant comes first (manga dialogue is the target
/// workload), then the instruct 4B, the fastest dense 8B, and the small E2B as
/// the fallbacks for an 8 GB card and below. Models not listed here stay
/// reachable through an explicit `--llm <id> --quantization <id>`.
const AUTO_PRIORITY: &[(&str, &str)] = &[
    ("gemma4-31b-uncensored", "Q4_K_M"),
    ("gemma4-26b-a4b-uncensored", "Q4_K_M"),
    ("gemma4-12b-uncensored", "Q4_K_M"),
    ("gemma4-e4b-uncensored", "Q4_K_P"),
    ("gemma4-e4b-it", "Q4_K_XL"),
    ("ministral-3-8b-instruct", "Q4_K_M"),
    ("gemma4-e2b-it", "Q4_K_XL"),
];

/// Selects the best local model for `budget` (usable bytes, already margin-trimmed).
///
/// `vision` skips models without a projector. Returns `None` when nothing fits.
#[must_use]
pub fn resolve_auto(budget: u64, vision: bool) -> Option<AutoChoice> {
    resolve_auto_with(budget, vision, &MeasuredPeaks::new())
}

/// [`resolve_auto`] against a table of real measurements, so a model that
/// measured over the budget on this machine is skipped in favor of the next
/// candidate that did fit.
#[must_use]
pub fn resolve_auto_with(
    budget: u64,
    vision: bool,
    measurements: &MeasuredPeaks,
) -> Option<AutoChoice> {
    AUTO_PRIORITY
        .iter()
        .filter_map(|&(model, quantization)| {
            let descriptor = MODELS.iter().find(|descriptor| descriptor.id == model)?;
            if vision && descriptor.projector.is_none() {
                return None;
            }
            if !descriptor
                .quantizations
                .iter()
                .any(|definition| definition.id == quantization)
            {
                return None;
            }
            let estimate = estimate_vram_with(descriptor, Some(quantization), vision, measurements);
            Some(AutoChoice {
                model,
                quantization,
                estimate,
            })
        })
        .find(|choice| choice.estimate.bytes <= budget)
}

/// Physical VRAM of the reference machine, trimmed by [`BUDGET_MARGIN`].
#[must_use]
pub fn budget_from_total(total_bytes: u64) -> u64 {
    (total_bytes as f64 * BUDGET_MARGIN) as u64
}

/// Looks up a catalog descriptor by model id.
#[must_use]
pub fn descriptor_for(model: &str) -> Option<&'static LocalModelDescriptor> {
    MODELS.iter().find(|descriptor| descriptor.id == model)
}

/// Whether the model ships with a vision projector (page-image input).
#[must_use]
pub fn supports_vision(descriptor: &LocalModelDescriptor) -> bool {
    descriptor.projector.is_some()
}

/// Default quantization id of a model (first catalog entry).
#[must_use]
pub fn default_quantization(descriptor: &LocalModelDescriptor) -> &'static str {
    descriptor.quantizations[0].id
}

/// Whether `quantization` exists for `descriptor`.
#[must_use]
pub fn has_quantization(descriptor: &LocalModelDescriptor, quantization: &str) -> bool {
    descriptor
        .quantizations
        .iter()
        .any(|definition| definition.id == quantization)
}

/// Failure detail for a model that does not fit: the estimate plus every
/// alternative that would fit the same budget.
#[derive(Clone, Debug)]
pub struct BudgetExceeded {
    pub estimate: VramEstimate,
    pub budget: u64,
    pub alternatives: Vec<String>,
}

impl std::fmt::Display for BudgetExceeded {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "estimated {} exceeds the VRAM budget {}",
            self.estimate.display(),
            bytes_as_gib(self.budget),
        )?;
        if self.alternatives.is_empty() {
            write!(
                formatter,
                "; no local model fits (lower the quantization or use --force)"
            )
        } else {
            write!(
                formatter,
                "; models that fit: {}",
                self.alternatives.join(", ")
            )
        }
    }
}

fn bytes_as_gib(bytes: u64) -> String {
    format!("{:.1} GiB", bytes as f64 / GIB as f64)
}

/// Why an explicit model selection was refused by the budget check.
#[derive(Clone, Debug)]
pub enum BudgetCheck {
    /// The model id is not in the local catalog.
    UnknownModel,
    /// The model exists but its estimated footprint exceeds the budget.
    Exceeded(BudgetExceeded),
}

/// Checks an explicit local model selection against a VRAM budget.
///
/// Returns the estimate, or why the selection was refused.
pub fn check_budget(
    budget: u64,
    model: &str,
    quantization: Option<&str>,
    vision: bool,
) -> Result<VramEstimate, BudgetCheck> {
    check_budget_with(budget, model, quantization, vision, &MeasuredPeaks::new())
}

/// [`check_budget`] against a table of real measurements: the model is
/// refused when its *measured* footprint exceeds the budget, and the listed
/// alternatives are the ones that actually fit on this machine.
pub fn check_budget_with(
    budget: u64,
    model: &str,
    quantization: Option<&str>,
    vision: bool,
    measurements: &MeasuredPeaks,
) -> Result<VramEstimate, BudgetCheck> {
    let Some(descriptor) = MODELS.iter().find(|descriptor| descriptor.id == model) else {
        return Err(BudgetCheck::UnknownModel);
    };
    let estimate = estimate_vram_with(descriptor, quantization, vision, measurements);
    if estimate.bytes <= budget {
        return Ok(estimate);
    }

    let vision_capable = vision && descriptor.projector.is_some();
    let alternatives = MODELS
        .iter()
        .filter(|candidate| !vision_capable || candidate.projector.is_some())
        .filter_map(|candidate| {
            let definition = candidate
                .quantizations
                .iter()
                .filter(|definition| {
                    estimate_vram_with(candidate, Some(definition.id), vision, measurements).bytes
                        <= budget
                })
                .max_by_key(|definition| {
                    estimate_vram_with(candidate, Some(definition.id), vision, measurements).bytes
                })?;
            let estimate = estimate_vram_with(candidate, Some(definition.id), vision, measurements);
            Some(format!(
                "{} {} ({})",
                candidate.id,
                definition.id,
                estimate.display()
            ))
        })
        .collect();
    Err(BudgetCheck::Exceeded(BudgetExceeded {
        estimate,
        budget,
        alternatives,
    }))
}

/// One catalog entry with per-quantization VRAM estimates, for `--list-models`.
#[derive(Clone, Debug)]
pub struct ModelVram {
    pub id: String,
    pub name: String,
    pub vision: bool,
    pub quantizations: Vec<QuantizationVram>,
}

#[derive(Clone, Debug)]
pub struct QuantizationVram {
    pub id: String,
    pub estimate: VramEstimate,
}

/// VRAM estimates for every local model in the catalog.
#[must_use]
pub fn vram_catalog(vision: bool) -> Vec<ModelVram> {
    vram_catalog_with(vision, &MeasuredPeaks::new())
}

/// [`vram_catalog`] against a table of real measurements.
#[must_use]
pub fn vram_catalog_with(vision: bool, measurements: &MeasuredPeaks) -> Vec<ModelVram> {
    MODELS
        .iter()
        .map(|descriptor| ModelVram {
            id: descriptor.id.to_owned(),
            name: descriptor.name.to_owned(),
            vision: descriptor.projector.is_some(),
            quantizations: descriptor
                .quantizations
                .iter()
                .map(|QuantizationDefinition { id, .. }| QuantizationVram {
                    id: (*id).to_owned(),
                    estimate: estimate_vram_with(descriptor, Some(id), vision, measurements),
                })
                .collect(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn descriptor(id: &str) -> &'static LocalModelDescriptor {
        MODELS
            .iter()
            .find(|descriptor| descriptor.id == id)
            .unwrap_or_else(|| panic!("{id} is cataloged"))
    }

    #[test]
    fn parameter_markers_parse_from_ids() {
        assert_eq!(parameters_billion("gemma4-e2b-it"), Some(2.0));
        assert_eq!(parameters_billion("gemma4-26b-a4b-it"), Some(26.0));
        assert_eq!(parameters_billion("qwen3.5-2b"), Some(2.0));
        assert_eq!(parameters_billion("ministral-3-8b-instruct"), Some(8.0));
        assert_eq!(parameters_billion("lfm2.5-1.2b-instruct"), Some(1.2));
        assert_eq!(parameters_billion("qwen3.6-35b-a3b"), Some(35.0));
    }

    #[test]
    fn measured_peaks_match_the_reference_table() {
        let e2b = estimate_vram(descriptor("gemma4-e2b-it"), Some("Q4_K_XL"), false);
        assert_eq!(e2b.bytes, 3100 * 1024 * 1024);
        assert!(e2b.measured);

        let e4b_vision = estimate_vram(descriptor("gemma4-e4b-uncensored"), Some("Q4_K_P"), true);
        assert_eq!(
            e4b_vision.bytes,
            5528 * 1024 * 1024 + VISION_PROJECTOR_BYTES
        );

        // The instruct E4B peak was measured the same way, on the same card.
        let e4b_it = estimate_vram(descriptor("gemma4-e4b-it"), Some("Q4_K_XL"), true);
        assert_eq!(e4b_it.bytes, 3446 * 1024 * 1024 + VISION_PROJECTOR_BYTES);
        assert!(
            e4b_it.bytes < budget_from_total(5 * GIB),
            "the instruct 4B must stay usable on a 5 GiB budget"
        );

        let twelve = estimate_vram(descriptor("gemma4-12b-it"), Some("Q4_K_XL"), true);
        assert_eq!(
            twelve.bytes,
            7700 * 1024 * 1024 + VISION_PROJECTOR_BYTES,
            "the 12B vision estimate must stay above an 8 GiB budget"
        );
    }

    #[test]
    fn unmeasured_models_use_the_parameter_formula() {
        let estimate = estimate_vram(descriptor("qwen3.5-4b"), Some("Q4_K_XL"), false);
        assert!(!estimate.measured);
        let expected = (4.0 * 1_000_000_000.0 * 0.57) as u64 + CONTEXT_OVERHEAD_BYTES;
        assert_eq!(estimate.bytes, expected);
    }

    #[test]
    fn auto_grows_with_the_card_up_to_the_largest_model_that_fits() {
        let expected = [
            (8, "gemma4-e4b-uncensored"),
            (12, "gemma4-12b-uncensored"),
            (20, "gemma4-26b-a4b-uncensored"),
            (24, "gemma4-31b-uncensored"),
            (48, "gemma4-31b-uncensored"),
        ];
        for (gib, model) in expected {
            let budget = budget_from_total(gib * GIB);
            let choice = resolve_auto(budget, true)
                .unwrap_or_else(|| panic!("{gib} GiB fits a vision model"));
            assert_eq!(choice.model, model, "a {gib} GiB card must not step down");
            assert!(
                choice.estimate.bytes <= budget,
                "the auto pick must respect the budget"
            );
        }
    }

    #[test]
    fn download_sizes_cover_the_weights_and_the_projector() {
        // A measured configuration derives its weight size from the measured
        // peak minus the runtime overhead the estimate adds on top.
        let e4b = descriptor("gemma4-e4b-uncensored");
        assert_eq!(
            download_bytes(e4b, Some("Q4_K_P"), true),
            (5528 - 1024) * 1024 * 1024 + VISION_PROJECTOR_BYTES
        );
        assert_eq!(
            download_bytes(e4b, Some("Q4_K_P"), false),
            (5528 - 1024) * 1024 * 1024,
            "a text-only run does not download the projector"
        );

        // An unmeasured configuration falls back to the parameter formula, so a
        // larger model always reports a larger download.
        let twelve = download_bytes(descriptor("gemma4-12b-it"), Some("Q4_K_XL"), true);
        let twenty_six = download_bytes(descriptor("gemma4-26b-a4b-it"), Some("Q4_K_XL"), true);
        let thirty_one = download_bytes(descriptor("gemma4-31b-it"), Some("Q4_K_XL"), true);
        assert!(twelve < twenty_six && twenty_six < thirty_one);
        assert!(
            twelve >= 5 * GIB,
            "the 12B model is a multi-gigabyte download"
        );
    }

    #[test]
    fn auto_selects_the_uncensored_e4b_on_an_eight_gib_card() {
        let budget = budget_from_total(8 * GIB);
        let choice = resolve_auto(budget, true).expect("an 8 GiB card fits a local model");
        assert_eq!(choice.model, "gemma4-e4b-uncensored");
        assert_eq!(choice.quantization, "Q4_K_P");

        // A tighter budget steps down the priority list: at 5 GiB the
        // uncensored E4B (≈5.7 GiB with vision) no longer fits and the
        // instruct E4B (≈4.5 GiB, formula-estimated) takes over.
        let choice =
            resolve_auto(budget_from_total(5 * GIB), true).expect("5 GiB fits a vision model");
        assert_eq!(choice.model, "gemma4-e4b-it");

        // The projector (+0.9 GiB) can push every vision model off a small
        // card even though a text-only model would still fit.
        assert!(resolve_auto(budget_from_total(4 * GIB), true).is_none());
        let choice = resolve_auto(budget_from_total(4 * GIB), false).expect("text-only fits 4 GiB");
        assert_eq!(choice.model, "gemma4-e4b-it");

        // Below the smallest footprint nothing fits at all.
        assert!(resolve_auto(budget_from_total(2 * GIB), true).is_none());
    }

    #[test]
    fn auto_skips_text_only_models_when_vision_is_required() {
        for budget in [budget_from_total(8 * GIB), u64::MAX] {
            let Some(choice) = resolve_auto(budget, true) else {
                continue;
            };
            assert_ne!(
                choice.model, "ministral-3-8b-instruct",
                "a vision selection must not pick a model without a projector"
            );
        }
    }

    #[test]
    fn the_twelve_byte_model_is_refused_on_eight_gib_with_alternatives() {
        let budget = budget_from_total(8 * GIB);
        let error = check_budget(budget, "gemma4-12b-it", Some("Q4_K_XL"), true)
            .expect_err("the 12B model cannot fit 8 GiB with vision");
        let BudgetCheck::Exceeded(error) = error else {
            panic!("expected a budget exceedance")
        };
        assert!(error.alternatives.iter().any(|alternative| {
            alternative.starts_with("gemma4-e4b-uncensored")
                || alternative.starts_with("gemma4-e4b-it")
        }));
    }

    #[test]
    fn budget_check_accepts_a_fitting_model() {
        let budget = budget_from_total(8 * GIB);
        let estimate = check_budget(budget, "gemma4-e4b-uncensored", Some("Q4_K_P"), true)
            .expect("the recommended model fits 8 GiB");
        assert!(estimate.measured);
    }

    #[test]
    fn the_catalog_reports_every_model() {
        let catalog = vram_catalog(false);
        assert!(!catalog.is_empty());
        let e4b = catalog
            .iter()
            .find(|model| model.id == "gemma4-e4b-uncensored")
            .expect("cataloged");
        assert!(e4b.vision);
        assert!(
            e4b.quantizations
                .iter()
                .any(|quantization| quantization.id == "Q4_K_P")
        );
    }

    fn sample_peak(model: &str, vision: bool, bytes: u64) -> MeasuredPeak {
        MeasuredPeak {
            model: model.to_owned(),
            quantization: "Q4_K_P".to_owned(),
            vision,
            bytes,
        }
    }

    #[test]
    fn real_measurements_override_builtin_estimates() {
        let mut measurements = MeasuredPeaks::new();
        measurements.record(sample_peak(
            "gemma4-e4b-uncensored",
            true,
            (6.4f64 * GIB as f64) as u64,
        ));
        let estimate = estimate_vram_with(
            descriptor("gemma4-e4b-uncensored"),
            Some("Q4_K_P"),
            true,
            &measurements,
        );
        assert!(estimate.measured);
        assert_eq!(estimate.bytes, (6.4f64 * GIB as f64) as u64);
    }

    #[test]
    fn measurements_distinguish_vision_from_text_only() {
        let mut measurements = MeasuredPeaks::new();
        measurements.record(sample_peak("gemma4-e4b-uncensored", true, 6 * GIB));
        assert_eq!(
            measurements.peak_bytes("gemma4-e4b-uncensored", "Q4_K_P", true),
            Some(6 * GIB)
        );
        assert_eq!(
            measurements.peak_bytes("gemma4-e4b-uncensored", "Q4_K_P", false),
            None,
            "a vision sample must not apply to a text-only run"
        );
    }

    #[test]
    fn recording_keeps_the_highest_observation_and_merges_tables() {
        let mut measurements = MeasuredPeaks::new();
        measurements.record(sample_peak("gemma4-e4b-uncensored", true, 6 * GIB));
        measurements.record(sample_peak("gemma4-e4b-uncensored", true, 5 * GIB));
        assert_eq!(
            measurements.peak_bytes("gemma4-e4b-uncensored", "Q4_K_P", true),
            Some(6 * GIB)
        );
        let mut merged = MeasuredPeaks::new();
        merged.record(sample_peak("gemma4-e4b-uncensored", true, 7 * GIB));
        measurements.merge(&merged);
        assert_eq!(
            measurements.peak_bytes("gemma4-e4b-uncensored", "Q4_K_P", true),
            Some(7 * GIB)
        );
    }

    #[test]
    fn auto_steps_down_when_the_measured_peak_exceeds_the_budget() {
        let mut measurements = MeasuredPeaks::new();
        // The preferred model measured over this machine's usable budget.
        measurements.record(sample_peak(
            "gemma4-e4b-uncensored",
            true,
            (7.4f64 * GIB as f64) as u64,
        ));
        let budget = budget_from_total(8 * GIB);
        let choice =
            resolve_auto_with(budget, true, &measurements).expect("the fallback still fits");
        assert_ne!(
            choice.model, "gemma4-e4b-uncensored",
            "auto must skip a model that measured over budget"
        );
    }

    #[test]
    fn budget_check_refuses_a_model_that_measured_over_budget() {
        let mut measurements = MeasuredPeaks::new();
        measurements.record(sample_peak(
            "gemma4-e4b-uncensored",
            true,
            (7.4f64 * GIB as f64) as u64,
        ));
        let budget = budget_from_total(8 * GIB);
        let error = check_budget_with(
            budget,
            "gemma4-e4b-uncensored",
            Some("Q4_K_P"),
            true,
            &measurements,
        )
        .expect_err("the measured footprint exceeds the budget");
        let BudgetCheck::Exceeded(error) = error else {
            panic!("expected a budget exceedance")
        };
        assert!(error.estimate.measured);
        assert_eq!(error.estimate.bytes, (7.4f64 * GIB as f64) as u64);
    }

    #[test]
    fn measured_peaks_counts_configurations() {
        let mut measurements = MeasuredPeaks::new();
        assert!(measurements.is_empty());
        measurements.record(sample_peak("gemma4-e4b-uncensored", true, 6 * GIB));
        measurements.record(sample_peak("gemma4-e4b-uncensored", false, 5 * GIB));
        assert_eq!(measurements.len(), 2);
    }
}
