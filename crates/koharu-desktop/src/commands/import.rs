//! Pages from files: images, ZIP, CBZ, RAR, and CBR archives of them, and PDFs
//! rendered with hayro, in name order. Each becomes a file of the project and
//! a page made from it.

use std::{
    cell::RefCell,
    io::{self, Read as _, Write},
    path::{Path, PathBuf},
    rc::Rc,
    sync::{Arc, LazyLock},
};

use anyhow::{Context as _, bail};
use fs_err::File;
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use hayro::{
    PixmapSettings, RenderCache, RenderSettings, hayro_interpret::InterpreterSettings,
    hayro_syntax::Pdf, render, vello_cpu::color::palette::css::WHITE,
};
use image::ImageFormat;
use koharu_scene::{Edit, Hash, Op};
use rars::ArchiveReader;
use rayon::prelude::*;
use serde::Deserialize;
use specta::Type;
use tauri::{State, Window};
use tauri_runtime_cef::CefRuntime;

use super::Result;
use crate::app::App;

#[derive(Clone, Copy, Debug, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ImportSource {
    Files,
    /// Every page file directly in a folder.
    Folder,
}

/// The page files the file dialog offers.
const EXTENSIONS: [&str; 9] = [
    "png", "jpg", "jpeg", "webp", "zip", "cbz", "rar", "cbr", "pdf",
];

/// PDF points to pixels at 300 DPI, which keeps fine lettering and line art.
/// An A4 page is about 33 MiB of RGBA at this scale; one page renders at a
/// time, so its buffer goes before the next.
const PDF_SCALE: f32 = 300.0 / 72.0;

/// The files an OS leaves in a folder it archives, which aren't pages:
/// macOS's resource forks and Finder settings, and Windows's thumbnail cache
/// and folder settings.
static METADATA: LazyLock<GlobSet> = LazyLock::new(|| {
    let mut set = GlobSetBuilder::new();
    for pattern in [
        "**/__MACOSX/**",
        "**/._*",
        "**/.DS_Store",
        "**/Thumbs.db",
        "**/desktop.ini",
    ] {
        set.add(
            GlobBuilder::new(pattern)
                .case_insensitive(true)
                .literal_separator(true)
                .build()
                .expect("the patterns are globs"),
        );
    }
    set.build().expect("the patterns are globs")
});

/// Whether a file or an archive member's name is an image a page can be.
fn raster(name: &Path) -> bool {
    matches!(
        ImageFormat::from_path(name),
        Ok(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    )
}

/// Reads a page file's pages: their images, encoded, in order.
type Reader = fn(&Path) -> anyhow::Result<Vec<Vec<u8>>>;

/// An archive's images by member name.
type Members = Vec<(String, Vec<u8>)>;

/// How a page file's pages are read, or `None` when it holds none.
fn reader(path: &Path) -> Option<Reader> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "zip" | "cbz" => Some(extract_zip),
        "rar" | "cbr" => Some(extract_rar),
        "pdf" => Some(render_pdf),
        _ if raster(path) => Some(|path| Ok(vec![fs_err::read(path)?])),
        _ => None,
    }
}

/// The page files directly in `folder`, in name order, numbers by value.
fn page_files(folder: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs_err::read_dir(folder)?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| path.is_file() && reader(path).is_some())
        .collect();
    alphanumeric_sort::sort_slice_by_os_str_key(&mut paths, |path| {
        path.file_name().unwrap_or(path.as_os_str())
    });
    Ok(paths)
}

/// The pages `paths` holds, an archive's or a PDF's own pages included, read
/// side by side.
fn read_pages(paths: &[PathBuf]) -> anyhow::Result<Vec<Vec<u8>>> {
    Ok(paths
        .par_iter()
        .map(|path| {
            let read = reader(path).with_context(|| {
                format!("{} isn't an image, an archive, or a PDF", path.display())
            })?;
            read(path)
        })
        .collect::<anyhow::Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect())
}

/// Appends `pages` after the open project's last, as one edit.
fn insert(app: &Arc<App>, pages: Vec<Vec<u8>>) -> anyhow::Result<()> {
    let session = app.session()?;
    let start = session.lock().project().pages().count();
    let mut ops = Vec::with_capacity(pages.len() * 2);
    for (index, bytes) in pages.into_iter().enumerate() {
        let background = Some(Hash::of(&bytes));
        ops.push(Op::AddFile { bytes });
        ops.push(Op::AddPage {
            id: None,
            index: start + index,
            size: None,
            background,
        });
    }
    app.edit(Edit {
        label: "import".to_owned(),
        ops,
    })?;
    Ok(())
}

/// Adds `paths`' pages to the open project, off the async runtime.
async fn add(app: &State<'_, Arc<App>>, paths: Vec<PathBuf>) -> Result<()> {
    let app = Arc::clone(app);
    Ok(tokio::task::spawn_blocking(move || {
        let pages = read_pages(&paths)?;
        insert(&app, pages)
    })
    .await??)
}

/// Picks files or a folder and adds their pages after the project's last: the
/// files in name order, numbers by value, an archive's or a PDF's own pages in
/// theirs.
#[tauri::command]
#[specta::specta]
pub async fn import(
    app: State<'_, Arc<App>>,
    window: Window<CefRuntime>,
    source: ImportSource,
) -> Result<()> {
    let dialog = rfd::AsyncFileDialog::new().set_parent(&window);
    let paths: Vec<PathBuf> = match source {
        ImportSource::Files => match dialog.add_filter("Pages", &EXTENSIONS).pick_files().await {
            Some(files) => files.iter().map(|file| file.path().to_owned()).collect(),
            None => return Ok(()),
        },
        ImportSource::Folder => match dialog.pick_folder().await {
            Some(folder) => page_files(folder.path())?,
            None => return Ok(()),
        },
    };
    add(&app, paths).await
}

/// A folder of pages as a project of its own: named after the folder, created,
/// then filled with the folder's pages. A dismissed dialog creates nothing.
#[tauri::command]
#[specta::specta]
pub(crate) async fn open_folder(
    app: State<'_, Arc<App>>,
    window: Window<CefRuntime>,
) -> Result<Option<String>> {
    let dialog = rfd::AsyncFileDialog::new().set_parent(&window);
    let Some(folder) = dialog.pick_folder().await else {
        return Ok(None);
    };
    let folder = folder.path().to_owned();
    let app = Arc::clone(&app);
    Ok(Some(
        tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
            let name = app.library.folder_name(&folder);
            let session = app.library.create(&name)?;
            app.open(name.clone(), session)?;
            let paths = page_files(&folder)?;
            if paths.is_empty() {
                app.library.delete(&name)?;
                bail!("{} holds no pages", folder.display());
            }
            let pages = read_pages(&paths)?;
            insert(&app, pages)?;
            Ok(name)
        })
        .await??,
    ))
}

/// An archive's images in name order, numbers by value.
fn in_name_order(path: &Path, mut images: Members) -> anyhow::Result<Vec<Vec<u8>>> {
    if images.is_empty() {
        bail!("{} holds no images", path.display());
    }
    alphanumeric_sort::sort_slice_by_str_key(&mut images, |(name, _)| name);
    Ok(images.into_iter().map(|(_, bytes)| bytes).collect())
}

/// A ZIP or CBZ archive's images.
fn extract_zip(path: &Path) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut archive = zip::ZipArchive::new(File::open(path)?)
        .with_context(|| format!("failed to read the archive {}", path.display()))?;
    let mut images = Vec::new();
    for index in 0..archive.len() {
        let mut member = archive
            .by_index(index)
            .with_context(|| format!("failed to read member {index} of {}", path.display()))?;
        if member.is_dir() {
            continue;
        }
        let name = member
            .name()
            .with_context(|| format!("failed to read member {index}'s name in {}", path.display()))?
            .into_owned();
        if !raster(Path::new(&name)) || METADATA.is_match(&name) {
            continue;
        }
        let mut bytes = Vec::new();
        member
            .read_to_end(&mut bytes)
            .with_context(|| format!("failed to read {name:?} in {}", path.display()))?;
        images.push((name, bytes));
    }
    in_name_order(path, images)
}

/// Writes into the RAR member opened last, which is the one being extracted:
/// `extract_to` writes each member whole before it opens the next.
struct LastMember(Rc<RefCell<Members>>);

impl Write for LastMember {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.0
            .borrow_mut()
            .last_mut()
            .expect("a member is opened before it's written")
            .1
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A RAR or CBR archive's images.
fn extract_rar(path: &Path) -> anyhow::Result<Vec<Vec<u8>>> {
    let archive = ArchiveReader::read_path(path)
        .with_context(|| format!("failed to open the archive {}", path.display()))?;
    let images = Rc::new(RefCell::new(Vec::new()));
    archive
        .extract_to(None, |entry| {
            let name = entry.name_lossy();
            let member = Path::new(&name);
            if entry.is_directory || !raster(member) || METADATA.is_match(member) {
                return Ok(Box::new(io::sink()));
            }
            images.borrow_mut().push((name, Vec::new()));
            Ok(Box::new(LastMember(Rc::clone(&images))))
        })
        .with_context(|| format!("failed to extract {}", path.display()))?;
    in_name_order(path, images.take())
}

/// A PDF's pages, each rendered with hayro.
fn render_pdf(path: &Path) -> anyhow::Result<Vec<Vec<u8>>> {
    let pdf = Pdf::new(fs_err::read(path)?)
        .map_err(|error| anyhow::anyhow!("failed to parse {}: {error:?}", path.display()))?;
    let pages = pdf.pages();
    if pages.is_empty() {
        bail!("{} has no pages", path.display());
    }
    let (interpreter, settings, cache) = (
        InterpreterSettings::default(),
        RenderSettings::default(),
        RenderCache::new(),
    );
    let pixmap = PixmapSettings {
        x_scale: PDF_SCALE,
        y_scale: PDF_SCALE,
        bg_color: WHITE,
    };
    pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            render(page, &cache, &interpreter, &settings, &pixmap)
                .into_png()
                .with_context(|| {
                    format!("failed to encode page {} of {}", index + 1, path.display())
                })
        })
        .collect()
}
