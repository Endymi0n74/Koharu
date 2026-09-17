//! Persistence of real VRAM measurements across `koharu-batch` runs.
//!
//! Every completed run records the peak footprint it observed for the model
//! configuration it used. The measurements live in
//! `%USERPROFILE%\.koharu\vram-calibration.toml` (or the platform equivalent)
//! and are merged — keeping the highest observation per configuration — so
//! `--llm auto` and the budget guard stay calibrated on the machine that
//! actually runs them.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use koharu_translator::preset::{MeasuredPeak, MeasuredPeaks};

/// Default calibration file location.
#[must_use]
pub fn default_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".koharu").join("vram-calibration.toml"))
}

/// Loads the calibration table, or an empty one when the file does not exist
/// yet. A corrupt file degrades to empty (built-in estimates still apply)
/// rather than failing the run.
#[must_use]
pub fn load(path: &Path) -> MeasuredPeaks {
    let Ok(content) = std::fs::read_to_string(path) else {
        return MeasuredPeaks::new();
    };
    parse(&content).unwrap_or_else(|error| {
        eprintln!(
            "warning: ignoring corrupt calibration file {} ({error})",
            path.display()
        );
        MeasuredPeaks::new()
    })
}

/// Parses a calibration table from its TOML text.
fn parse(content: &str) -> Result<MeasuredPeaks> {
    #[derive(serde::Deserialize)]
    struct File {
        #[serde(default)]
        peaks: Vec<MeasuredPeak>,
    }
    let file: File = toml::from_str(content).context("invalid calibration TOML")?;
    let mut measurements = MeasuredPeaks::new();
    for peak in file.peaks {
        measurements.record(peak);
    }
    Ok(measurements)
}

/// Serializes a calibration table to its TOML text.
fn serialize(measurements: &MeasuredPeaks) -> String {
    #[derive(serde::Serialize)]
    struct File {
        peaks: Vec<MeasuredPeak>,
    }
    let document = toml::to_string_pretty(&File {
        peaks: measurements.iter().cloned().collect(),
    })
    .expect("calibration entries serialize");
    format!(
        "# Written by koharu-batch: real VRAM peaks observed per model\n\
         # configuration, used to calibrate --llm auto and budget checks.\n\
         # Deleting the file restores the built-in reference estimates.\n{document}"
    )
}

/// Saves the calibration table, creating the parent directory as needed.
pub fn save(path: &Path, measurements: &MeasuredPeaks) -> Result<()> {
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, serialize(measurements))
        .with_context(|| format!("failed to write {}", path.display()))
}

/// Merges one new observation into the table on disk and saves it. Returns
/// the merged table (the value future runs will load).
pub fn record(path: &Path, peak: MeasuredPeak) -> Result<MeasuredPeaks> {
    let mut measurements = load(path);
    measurements.record(peak);
    save(path, &measurements)?;
    Ok(measurements)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(model: &str, quantization: &str, vision: bool, bytes: u64) -> MeasuredPeak {
        MeasuredPeak {
            model: model.to_owned(),
            quantization: quantization.to_owned(),
            vision,
            bytes,
        }
    }

    #[test]
    fn missing_files_load_empty() {
        let measurements = load(Path::new("Z:/definitely/missing/calibration.toml"));
        assert!(measurements.is_empty());
    }

    #[test]
    fn tables_round_trip_through_toml() {
        let mut written = MeasuredPeaks::new();
        written.record(peak(
            "gemma4-e4b-uncensored",
            "Q4_K_P",
            true,
            6_400_000_000,
        ));
        written.record(peak("gemma4-e2b-it", "Q4_K_XL", false, 3_100_000_000));
        let text = serialize(&written);
        assert!(text.contains("[[peaks]]"));
        let read_back = parse(&text).expect("parses");
        assert_eq!(read_back.len(), 2);
        assert_eq!(
            read_back.peak_bytes("gemma4-e4b-uncensored", "Q4_K_P", true),
            Some(6_400_000_000)
        );
        assert_eq!(
            read_back.peak_bytes("gemma4-e4b-uncensored", "Q4_K_P", false),
            None
        );
    }

    #[test]
    fn corrupt_files_degrade_to_empty() {
        let measurements = parse("not toml at all ===").expect_err("invalid TOML");
        let _ = measurements;
        let parsed = parse("[peaks]\nbytes = true");
        assert!(parsed.is_err(), "invalid entries must not parse");
    }

    #[test]
    fn recording_merges_with_the_highest_observation() {
        let directory = std::env::temp_dir().join(format!(
            "koharu-calibration-test-{}",
            std::process::id()
        ));
        let path = directory.join("vram-calibration.toml");
        let _ = std::fs::remove_file(&path);

        let first = record(&path, peak("gemma4-e4b-uncensored", "Q4_K_P", true, 6 * GIB_TEST))
            .expect("saves");
        assert_eq!(first.len(), 1);
        let second = record(&path, peak("gemma4-e4b-uncensored", "Q4_K_P", true, 7 * GIB_TEST))
            .expect("saves");
        assert_eq!(
            second.peak_bytes("gemma4-e4b-uncensored", "Q4_K_P", true),
            Some(7 * GIB_TEST),
            "the higher observation wins"
        );
        let third = record(&path, peak("gemma4-e4b-uncensored", "Q4_K_P", true, 5 * GIB_TEST))
            .expect("saves");
        assert_eq!(
            third.peak_bytes("gemma4-e4b-uncensored", "Q4_K_P", true),
            Some(7 * GIB_TEST),
            "a lower re-measurement never lowers the calibration"
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(directory);
    }

    const GIB_TEST: u64 = 1024 * 1024 * 1024;
}
