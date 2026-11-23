//! VRAM budgeting for the local translation model `--translator auto` picks.
//!
//! The numbers are the fork's (`koharu-translator::preset`): peaks measured on
//! its reference machine (Ryzen 7600 / RTX 3070 8 GB, CUDA, 8-segment
//! Japanese→French prompt under the constrained JSON schema) where one exists,
//! and elsewhere an estimate from the parameter count and the quantization.
//! Estimates exist to refuse what cannot fit, not to predict exact peaks.
//!
//! Not ported: the fork's calibration file, which recorded every run's own
//! peak and fed later estimates. 0.92 has nothing to record it from — its
//! engine lets llama.cpp place the weights and publishes free memory only when
//! loading fails (`koharu-pipeline/src/gpu.rs`) — so a calibration file here
//! could never be written.

/// One gibibyte.
const GIB: u64 = 1024 * 1024 * 1024;

/// Context and compute buffers observed around the measured weights footprint.
const CONTEXT_OVERHEAD_BYTES: u64 = GIB;
/// Detection, OCR, and inpainting, which load around the language model on the
/// same card: Hy-MT2 7B estimated at 4.7 GiB peaked at 5.6 GiB for the whole
/// pipeline, Qwen 3.5 4B at 4.0 GiB peaked at 4.9 GiB, the same 0.9 GiB both
/// times. A measured peak already includes it.
const PIPELINE_STAGES_BYTES: u64 = (0.9 * GIB as f64) as u64;
/// A vision projector (mtmd) on top of the text-only peak.
const VISION_PROJECTOR_BYTES: u64 = (0.9 * GIB as f64) as u64;
/// The fraction of physical VRAM usable before the compositor's overhead makes
/// llama.cpp thrash.
const BUDGET_MARGIN: f64 = 0.9;

/// Peaks of the fork's reference machine, in mebibytes, keyed by model and
/// quantization: what the whole pipeline took above the desktop baseline, so
/// the other stages are already inside them.
const MEASURED_PEAKS: &[(&str, &str, u64)] = &[("hy-mt2-7b", "Q4_K_M", 5734)];

/// The models `auto` may pick, best first: Hy-MT2 7B won the fork's
/// Japanese→French measurement, its 1.8B followed, and Index-Translate 2B
/// came last.
const CANDIDATES: &[(&str, &str)] = &[
    ("hy-mt2-7b", "Q4_K_M"),
    ("hy-mt2-1.8b", "Q4_K_M"),
    ("index-translate-2b", "Q4_K_M"),
];

/// Estimated VRAM footprint of one configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VramEstimate {
    pub bytes: u64,
    /// Whether the peak was measured rather than estimated.
    pub measured: bool,
}

impl VramEstimate {
    /// The footprint as a terminal shows it, such as `5.6 GiB (measured)`.
    #[must_use]
    pub fn display(&self) -> String {
        format!(
            "{:.1} GiB{}",
            self.bytes as f64 / GIB as f64,
            if self.measured { " (measured)" } else { "" }
        )
    }
}

/// What `auto` picked: a `vendor/model:QUANTIZATION` reference and what it is
/// expected to take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub reference: String,
    pub estimate: VramEstimate,
}

/// Estimates the peak of `model` at `quantization`, including the vision
/// projector when `vision` asks for one.
#[must_use]
pub fn estimate(model: &str, quantization: &str, vision: bool) -> VramEstimate {
    if let Some(&(_, _, mebibytes)) = MEASURED_PEAKS
        .iter()
        .find(|&&(measured, quant, _)| measured == model && quant == quantization)
    {
        return VramEstimate {
            bytes: mebibytes * 1024 * 1024 + if vision { VISION_PROJECTOR_BYTES } else { 0 },
            measured: true,
        };
    }
    let weights = parameters_billion(model).map_or(0.0, |parameters| {
        parameters * 1_000_000_000.0 * bytes_per_parameter(quantization)
    });
    VramEstimate {
        bytes: weights as u64
            + CONTEXT_OVERHEAD_BYTES
            + PIPELINE_STAGES_BYTES
            + if vision { VISION_PROJECTOR_BYTES } else { 0 },
        measured: false,
    }
}

/// The strongest candidate whose whole peak fits `budget` bytes, or `None` when
/// none does.
#[must_use]
pub fn resolve(budget: u64, vision: bool) -> Option<Choice> {
    CANDIDATES
        .iter()
        .map(|&(model, quantization)| {
            let estimate = estimate(model, quantization, vision);
            (model, quantization, estimate)
        })
        .find(|(_, _, estimate)| estimate.bytes <= budget)
        .map(|(model, quantization, estimate)| Choice {
            reference: format!("koharu/{model}:{quantization}"),
            estimate,
        })
}

/// The share of a card's physical VRAM a run may use.
#[must_use]
pub fn budget_from_total(total: u64) -> u64 {
    (total as f64 * BUDGET_MARGIN) as u64
}

/// The estimate for a `vendor/model:QUANTIZATION` reference, which `auto`
/// itself produces; `None` when it names no quantization.
#[must_use]
pub fn of_reference(reference: &str, vision: bool) -> Option<(String, String, VramEstimate)> {
    let (model, quantization) = reference.split_once(':')?;
    let model = model.split_once('/').map_or(model, |(_, model)| model);
    let estimate = estimate(model, quantization, vision);
    Some((model.to_owned(), quantization.to_owned(), estimate))
}

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

/// The largest `…b` parameter marker of a model id: `gemma4-e2b-it` → 2,
/// `qwen3.5-2b` → 2, `gemma4-26b-a4b-it` → 26.
#[must_use]
pub fn parameters_billion(id: &str) -> Option<f64> {
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
            Some(largest.map_or(value, |largest| largest.max(value)))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_parameter_count_comes_from_the_largest_marker() {
        assert_eq!(parameters_billion("hy-mt2-7b"), Some(7.0));
        assert_eq!(parameters_billion("gemma4-26b-a4b-it"), Some(26.0));
        assert_eq!(parameters_billion("qwen3.5-0.8b"), Some(0.8));
        assert_eq!(parameters_billion("index-translate-2b"), Some(2.0));
        assert_eq!(parameters_billion("manga-ocr"), None);
    }

    #[test]
    fn a_measured_peak_wins_over_the_estimate() {
        let measured = estimate("hy-mt2-7b", "Q4_K_M", false);
        assert!(measured.measured);
        assert_eq!(measured.bytes, 5734 * 1024 * 1024);
        let estimated = estimate("hy-mt2-1.8b", "Q4_K_M", false);
        assert!(!estimated.measured);
        assert!(estimated.bytes < measured.bytes);
        assert_eq!(
            estimate("index-translate-2b", "Q4_K_M", true).bytes
                - estimate("index-translate-2b", "Q4_K_M", false).bytes,
            VISION_PROJECTOR_BYTES
        );
    }

    #[test]
    fn auto_takes_the_strongest_model_the_budget_holds() {
        let eight = budget_from_total(8 * GIB);
        assert_eq!(
            resolve(eight, false).expect("8 GiB holds it").reference,
            "koharu/hy-mt2-7b:Q4_K_M"
        );
        let three = 3 * GIB;
        assert_eq!(
            resolve(three, false)
                .expect("3 GiB holds the 1.8B")
                .reference,
            "koharu/hy-mt2-1.8b:Q4_K_M"
        );
        assert!(resolve(GIB / 2, false).is_none());
    }
}
