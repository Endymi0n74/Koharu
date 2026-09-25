//! End-of-run report summarizing every page of a batch translation.
//!
//! The report is written as Markdown and as a self-contained HTML file so a
//! chapter can be reviewed (timings, model, failures) without re-running it.
//! The HTML embeds a small script adding keyboard navigation: arrows move
//! between pages, `+`/`-` adjust the thumbnail scale and `Enter` opens the
//! selected page full screen.

use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// Before/after thumbnails of a translated page, as data URIs.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Thumbnails {
    /// Original page thumbnail (`data:image/jpeg;base64,…`).
    pub before: String,
    /// Translated page thumbnail (`data:image/jpeg;base64,…`).
    pub after: String,
}

/// Outcome of one page within the batch run.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub enum PageOutcome {
    /// The page was fully translated and written.
    Translated {
        /// Total pipeline duration for the page.
        elapsed: Duration,
        /// Per-stage durations in execution order.
        stages: Vec<(String, Duration)>,
        /// Before/after thumbnails, when the report writer captured them.
        thumbnails: Option<Thumbnails>,
        /// Peak GPU memory in use while this page ran (formatted).
        vram_peak: Option<String>,
    },
    /// The page already had an output and was skipped (resume mode).
    Skipped,
    /// The page failed; the message is the error chain summary.
    Failed(String),
}

/// One row of the report.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct PageReport {
    /// Zero-based reading-order position.
    pub index: usize,
    /// Original page name (archive entry for CBZ inputs).
    pub name: String,
    /// What happened to the page.
    pub outcome: PageOutcome,
    /// Translated outcome of the run that produced a skipped page's output,
    /// remembered by the report state (see [`save_state`]) so a resumed run
    /// rewrites the row with the original durations and thumbnails instead
    /// of an empty one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<Box<PageOutcome>>,
}

impl PageReport {
    /// A failed page row.
    #[must_use]
    pub fn failed(index: usize, name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            outcome: PageOutcome::Failed(error.into()),
            previous: None,
        }
    }

    /// A skipped page's row (resume); [`PageReport::remembering`] fills its
    /// `previous` field from the history.
    #[must_use]
    pub fn skipped(index: usize, name: impl Into<String>) -> Self {
        Self {
            index,
            name: name.into(),
            outcome: PageOutcome::Skipped,
            previous: None,
        }
    }

    /// This row with `previous` set to the translated outcome `history`
    /// remembers for it: a resumed run's skipped rows keep the durations,
    /// stage breakdown and thumbnails of the run that wrote their output.
    /// A renamed page (or one whose recorded run never translated it) is
    /// left bare rather than paired with the wrong data.
    #[must_use]
    pub fn remembering(mut self, history: &[PageReport]) -> Self {
        self.previous = history
            .iter()
            .find(|row| row.index == self.index && row.name == self.name)
            .and_then(Self::translated)
            .map(|outcome| Box::new(outcome.clone()));
        self
    }

    /// The translated outcome a stored row holds — its own, or one a resume
    /// carried into its skipped row.
    fn translated(row: &PageReport) -> Option<&PageOutcome> {
        match &row.outcome {
            outcome @ PageOutcome::Translated { .. } => Some(outcome),
            _ => match row.previous.as_deref() {
                outcome @ Some(PageOutcome::Translated { .. }) => outcome,
                _ => None,
            },
        }
    }

    /// The outcome to display: a page this run skipped falls back to the
    /// remembered one, so its row still carries data; every other row shows
    /// what this run did.
    #[must_use]
    pub fn display_outcome(&self) -> &PageOutcome {
        match &self.outcome {
            PageOutcome::Skipped => self.previous.as_deref().unwrap_or(&self.outcome),
            outcome => outcome,
        }
    }
}

/// State file remembering every page's last known outcome across runs:
/// `<base>.state.json`, rewritten together with the report files so an
/// interrupted or resumed run always knows what earlier runs produced.
#[must_use]
pub fn state_path(base: &Path) -> PathBuf {
    let mut path = base.as_os_str().to_owned();
    path.push(".state.json");
    PathBuf::from(path)
}

/// The remembered rows of an earlier run, or `None` when there is no state
/// yet — or it cannot be read, in which case a stale state must never block
/// the run that follows it.
pub fn load_state(base: &Path) -> Option<Vec<PageReport>> {
    let path = state_path(base);
    let data = fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&data) {
        Ok(pages) => Some(pages),
        Err(error) => {
            eprintln!(
                "warning: {} cannot be read ({error}); the previous report data is not reused",
                path.display()
            );
            None
        }
    }
}

/// The state to persist: `pages` (what this run knows) with the rows of
/// `previous` this run does not know — pages left out by `--pages` — so the
/// next resume still remembers them.
#[must_use]
pub fn merge_state(pages: &[PageReport], previous: &[PageReport]) -> Vec<PageReport> {
    let mut state = pages.to_vec();
    for row in previous {
        if !state.iter().any(|known| known.index == row.index) {
            state.push(row.clone());
        }
    }
    state.sort_by_key(|row| row.index);
    state
}

/// Persists the rows a later resume reads to rebuild its skipped rows.
pub fn save_state(base: &Path, pages: &[PageReport], previous: &[PageReport]) -> Result<()> {
    let path = state_path(base);
    let document = serde_json::to_string(&merge_state(pages, previous))
        .with_context(|| format!("failed to encode {}", path.display()))?;
    fs::write(&path, document).with_context(|| format!("failed to write {}", path.display()))
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
    /// Peak GPU memory actually in use during the run (formatted).
    pub vram_peak: Option<String>,
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
    let vram_row = match (&report.vram_estimate, &report.vram_peak) {
        (Some(estimate), Some(peak)) => {
            format!("| VRAM (estimée / pic réel) | {} / {} |\n", estimate, peak)
        }
        (Some(estimate), None) => format!("| VRAM estimée | {} |\n", estimate),
        (None, Some(peak)) => format!("| VRAM (pic réel) | {} |\n", peak),
        (None, None) => String::new(),
    };
    document.push_str(&format!(
        "| Modèle | `{}` (`{}`) |\n{}| Périphérique | {} |\n",
        report.model, report.quantization, vram_row, report.device
    ));
    document.push_str(&format!(
        "| Démarré | {} |\n| Durée totale | {:.1}s |\n| Pages | {} |\n\n",
        report.started_at,
        report.total_seconds(),
        page_counts_line(translated, skipped, failed)
    ));

    document.push_str(
        "| # | Page | Statut | Durée | Pic VRAM | Détail |\n|---:|---|---|---:|---|---|\n",
    );
    for page in &report.pages {
        // A skipped page displays the run the state remembers: its durations,
        // stage breakdown and thumbnails survive the resume that left it be.
        let shown = page.display_outcome();
        let detail = match shown {
            PageOutcome::Translated { stages, .. } => stage_seconds(stages),
            PageOutcome::Skipped => "sortie déjà présente".to_owned(),
            PageOutcome::Failed(error) => escape_markdown_cell(error),
        };
        let (elapsed, vram_peak) = match shown {
            PageOutcome::Translated {
                elapsed, vram_peak, ..
            } => (
                seconds(*elapsed),
                vram_peak.clone().unwrap_or_else(|| "—".to_owned()),
            ),
            _ => ("—".to_owned(), "—".to_owned()),
        };
        let status = match &page.outcome {
            PageOutcome::Translated { .. } => "✅ traduite",
            PageOutcome::Skipped => "⏭️ ignorée",
            PageOutcome::Failed(_) => "❌ échec",
        };
        document.push_str(&format!(
            "| {} | `{}` | {} | {} | {} | {} |\n",
            page.index + 1,
            page.name,
            status,
            elapsed,
            vram_peak,
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
table.pages tr.selected{outline:2px solid #1a73e8;outline-offset:-2px}\
.status-ok{color:#137333;font-weight:600}.status-fail{color:#c5221f;font-weight:600}\
td.thumbs img{border:1px solid #bbb;display:block;margin:2px 0;height:var(--thumb-h);width:auto;max-width:100%}\
td.thumbs a{display:inline-block;margin-right:4px}\
kbd{border:1px solid #bbb;border-radius:3px;padding:0 .3em;font-size:.8em;background:#fff}\
.kbd-hint{color:#666;font-size:.85rem;margin-top:.75rem}\
footer{margin-top:1.5rem;color:#777;font-size:.85rem}\
#page-viewer{display:none;position:fixed;inset:0;background:rgba(0,0,0,.92);z-index:10;\
cursor:zoom-in;text-align:center}\
#page-viewer.open{display:flex;flex-direction:column;align-items:center;justify-content:center}\
#page-viewer img{height:auto;width:auto;max-width:calc(100vw - 6rem);max-height:calc(100vh - 6rem);\
transform-origin:center center}\
#page-viewer .viewer-label{color:#eee;font:.85rem 'Segoe UI',system-ui,sans-serif;margin-top:.75rem}";

/// Default thumbnail height in pixels, adjustable with `+`/`-` in the report.
const DEFAULT_THUMB_HEIGHT: f64 = 72.0;

/// Keyboard help shown under the pages table.
const KEYBOARD_HINT: &str = "Navigation : <kbd>←</kbd>/<kbd>→</kbd> page précédente/suivante · \
<kbd>Entrée</kbd> plein écran · <kbd>Échap</kbd> fermer · \
<kbd>+</kbd>/<kbd>−</kbd> taille des aperçus · <kbd>0</kbd> taille par défaut";

/// Viewer markup and the inline script wiring the keyboard navigation. Single
/// quotes and `{}` are avoided so the snippet stays format!-safe.
const VIEWER_MARKUP: &str = "\
<div id=\"page-viewer\" title=\"Cliquez pour fermer\"><img alt=\"\">\
<div class=\"viewer-label\"></div></div>\n\
<p class=\"kbd-hint\">…HINT…</p>\n\
<script>…SCRIPT…</script>\n";

/// Inline script powering the keyboard navigation. Kept dependency-free and
/// embedded verbatim into the self-contained report.
const VIEWER_SCRIPT: &str = "\
var ROWS = document.querySelectorAll('table.pages tbody tr');\
var IDX = 0;\
var SCALE = 1;\
var STEP = 1.25;\
var MAX = 4;\
var MIN = 0.45;\
var viewer = document.getElementById('page-viewer');\
var viewerImage = viewer.querySelector('img');\
var viewerLabel = viewer.querySelector('.viewer-label');\
function applyScale(){\
  document.documentElement.style.setProperty('--thumb-h',(72*SCALE)+'px');\
}\
function show(row,scroll){\
  if(!row)return;\
  var previous=document.querySelector('table.pages tr.selected');\
  if(previous)previous.classList.remove('selected');\
  row.classList.add('selected');\
  IDX=Array.prototype.indexOf.call(ROWS,row);\
  if(scroll&&row.scrollIntoView)row.scrollIntoView({block:'nearest'});\
}\
function applyViewerZoom(){\
  var base=parseFloat(viewerImage.getAttribute('data-base-scale')||'1');\
  viewerImage.style.transform='scale('+(base*SCALE)+')';\
}\
function openViewer(){\
  var row=ROWS[IDX];\
  if(!row)return;\
  var before=row.querySelector('td.thumbs a img');\
  if(!before)return;\
  viewerImage.setAttribute('src',before.getAttribute('src'));\
  viewerImage.setAttribute('data-base-scale',1);\
  viewerImage.style.transform='';\
  viewerLabel.textContent='Page '+(IDX+1)+' — '+(row.querySelector('td:nth-child(2)').textContent)+' (avant traduction)';\
  viewer.classList.add('open');\
}\
function closeViewer(){viewer.classList.remove('open');}\
function toggleViewerImage(){\
  var row=ROWS[IDX];\
  if(!row)return;\
  var links=row.querySelectorAll('td.thumbs a');\
  if(!links.length)return;\
  var first=links[0].querySelector('img');\
  var index=(first&&first.getAttribute('src')===viewerImage.getAttribute('src'))?1:0;\
  var next=links[index];\
  if(next&&next.querySelector('img')){\
    viewerImage.setAttribute('src',next.querySelector('img').getAttribute('src'));\
  }\
}\
document.addEventListener('keydown',function(event){\
  if(!ROWS.length)return;\
  var isOpen=viewer.classList.contains('open');\
  switch(event.key){\
    case 'ArrowDown':\
      if(isOpen){return;}\
      show(ROWS[Math.min(IDX+1,ROWS.length-1)],true);event.preventDefault();break;\
    case 'ArrowUp':\
      if(isOpen){return;}\
      show(ROWS[Math.max(IDX-1,0)],true);event.preventDefault();break;\
    case 'ArrowRight':\
      if(isOpen){toggleViewerImage();}\
      else{show(ROWS[Math.min(IDX+1,ROWS.length-1)],true);}\
      event.preventDefault();break;\
    case 'ArrowLeft':\
      if(isOpen){toggleViewerImage();}\
      else{show(ROWS[Math.max(IDX-1,0)],true);}\
      event.preventDefault();break;\
    case 'Enter':\
      if(isOpen){closeViewer();}\
      else{openViewer();}\
      event.preventDefault();break;\
    case 'Escape':\
      if(isOpen){closeViewer();event.preventDefault();}break;\
    case '+':\
    case '=':\
      if(SCALE*STEP<=MAX)SCALE*=STEP;applyScale();applyViewerZoom();event.preventDefault();break;\
    case '-':\
      if(SCALE/STEP>=MIN)SCALE/=STEP;applyScale();applyViewerZoom();event.preventDefault();break;\
    case '0':\
      SCALE=1;applyScale();applyViewerZoom();event.preventDefault();break;\
  }\
});\
viewer.addEventListener('click',closeViewer);\
show(ROWS[0],false);";

/// Final viewer snippet: keyboard hint plus script, ready for the template.
fn viewer_snippet() -> String {
    VIEWER_MARKUP
        .replace("…HINT…", KEYBOARD_HINT)
        .replace("…SCRIPT…", VIEWER_SCRIPT)
}

/// One `<img>` cell with a click-to-zoom link, or a dash when absent.
fn thumbnail_cell(label: &str, data_uri: &str) -> String {
    format!(
        "<a href=\"{data_uri}\" target=\"_blank\">\
<img src=\"{data_uri}\" alt=\"{label}\" title=\"{label}\" loading=\"lazy\"></a>"
    )
}

/// Renders the run report as a self-contained HTML document.
#[must_use]
pub fn to_html(report: &RunReport) -> String {
    let (translated, skipped, failed) = report.counts();
    let mut rows = String::new();
    for page in &report.pages {
        let (status_class, status) = match &page.outcome {
            PageOutcome::Translated { .. } => ("ok", "traduite"),
            PageOutcome::Skipped => ("skipped", "ignorée"),
            PageOutcome::Failed(_) => ("failed", "échec"),
        };
        // Status comes from this run, the row's data from what the state
        // remembers: a skipped page keeps the original run's numbers and
        // before/after previews instead of showing an empty row.
        let (elapsed, vram_peak, stages_detail, thumbs) = match page.display_outcome() {
            PageOutcome::Translated {
                elapsed,
                stages,
                thumbnails,
                vram_peak,
            } => (
                seconds(*elapsed),
                vram_peak.clone().unwrap_or_else(|| "—".to_owned()),
                html_escape(&stage_seconds(stages)),
                match thumbnails {
                    Some(thumbnails) => format!(
                        "{} {}",
                        thumbnail_cell("Avant", &thumbnails.before),
                        thumbnail_cell("Après", &thumbnails.after)
                    ),
                    None => "—".to_owned(),
                },
            ),
            PageOutcome::Skipped => (
                "—".to_owned(),
                "—".to_owned(),
                "sortie déjà présente".to_owned(),
                "—".to_owned(),
            ),
            PageOutcome::Failed(error) => (
                "—".to_owned(),
                "—".to_owned(),
                html_escape(&error.replace('\n', " ")),
                "—".to_owned(),
            ),
        };
        rows.push_str(&format!(
            "<tr class=\"{status_class}\" style=\"height:{DEFAULT_THUMB_HEIGHT}px\"><td>{}</td><td><code>{}</code></td>\
<td class=\"status-{status_class}\">{}</td><td>{elapsed}</td><td>{vram_peak}</td>\
<td>{stages_detail}</td><td class=\"thumbs\">{thumbs}</td></tr>",
            page.index + 1,
            html_escape(&page.name),
            status,
        ));
    }
    let vram = match (&report.vram_estimate, &report.vram_peak) {
        (Some(estimate), Some(peak)) => format!("{estimate} / pic réel {peak}"),
        (Some(estimate), None) => estimate.clone(),
        (None, Some(peak)) => format!("pic réel {peak}"),
        (None, None) => "inconnue".to_owned(),
    };
    format!(
        "<!doctype html>\n<html lang=\"fr\">\n<head>\n<meta charset=\"utf-8\">\n\
<title>Koharu batch — {input}</title>\n\
<meta name=\"description\" content=\"Rapport de traduction Koharu — navigation clavier : flèches, Entrée, +/−, 0\">\n\
<style>{REPORT_STYLES}</style>\n</head>\n<body>\n\
<h1>Rapport de traduction — Koharu batch</h1>\n\
<table class=\"summary\">\n\
<tr><td>Entrée</td><td><code>{input}</code></td></tr>\n\
<tr><td>Sortie</td><td><code>{output}</code></td></tr>\n\
<tr><td>Langue cible</td><td>{language}</td></tr>\n\
<tr><td>Modèle</td><td><code>{model}</code> (<code>{quantization}</code>)</td></tr>\n\
<tr><td>VRAM</td><td>{vram}</td></tr>\n\
<tr><td>Périphérique</td><td>{device}</td></tr>\n\
<tr><td>Démarré</td><td>{started}</td></tr>\n\
<tr><td>Durée totale</td><td>{total:.1}s</td></tr>\n\
<tr><td>Pages</td><td>{pages_line}</td></tr>\n\
</table>\n\
<table class=\"pages\">\n\
<thead><tr><th>#</th><th>Page</th><th>Statut</th><th>Durée</th><th>Pic VRAM</th><th>Détail</th><th>Aperçu</th></tr></thead>\n\
<tbody>\n{rows}\
</tbody>\n\
</table>\n\n{snippet}\n\
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
        snippet = viewer_snippet(),
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
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
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
                        thumbnails: Some(Thumbnails {
                            before: "data:image/jpeg;base64,QkVGT1JF".to_owned(),
                            after: "data:image/jpeg;base64,QUBFSF0".to_owned(),
                        }),
                        vram_peak: Some("5.9 GiB".to_owned()),
                    },
                    previous: None,
                },
                PageReport::skipped(1, "002.png"),
                PageReport::failed(2, "010.png", "translation failed: model timeout"),
            ],
            total_elapsed: Duration::from_millis(15_000),
            model: "gemma4-e4b-uncensored".to_owned(),
            quantization: "Q4_K_P".to_owned(),
            vram_estimate: Some("5.6 GiB (measured)".to_owned()),
            vram_peak: Some("6.2 GiB".to_owned()),
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
    fn vram_peak_shows_beside_the_estimate() {
        let markdown = to_markdown(&sample());
        assert!(markdown.contains("| VRAM (estimée / pic réel) | 5.6 GiB (measured) / 6.2 GiB |"));
        // Translated rows carry their own peak; failed/skipped rows show a dash.
        assert!(markdown.contains("| ✅ traduite | 4.2s | 5.9 GiB |"));
        assert_eq!(markdown.matches("| ⏭️ ignorée | — | — |").count(), 1);
        let html = to_html(&sample());
        assert!(html.contains("5.6 GiB (measured) / pic réel 6.2 GiB"));
        assert!(html.contains("Pic VRAM"));
    }

    #[test]
    fn html_is_self_contained_and_escapes() {
        let html = to_html(&sample());
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("<style>"));
        // Exactly one inline script: the keyboard-navigation viewer.
        assert_eq!(html.matches("<script>").count(), 1);
        assert!(!html.contains("<script src="));
        assert!(html.contains("gemma4-e4b-uncensored"));
        assert!(html.contains("tr class=\"failed\""));
        assert!(html.contains("tr class=\"skipped\""));
    }

    #[test]
    fn html_embeds_keyboard_navigation_and_scale() {
        let html = to_html(&sample());
        // Viewer overlay, keyboard hint, and the script wiring.
        assert!(html.contains("id=\"page-viewer\""));
        assert!(html.contains("kbd-hint"));
        assert!(html.contains("ArrowRight"));
        assert!(html.contains("ArrowLeft"));
        assert!(html.contains("--thumb-h"));
        assert!(html.contains("scrollIntoView"));
        // Rows and header live in explicit thead/tbody so the script can
        // target page rows only.
        assert!(html.contains("<thead><tr><th>#</th>"));
        assert!(html.contains("<tbody>"));
        // Scale bounds are materialized in the script.
        assert!(html.contains("var MAX = 4"));
        assert!(html.contains("var MIN = 0.45"));
    }

    #[test]
    fn html_embeds_thumbnails_as_data_uris() {
        let html = to_html(&sample());
        assert!(html.contains("src=\"data:image/jpeg;base64,QkVGT1JF\""));
        assert!(html.contains("src=\"data:image/jpeg;base64,QUBFSF0\""));
        assert!(html.contains("Aperçu"));
        // Failed and skipped rows carry no thumbnails.
        assert_eq!(html.matches("<td class=\"thumbs\">—</td>").count(), 2);
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

    fn remembered_translated() -> PageOutcome {
        PageOutcome::Translated {
            elapsed: Duration::from_millis(3210),
            stages: vec![("detection".to_owned(), Duration::from_millis(500))],
            thumbnails: Some(Thumbnails {
                before: "data:image/jpeg;base64,QUJPUkVf".to_owned(),
                after: "data:image/jpeg;base64,QVJFVEVS".to_owned(),
            }),
            vram_peak: Some("6.0 GiB".to_owned()),
        }
    }

    #[test]
    fn a_skipped_row_displays_the_run_that_translated_it() {
        let mut report = sample();
        report.pages[1] = PageReport::skipped(1, "002.png").remembering(&[PageReport {
            index: 1,
            name: "002.png".to_owned(),
            outcome: remembered_translated(),
            previous: None,
        }]);

        let markdown = to_markdown(&report);
        // Status of this run, numbers of the original one.
        assert!(
            markdown.contains("| ⏭️ ignorée | 3.2s | 6.0 GiB |"),
            "{markdown}"
        );
        assert!(markdown.contains("detection 0.5s"));
        assert!(
            markdown.contains("1 traduite, 1 ignorée, 1 en échec"),
            "the counts stay this run's own"
        );

        let html = to_html(&report);
        assert!(html.contains("tr class=\"skipped\""));
        assert!(
            html.contains("src=\"data:image/jpeg;base64,QUJPUkVf\""),
            "the original run's thumbnails stay in the rewritten report"
        );
        assert!(html.contains("src=\"data:image/jpeg;base64,QVJFVEVS\""));
    }

    #[test]
    fn a_renamed_page_is_never_paired_with_stale_data() {
        let history = [PageReport {
            index: 0,
            name: "p1.png".to_owned(),
            outcome: remembered_translated(),
            previous: None,
        }];
        assert!(
            PageReport::skipped(0, "p1.png")
                .remembering(&history)
                .previous
                .is_some()
        );
        assert!(
            PageReport::skipped(0, "renamed.png")
                .remembering(&history)
                .previous
                .is_none()
        );
        assert!(
            PageReport::skipped(1, "p1.png")
                .remembering(&history)
                .previous
                .is_none()
        );
    }

    #[test]
    fn the_state_lives_beside_the_report_and_roundtrips() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("Vol")).unwrap();
        let base = directory.path().join("Vol").join("Ch1.report");
        assert_eq!(
            state_path(&base),
            directory.path().join("Vol/Ch1.report.state.json")
        );
        assert!(load_state(&base).is_none(), "no state yet");

        let pages = sample().pages;
        save_state(&base, &pages, &[]).unwrap();
        assert_eq!(load_state(&base).unwrap(), pages);
    }

    #[test]
    fn the_state_keeps_pages_this_run_left_out() {
        let pages = sample().pages;
        // A `--pages` run knows only its first row; the earlier state's other
        // rows must survive so a later full resume still remembers them.
        let merged = merge_state(&pages[..1], &pages);
        assert_eq!(
            merged.iter().map(|row| row.index).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(merged[1], pages[1]);
    }
}
