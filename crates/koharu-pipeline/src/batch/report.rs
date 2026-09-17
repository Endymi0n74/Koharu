//! End-of-run report summarizing every page of a batch translation.
//!
//! The report is written as Markdown and as a self-contained HTML file so a
//! chapter can be reviewed (timings, model, failures) without re-running it.

use std::time::Duration;

/// Outcome of one page within the batch run.
#[derive(Clone, Debug)]
pub enum PageOutcome {
    /// The page was fully translated and written.
    Translated {
        /// Total pipeline duration for the page.
        elapsed: Duration,
        /// Per-stage durations in execution order.
        stages: Vec<(String, Duration)>,
    },
    /// The page already had an output and was skipped (resume mode).
    Skipped,
    /// The page failed; the message is the error chain summary.
    Failed(String),
}

/// One row of the report.
#[derive(Clone, Debug)]
pub struct PageReport {
    /// Zero-based reading-order position.
    pub index: usize,
    /// Original page name (archive entry for CBZ inputs).
    pub name: String,
    /// What happened to the page.
    pub outcome: PageOutcome,
}

impl PageReport {
    /// A failed page row.
    #[must_use]
    pub fn failed(index: usize, name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            outcome: PageOutcome::Failed(error.into()),
        }
    }
}

/// End-of-run summary handed to the writers.
pub struct RunReport {
    /// Pages in reading order, translated pages and pre-existing failures.
    pub pages: Vec<PageReport>,
    /// Wall-clock duration of the whole run.
    pub total_elapsed: Duration,
    /// Translation model id.
    pub model: String,
    /// Quantization of the translation model.
    pub quantization: String,
    /// VRAM estimate for the model/quantization pair.
    pub vram_estimate: Option<String>,
    /// Target language tag (for example `fr-FR`).
    pub language: String,
    /// Input source description (path or archive).
    pub input: String,
    /// Output description (folder or archive path).
    pub output: String,
    /// Translation model execution device.
    pub device: String,
    /// Wall-clock date and time when the run started.
    pub started_at: String,
}

impl RunReport {
    /// Number of translated, skipped, and failed pages.
    #[must_use]
    pub fn counts(&self) -> (usize, usize, usize) {
        let mut translated = 0;
        let mut skipped = 0;
        let mut failed = 0;
        for page in &self.pages {
            match page.outcome {
                PageOutcome::Translated { .. } => translated += 1,
                PageOutcome::Skipped => skipped += 1,
                PageOutcome::Failed(_) => failed += 1,
            }
        }
        (translated, skipped, failed)
    }

    /// Seconds of the total elapsed time, for formatting.
    #[must_use]
    pub fn total_seconds(&self) -> f64 {
        self.total_elapsed.as_secs_f64()
    }
}

fn stage_seconds(stages: &[(String, Duration)]) -> String {
    if stages.is_empty() {
        return "—".to_owned();
    }
    stages
        .iter()
        .map(|(stage, elapsed)| format!("{stage} {:.1}s", elapsed.as_secs_f64()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn seconds(duration: Duration) -> String {
    format!("{:.1}s", duration.as_secs_f64())
}

/// French pluralization helper: `"1 traduite"` / `"3 traduites"`.
fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("{count} {one}")
    } else {
        format!("{count} {many}")
    }
}

fn page_counts_line(translated: usize, skipped: usize, failed: usize) -> String {
    format!(
        "{}, {}, {}",
        plural(translated, "traduite", "traduites"),
        plural(skipped, "ignorée", "ignorées"),
        plural(failed, "en échec", "en échec")
    )
}

/// Renders the run report as Markdown.
#[must_use]
pub fn to_markdown(report: &RunReport) -> String {
    let (translated, skipped, failed) = report.counts();
    let mut document = String::new();
    document.push_str("# Rapport de traduction — Koharu batch\n\n");
    document.push_str("| | |\n|---|---|\n");
    document.push_str(&format!(
        "| Entrée | `{}` |\n| Sortie | `{}` |\n| Langue cible | {} |\n",
        report.input, report.output, report.language
    ));
    document.push_str(&format!(
        "| Modèle | `{}` (`{}`) |\n| VRAM estimée | {} |\n| Périphérique | {} |\n",
        report.model,
        report.quantization,
        report.vram_estimate.as_deref().unwrap_or("inconnue"),
        report.device
    ));
    document.push_str(&format!(
        "| Démarré | {} |\n| Durée totale | {:.1}s |\n| Pages | {} |\n\n",
        report.started_at,
        report.total_seconds(),
        page_counts_line(translated, skipped, failed)
    ));

    document.push_str("| # | Page | Statut | Durée | Détail |\n|---:|---|---|---:|---|\n");
    for page in &report.pages {
        let detail = match &page.outcome {
            PageOutcome::Translated { stages, .. } => stage_seconds(stages),
            PageOutcome::Skipped => "sortie déjà présente".to_owned(),
            PageOutcome::Failed(error) => escape_markdown_cell(error),
        };
        let status = match &page.outcome {
            PageOutcome::Translated { .. } => "✅ traduite",
            PageOutcome::Skipped => "⏭️ ignorée",
            PageOutcome::Failed(_) => "❌ échec",
        };
        let elapsed = match &page.outcome {
            PageOutcome::Translated { elapsed, .. } => seconds(*elapsed),
            _ => "—".to_owned(),
        };
        document.push_str(&format!(
            "| {} | `{}` | {} | {} | {} |\n",
            page.index + 1,
            page.name,
            status,
            elapsed,
            detail
        ));
    }
    document
}

fn escape_markdown_cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

const REPORT_STYLES: &str = "\
body{font-family:'Segoe UI',system-ui,sans-serif;margin:2rem auto;max-width:60rem;\
padding:0 1rem;color:#1c1c1c;background:#fafafa}\
h1{font-size:1.4rem}\
table.summary td:first-child{font-weight:600;white-space:nowrap;padding-right:1rem}\
table.pages{border-collapse:collapse;width:100%;margin-top:1rem}\
table.pages th,table.pages td{border:1px solid #d0d0d0;padding:.35rem .6rem;text-align:left}\
table.pages tr.failed{background:#fde8e8}\
table.pages tr.skipped{background:#f4f4f4;color:#666}\
.status-ok{color:#137333;font-weight:600}.status-fail{color:#c5221f;font-weight:600}\
footer{margin-top:1.5rem;color:#777;font-size:.85rem}";

/// Renders the run report as a self-contained HTML document.
#[must_use]
pub fn to_html(report: &RunReport) -> String {
    let (translated, skipped, failed) = report.counts();
    let mut rows = String::new();
    for page in &report.pages {
        let (status_class, status, elapsed, detail) = match &page.outcome {
            PageOutcome::Translated { elapsed, stages } => (
                "ok",
                "traduite",
                seconds(*elapsed),
                html_escape(&stage_seconds(stages)),
            ),
            PageOutcome::Skipped => (
                "skipped",
                "ignorée",
                "—".to_owned(),
                "sortie déjà présente".to_owned(),
            ),
            PageOutcome::Failed(error) => (
                "failed",
                "échec",
                "—".to_owned(),
                html_escape(&error.replace('\n', " ")),
            ),
        };
        rows.push_str(&format!(
            "<tr class=\"{status_class}\"><td>{}</td><td><code>{}</code></td>\
<td class=\"status-{status_class}\">{}</td><td>{elapsed}</td><td>{detail}</td></tr>",
            page.index + 1,
            html_escape(&page.name),
            status,
        ));
    }
    let vram = report
        .vram_estimate
        .as_deref()
        .unwrap_or("inconnue")
        .to_owned();
    format!(
        "<!doctype html>\n<html lang=\"fr\">\n<head>\n<meta charset=\"utf-8\">\n\
<title>Koharu batch — {input}</title>\n<style>{REPORT_STYLES}</style>\n</head>\n<body>\n\
<h1>Rapport de traduction — Koharu batch</h1>\n\
<table class=\"summary\">\n\
<tr><td>Entrée</td><td><code>{input}</code></td></tr>\n\
<tr><td>Sortie</td><td><code>{output}</code></td></tr>\n\
<tr><td>Langue cible</td><td>{language}</td></tr>\n\
<tr><td>Modèle</td><td><code>{model}</code> (<code>{quantization}</code>)</td></tr>\n\
<tr><td>VRAM estimée</td><td>{vram}</td></tr>\n\
<tr><td>Périphérique</td><td>{device}</td></tr>\n\
<tr><td>Démarré</td><td>{started}</td></tr>\n\
<tr><td>Durée totale</td><td>{total:.1}s</td></tr>\n\
<tr><td>Pages</td><td>{pages_line}</td></tr>\n\
</table>\n\
<table class=\"pages\">\n\
<tr><th>#</th><th>Page</th><th>Statut</th><th>Durée</th><th>Détail</th></tr>\n{rows}\
</table>\n\
<footer>Généré par koharu-batch</footer>\n</body>\n</html>\n",
        input = html_escape(&report.input),
        output = html_escape(&report.output),
        language = html_escape(&report.language),
        model = html_escape(&report.model),
        quantization = html_escape(&report.quantization),
        vram = html_escape(&vram),
        device = html_escape(&report.device),
        started = html_escape(&report.started_at),
        total = report.total_seconds(),
        pages_line = page_counts_line(translated, skipped, failed),
    )
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Formats a timestamp as `YYYY-MM-DD HH:MM:SS` (UTC), without pulling a date
/// dependency into the pipeline.
#[must_use]
pub fn format_timestamp_utc(system_time: std::time::SystemTime) -> String {
    let seconds = system_time
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let days = seconds / 86_400;
    let time_of_day = seconds % 86_400;
    // Howard Hinnant's civil-from-days algorithm.
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    let (hour, minute, second) = (
        time_of_day / 3_600,
        (time_of_day % 3_600) / 60,
        time_of_day % 60,
    );
    format!(
        "{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC"
    )
}

/// Current wall-clock time, formatted for the report header.
#[must_use]
pub fn timestamp_now() -> String {
    format_timestamp_utc(std::time::SystemTime::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RunReport {
        RunReport {
            pages: vec![
                PageReport {
                    index: 0,
                    name: "001.png".to_owned(),
                    outcome: PageOutcome::Translated {
                        elapsed: Duration::from_millis(4250),
                        stages: vec![
                            ("detection".to_owned(), Duration::from_millis(700)),
                            ("ocr".to_owned(), Duration::from_millis(1100)),
                            ("inpainting".to_owned(), Duration::from_millis(600)),
                            ("translation".to_owned(), Duration::from_millis(1800)),
                        ],
                    },
                },
                PageReport {
                    index: 1,
                    name: "002.png".to_owned(),
                    outcome: PageOutcome::Skipped,
                },
                PageReport::failed(2, "010.png", "translation failed: model timeout"),
            ],
            total_elapsed: Duration::from_millis(15_000),
            model: "gemma4-e4b-uncensored".to_owned(),
            quantization: "Q4_K_P".to_owned(),
            vram_estimate: Some("5.6 GiB (measured)".to_owned()),
            language: "fr-FR".to_owned(),
            input: "chapter-test.cbz".to_owned(),
            output: "chapter-test-fr.cbz".to_owned(),
            device: "NVIDIA GeForce RTX 3070 (CUDA)".to_owned(),
            started_at: "2026-09-17 08:12:00".to_owned(),
        }
    }

    #[test]
    fn markdown_lists_every_page_with_status() {
        let markdown = to_markdown(&sample());
        assert!(markdown.contains("gemma4-e4b-uncensored"));
        assert!(markdown.contains("fr-FR"));
        assert!(markdown.contains("✅ traduite"));
        assert!(markdown.contains("⏭️ ignorée"));
        assert!(markdown.contains("❌ échec"));
        assert!(markdown.contains("detection 0.7s"));
        assert!(markdown.contains("translation failed: model timeout"));
        assert!(markdown.contains("1 traduite, 1 ignorée, 1 en échec"));
    }

    #[test]
    fn html_is_self_contained_and_escapes() {
        let html = to_html(&sample());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<style>"));
        assert!(!html.contains("<script"));
        assert!(html.contains("gemma4-e4b-uncensored"));
        assert!(html.contains("tr class=\"failed\""));
        assert!(html.contains("tr class=\"skipped\""));
    }

    #[test]
    fn failure_text_is_sanitized_for_tables() {
        let mut report = sample();
        report.pages[2] = PageReport::failed(2, "010.png", "pipe | and newline\nhere");
        let markdown = to_markdown(&report);
        assert!(markdown.contains("pipe \\| and newline here"));
        // Pipes are valid inside HTML cells; only Markdown needs escaping.
        let html = to_html(&report);
        assert!(html.contains("pipe | and newline here"));
        assert!(!html.contains("pipe | and\n"));
    }

    #[test]
    fn timestamp_formats_known_instants() {
        let epoch = std::time::UNIX_EPOCH;
        assert_eq!(format_timestamp_utc(epoch), "1970-01-01 00:00:00 UTC");
        let known = epoch + std::time::Duration::from_secs(1_760_000_000);
        // 2025-10-09 08:53:20 UTC
        assert_eq!(format_timestamp_utc(known), "2025-10-09 08:53:20 UTC");
        let leap_day = epoch + std::time::Duration::from_secs(951_782_400);
        // 2000-02-29 00:00:00 UTC
        assert_eq!(format_timestamp_utc(leap_day), "2000-02-29 00:00:00 UTC");
    }
}
