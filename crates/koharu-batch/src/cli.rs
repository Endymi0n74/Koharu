//! The command line: a chapter in, a project out, the way the app would run it.

use std::path::PathBuf;

use clap::Parser;

/// `koharu-batch`: a chapter through Koharu's stages, without the window.
#[derive(Debug, Parser)]
#[command(name = "koharu-batch", version, about, long_about = None)]
pub struct Arguments {
    /// A folder of page images, in name order.
    #[arg(short, long, value_name = "DIR")]
    pub input: PathBuf,
    /// The project file to write, such as `chapter.khr`.
    #[arg(short, long, value_name = "PATH")]
    pub output: PathBuf,
    /// The target language, as a tag CLDR names: `fr-FR`, `en-US`, `ja-JP`.
    #[arg(long, default_value = "fr-FR")]
    pub target: String,
    /// The translation model, `vendor/model` with an optional `:QUANTIZATION`.
    #[arg(long, default_value = "koharu/hy-mt2-7b:Q4_K_M")]
    pub translator: String,
    /// The layout model, by its configuration name.
    #[arg(long, default_value = "koharu_layout_highres")]
    pub detector: String,
    /// The OCR model, by its configuration name.
    #[arg(long, default_value = "hayai_ocr")]
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
    /// inpaint,translate`. All four by default.
    #[arg(long, value_name = "STAGES", value_delimiter = ',')]
    pub stages: Vec<String>,
    /// Pages to work on, 1-based in reading order, as `1-10,15`.
    #[arg(long, value_name = "RANGE")]
    pub pages: Option<String>,
    /// How long to wait for the run before reporting it as it is, in seconds.
    #[arg(long, default_value_t = 3_600)]
    pub wait: u64,
}
