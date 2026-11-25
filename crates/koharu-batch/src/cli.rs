//! The command line: a chapter in, a project out, a report about either.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// `koharu-batch`: a chapter through KoharuBatch's stages, without the window.
#[derive(Debug, Parser)]
#[command(name = "koharu-batch", version, about, long_about = None)]
pub struct Arguments {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Translate a chapter: a folder of page images, one image, or a `.cbz`.
    Run(Run),
    /// Write what a project holds: Markdown, and JSON when asked.
    Report(Report),
    /// List what the store holds for nothing, and delete it with `--delete`.
    Prune(Prune),
}

#[derive(Debug, Args)]
pub struct Run {
    /// A folder of page images, in name order, an image, or a `.cbz`.
    /// Required to create a project; ignored when `--overwrite` opens one.
    #[arg(short, long, value_name = "PATH")]
    pub input: Option<PathBuf>,
    /// The project file to write, such as `chapter.khr`; it must not exist
    /// unless `--overwrite` resumes it.
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,
    /// The target language, as a tag CLDR names: `fr-FR`, `en-US`, `ja-JP`.
    #[arg(long, default_value = "fr-FR")]
    pub target: String,
    /// The translation model, `vendor/model` with an optional `:QUANTIZATION`,
    /// or `auto` for the strongest local one a VRAM budget holds (8 GiB when
    /// none is given).
    #[arg(long, default_value = "koharu/hy-mt2-7b:Q4_K_M")]
    pub translator: String,
    /// The VRAM a run may use, in MiB: a model whose estimated peak is larger
    /// is refused before any page is read.
    #[arg(long, value_name = "MIB")]
    pub vram_budget: Option<u64>,
    /// Re-translate the pages of an existing project instead of refusing it;
    /// without `--stages`, translation alone runs.
    #[arg(long)]
    pub overwrite: bool,
    /// Translate greedily (temperature 0), so rerunning the same pages renders
    /// the same text on every run.
    #[arg(long)]
    pub deterministic: bool,
    /// The layout model, by its configuration name.
    #[arg(long, default_value = "koharu-layout-highres")]
    pub detector: String,
    /// The OCR model, by its configuration name.
    #[arg(long, default_value = "hayai-ocr")]
    pub ocr: String,
    /// The inpainting model, by its configuration name.
    #[arg(long, default_value = "lama")]
    pub inpainting: String,
    /// Instructions the translator follows, in any language and form.
    #[arg(long)]
    pub instructions: Option<String>,
    /// A term the translation must reuse, `SOURCE=TARGET`; repeatable, and it
    /// wins over what the chapter's dossier learns.
    #[arg(long, value_name = "SOURCE=TARGET")]
    pub glossary: Vec<String>,
    /// The stages to run, in the order a page runs them: `detect,recognize,
    /// inpaint,translate`. All four for a new project; translation alone when
    /// `--overwrite` resumes one, unless this names stages.
    #[arg(long, value_name = "STAGES", value_delimiter = ',')]
    pub stages: Vec<String>,
    /// Pages to work on, 1-based in reading order, as `1-10,15`.
    #[arg(long, value_name = "RANGE")]
    pub pages: Option<String>,
    /// How long to wait for the run before reporting it as it is, in seconds.
    #[arg(long, default_value_t = 3_600)]
    pub wait: u64,
}

#[derive(Debug, Args)]
pub struct Prune {
    /// Delete what the listing shows instead of only showing it.
    #[arg(long)]
    pub delete: bool,
}

#[derive(Debug, Args)]
pub struct Report {
    /// The project to read, such as `chapter.khr`.
    #[arg(short, long, value_name = "PATH")]
    pub project: PathBuf,
    /// Where to write the JSON summary; the Markdown goes to stdout unless
    /// `--markdown` names a file.
    #[arg(long, value_name = "PATH")]
    pub json: Option<PathBuf>,
    /// Where to write the Markdown instead of stdout.
    #[arg(long, value_name = "PATH")]
    pub markdown: Option<PathBuf>,
}
