//! VRAM budgeting for local translation models.
//!
//! Peaks measured on the fork's reference machine (Ryzen 7600 / RTX 3070 8 GB,
//! CUDA, 8-segment Japanese→French prompt under the constrained JSON schema)
//! are recorded in [`MEASURED_PEAKS`]; everything else is estimated from the
//! parameter count and quantization. Estimates trade precision for coverage:
//! the goal is refusing models that cannot fit, not predicting exact peaks.

use super::catalog::MODELS;
use super::catalog::LocalModelDescriptor;
use crate::QuantizationDefinition;

const GIB: u64 = 1024 * 1024 * 1024;

/// Context and compute buffers observed around the measured weights footprint.
const CONTEXT_OVERHEAD_BYTES: u64 = GIB;
/// Vision projector footprint (mtmd) added on top of the text-only peak.
const VISION_PROJECTOR_BYTES: u64 = (0.9 * GIB as f64) as u64;
/// Fraction of the physical VRAM usable before Windows/compositor overhead
/// makes llama.cpp thrash.
const BUDGET_MARGIN: f64 = 0.9;

/// Measured no-vision peaks, from `docs/fork.md` ("Choosing a translation
/// model for 8 GB of VRAM"). Keys are (model id, quantization id).
const MEASURED_PEAKS: &[(&str, &str, u64)] = &[
    ("gemma4-e2b-it", "Q4_K_XL", 3100),
    ("gemma4-e4b-uncensored", "Q4_K_P", 4800),
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

/// Estimates the VRAM peak of `descriptor` at `quantization`, including the
/// vision projector when `vision` is requested and the model supports it.
#[must_use]
pub fn estimate_vram(
    descriptor: &LocalModelDescriptor,
    quantization: Option<&str>,
    vision: bool,
) -> VramEstimate {
    let quantization = quantization.or_else(|| {
        descriptor
            .quantizations
            .first()
            .map(|definition| definition.id)
    });
    let base = quantization.and_then(|quantization| measured_peak(descriptor, quantization));
    if let Some(base) = base {
        return VramEstimate {
            bytes: base + vision.then_some(VISION_PROJECTOR_BYTES).unwrap_or(0),
            measured: true,
        };
    }

    let parameters = parameters_billion(descriptor.id).unwrap_or(8.0);
    let bytes_per_parameter = quantization.map_or(0.57, bytes_per_parameter);
    let weights = (parameters * 1_000_000_000.0 * bytes_per_parameter) as u64;
    let projector = u64::from(vision && descriptor.projector.is_some()) * VISION_PROJECTOR_BYTES;
    VramEstimate {
        bytes: weights + CONTEXT_OVERHEAD_BYTES + projector,
        measured: false,
    }
}

/// A local model configuration that fits a VRAM budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoChoice {
    pub model: &'static str,
    pub quantization: &'static str,
    pub estimate: VramEstimate,
}

/// Preference order for `--llm auto`, from `docs/fork.md`: uncensored E4B
/// first, then the instruct E4B, the fastest dense 8B, and the small E2B.
const AUTO_PRIORITY: &[(&str, &str)] = &[
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
            let estimate = estimate_vram(descriptor, Some(quantization), vision);
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
            write!(formatter, "; no local model fits (lower the quantization or use --force)")
        } else {
            write!(formatter, "; models that fit: {}", self.alternatives.join(", "))
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
    let Some(descriptor) = MODELS.iter().find(|descriptor| descriptor.id == model) else {
        return Err(BudgetCheck::UnknownModel);
    };
    let estimate = estimate_vram(descriptor, quantization, vision);
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
                    estimate_vram(candidate, Some(definition.id), vision).bytes <= budget
                })
                .max_by_key(|definition| {
                    estimate_vram(candidate, Some(definition.id), vision).bytes
                })?;
            let estimate = estimate_vram(candidate, Some(definition.id), vision);
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
                    estimate: estimate_vram(descriptor, Some(id), vision),
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

        let e4b_vision =
            estimate_vram(descriptor("gemma4-e4b-uncensored"), Some("Q4_K_P"), true);
        assert_eq!(e4b_vision.bytes, 4800 * 1024 * 1024 + VISION_PROJECTOR_BYTES);

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
        let choice =
            resolve_auto(budget_from_total(4 * GIB), false).expect("text-only fits 4 GiB");
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
        assert!(e4b.quantizations.iter().any(|quantization| quantization.id == "Q4_K_P"));
    }
}
