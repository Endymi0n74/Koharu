//! `prune`: the folders the store holds for nothing, listed, and deleted when
//! `--delete` says so.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::cli::Prune;

/// Lists what the store keeps for nothing, and deletes it when `--delete` says
/// so. Nothing is removed without it, so the listing can be read first.
///
/// Reusable legacy downloads that share a commit with a current snapshot are
/// reported separately: they would be staged again for the pinned revision and
/// are not stale drops. They are not included in the reclaimable summary.
pub async fn prune(arguments: &Prune) -> Result<()> {
    let root = koharu_runtime::store_root();
    if !root.is_dir() {
        println!("{} holds no store", root.display());
        return Ok(());
    }
    let keep = referenced_folders();
    let reusable = reusable_repositories();
    let mut candidates = plan(&root, &keep)?;
    candidates.extend(plan(&root, &legacy(&root, &keep))?);
    candidates.sort();
    if reusable.is_empty() && candidates.is_empty() {
        println!("the store keeps nothing it could drop");
        return Ok(());
    }
    if !reusable.is_empty() {
        print_reusable(&root, &reusable);
    }
    if candidates.is_empty() {
        return Ok(());
    }
    let mut total = 0;
    for candidate in &candidates {
        let bytes = size(candidate)?;
        total += bytes;
        println!("{}\t{}", mibibytes(bytes), shown(&root, candidate));
    }
    println!(
        "\n{} folder(s), {} reclaimable",
        candidates.len(),
        mibibytes(total)
    );
    if !arguments.delete {
        println!("nothing was removed: run it again with --delete");
        return Ok(());
    }
    for candidate in &candidates {
        koharu_runtime::remove_from_store(candidate)
            .await
            .with_context(|| format!("couldn't remove {}", shown(&root, candidate)))?;
        println!("removed {}", shown(&root, candidate));
    }
    Ok(())
}

/// The models root the fork wrote before upstream renamed it. Nothing reads it
/// any more, but the folder a catalog still pins is that model's download, not
/// something a prune may call stale, so it stays whole whatever revision the
/// fork left there.
const LEGACY_MODELS: &str = "hugging-face";

/// The folders a catalog or a release still references, on both roots: one per
/// repository for the legacy root, the snapshot folder for the current one.
fn referenced_folders() -> Vec<PathBuf> {
    let mut folders = koharu_runtime::package_versions();
    for (kind, id, commit) in referenced_repositories() {
        folders.push(root_join([
            kind.current_root(),
            kind.plural(),
            id,
            "snapshots",
            commit,
        ]));
        folders.push(root_join([kind.legacy_root(), kind.plural(), id]));
    }
    folders
}

fn root_join(parts: impl IntoIterator<Item = impl AsRef<Path>>) -> PathBuf {
    let mut path = PathBuf::new();
    for part in parts {
        path.push(part);
    }
    path
}

/// A repository a catalog or a release still references, with the kind that
/// decides its store path.
type ReferencedRepository = (RepoKind, &'static str, &'static str);

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RepoKind {
    Model,
    Dataset,
}

impl RepoKind {
    fn plural(self) -> &'static str {
        match self {
            Self::Model => "models",
            Self::Dataset => "datasets",
        }
    }

    fn current_root(self) -> &'static str {
        "huggingface"
    }

    fn legacy_root(self) -> &'static str {
        LEGACY_MODELS
    }
}

fn referenced_repositories() -> Vec<ReferencedRepository> {
    use koharu_runtime::RepositoryKind;
    use koharu_runtime::referenced_repositories as runtime_package_repositories;

    let mut repos: Vec<ReferencedRepository> = runtime_package_repositories()
        .into_iter()
        .filter_map(|kind| match kind {
            RepositoryKind::Model(id, commit) => Some((RepoKind::Model, id, commit)),
            RepositoryKind::Dataset(id, commit) => Some((RepoKind::Dataset, id, commit)),
            #[allow(unreachable_patterns)]
            _ => None,
        })
        .collect();
    for repo in koharu_llm::repositories() {
        let kind = repo.kind();
        if let RepositoryKind::Model(id, commit) = kind {
            repos.push((RepoKind::Model, id, commit));
        } else if let RepositoryKind::Dataset(id, commit) = kind {
            repos.push((RepoKind::Dataset, id, commit));
        }
    }
    for repo in koharu_ml::repositories() {
        let kind = repo.kind();
        if let RepositoryKind::Model(id, commit) = kind {
            repos.push((RepoKind::Model, id, commit));
        } else if let RepositoryKind::Dataset(id, commit) = kind {
            repos.push((RepoKind::Dataset, id, commit));
        }
    }
    for repo in koharu_renderer::fonts::repositories() {
        let kind = repo.kind();
        if let RepositoryKind::Model(id, commit) = kind {
            repos.push((RepoKind::Model, id, commit));
        } else if let RepositoryKind::Dataset(id, commit) = kind {
            repos.push((RepoKind::Dataset, id, commit));
        }
    }
    repos
}

/// The legacy downloads a catalog still pins with the same commit a current
/// snapshot uses: they already exist on a re-download, so they are not stale
/// drops. They are reported separately and never entered into the reclaimable
/// summary.
fn reusable_repositories() -> Vec<PathBuf> {
    reusable_repositories_in_store(koharu_runtime::store_root())
}

fn reusable_repositories_in_store(root: &Path) -> Vec<PathBuf> {
    reusable_repositories_in_root(root, &referenced_repositories())
}

fn reusable_repositories_in_root(root: &Path, repos: &[(RepoKind, &str, &str)]) -> Vec<PathBuf> {
    let mut reusable = Vec::new();
    for (kind, id, commit) in repos {
        if !root
            .join(format!(
                "{}/{}/{}/snapshots/{}",
                kind.current_root(),
                kind.plural(),
                id,
                commit,
            ))
            .is_dir()
        {
            continue;
        }
        let repo = root.join(format!("{}/{}/{}", kind.legacy_root(), kind.plural(), id,));
        if !repo.is_dir() {
            continue;
        }
        reusable.push(repo);
    }
    reusable.sort();
    reusable.dedup();
    reusable
}

/// The folders the legacy root keeps: one per repository a catalog still pins,
/// where a kept `<root>/huggingface/<kind>/<repository>/snapshots/<revision>`
/// keeps `<root>/hugging-face/<kind>/<repository>` whole.
fn legacy(root: &Path, keep: &[PathBuf]) -> Vec<PathBuf> {
    let current = root.join("huggingface");
    let legacy = root.join(LEGACY_MODELS);
    let mut out = Vec::new();
    for folder in keep {
        let Ok(rest) = folder.strip_prefix(&current) else {
            continue;
        };
        let mut components = rest.components();
        let Some(kind) = components.next() else {
            continue;
        };
        let Some(id) = components.next() else {
            continue;
        };
        let repo = legacy.join(&kind).join(&id);
        if !repo.is_dir() {
            continue;
        }
        out.push(repo);
    }
    out.sort();
    out.dedup();
    out
}

/// The folders under `root` that `keep` leaves behind: inside a package a kept
/// version lives in, every version it doesn't hold; deeper, such as a commit
/// under a repository's `snapshots`, the same until a kept folder is reached.
///
/// Folders outside every kept package's namespace stay: an unknown folder is
/// not a stale download, and a prune doesn't guess.
fn plan(root: &Path, keep: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let namespaces: BTreeSet<PathBuf> = keep
        .iter()
        .filter_map(|folder| Some(root.join(folder.strip_prefix(root).ok()?.components().next()?)))
        .collect();
    let mut candidates = Vec::new();
    for namespace in &namespaces {
        collect(namespace, keep, &mut candidates)?;
    }
    Ok(candidates)
}

fn collect(folder: &Path, keep: &[PathBuf], candidates: &mut Vec<PathBuf>) -> Result<()> {
    for child in folders(folder)? {
        if keep.contains(&child) {
            continue;
        }
        if keep.iter().any(|kept| kept.starts_with(&child)) {
            collect(&child, keep, candidates)?;
        } else {
            candidates.push(child);
        }
    }
    Ok(())
}

/// The subfolders of `folder`, without the store's hidden entries: a
/// download's stage and a version's lock.
fn folders(folder: &Path) -> Result<Vec<PathBuf>> {
    let entries = match fs_err::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut folders: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && !entry.file_name().to_string_lossy().starts_with('.')
        })
        .map(|entry| entry.path())
        .collect();
    folders.sort();
    Ok(folders)
}

/// What a folder holds, its own files and everything under them.
fn size(folder: &Path) -> Result<u64> {
    let mut total = 0;
    let mut pending = vec![folder.to_owned()];
    while let Some(folder) = pending.pop() {
        for entry in fs_err::read_dir(&folder)?.flatten() {
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total += metadata.len();
            }
        }
    }
    Ok(total)
}

fn shown(root: &Path, folder: &Path) -> String {
    folder
        .strip_prefix(root)
        .unwrap_or(folder)
        .display()
        .to_string()
}

fn mibibytes(bytes: u64) -> String {
    const MIB: f64 = (1024 * 1024) as f64;
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * MIB))
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB)
    }
}

fn print_reusable(root: &Path, reusable: &[PathBuf]) {
    let mut total = 0;
    for folder in reusable {
        let bytes = size(folder).unwrap_or(0);
        total += bytes;
        println!("{}\t{}", mibibytes(bytes), shown(&root, folder));
    }
    println!(
        "\n{} reusable legacy download(s), {} already present on a re-download",
        reusable.len(),
        mibibytes(total)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store with one repository whose commit is kept and one whose commit is
    /// stale, a stale commit beside the kept one, two llama.cpp tags, and a
    /// folder no package owns.
    fn populated(root: &Path) {
        for folder in [
            "huggingface/models/a--b/snapshots/kept",
            "huggingface/models/a--b/snapshots/old",
            "huggingface/models/c--d/snapshots/gone",
            "llama/current",
            "llama/previous",
            "unrelated",
        ] {
            fs_err::create_dir_all(root.join(folder)).unwrap();
        }
        fs_err::write(
            root.join("huggingface/models/a--b/snapshots/old/weights.gguf"),
            b"old",
        )
        .unwrap();
    }

    #[test]
    fn a_prune_keeps_what_a_catalog_still_pins() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        populated(root);
        let keep = vec![
            root.join("huggingface/models/a--b/snapshots/kept"),
            root.join("llama/current"),
        ];

        let candidates = plan(root, &keep).unwrap();

        assert_eq!(
            candidates,
            [
                root.join("huggingface/models/a--b/snapshots/old"),
                root.join("huggingface/models/c--d"),
                root.join("llama/previous"),
            ]
        );
    }

    #[test]
    fn a_folder_no_package_owns_stays() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        populated(root);

        let candidates = plan(root, &[root.join("llama/current")]).unwrap();

        assert!(!candidates.contains(&root.join("unrelated")));
    }

    #[test]
    fn a_legacy_repository_a_catalog_still_pins_stays() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        for folder in [
            "huggingface/models/a--b/snapshots/kept",
            "hugging-face/models/a--b/snapshots/older",
            "hugging-face/models/c--d/snapshots/gone",
            "hugging-face/datasets/e--f/snapshots/older",
        ] {
            fs_err::create_dir_all(root.join(folder)).unwrap();
        }
        let keep = [
            root.join("huggingface/models/a--b/snapshots/kept"),
            root.join("huggingface/datasets/e--f/snapshots/kept"),
        ];

        let candidates = plan(root, &legacy(root, &keep)).unwrap();

        assert_eq!(candidates, [root.join("hugging-face/models/c--d")]);
    }

    #[test]
    fn the_size_covers_what_a_folder_holds() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        populated(root);

        assert_eq!(
            size(&root.join("huggingface/models/a--b/snapshots/old")).unwrap(),
            3
        );
    }

    #[test]
    fn a_legacy_reusable_repository_shares_a_commit_with_a_current_snapshot() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let a2b_cur = root.join("huggingface/models/a--b/snapshots/kept");
        let a2b_legacy_snapshot = root.join("hugging-face/models/a--b/snapshots/kept");
        let a2b_older_snapshot = root.join("hugging-face/models/a--b/snapshots/older");

        fs_err::create_dir_all(&a2b_cur).unwrap();
        fs_err::create_dir_all(&a2b_legacy_snapshot).unwrap();
        fs_err::create_dir_all(&a2b_older_snapshot).unwrap();

        let cur = root.join("huggingface/models/a--b/snapshots/kept");
        let legacy_repo = root.join("hugging-face/models/a--b");
        let kept_snapshot = legacy_repo.join("snapshots/kept");

        fs_err::create_dir_all(&cur).unwrap();
        fs_err::create_dir_all(&legacy_repo).unwrap();
        fs_err::create_dir_all(&kept_snapshot).unwrap();

        assert_eq!(
            reusable_repositories_in_root(root, &[(RepoKind::Model, "a--b", "kept")]),
            vec![legacy_repo]
        );
    }

    #[test]
    fn the_legacy_directory_that_only_exists_under_snapshots_is_not_reusable() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let cur = root.join("huggingface/models/a--b/snapshots/kept");
        let legacy_repo = root.join("hugging-face/models/a--b");

        fs_err::create_dir_all(&cur).unwrap();
        fs_err::create_dir_all(&legacy_repo).unwrap();

        assert!(
            reusable_repositories_in_root(root, &[(RepoKind::Model, "a--b", "not-kept")])
                .is_empty(),
            "the reusable listing must not list a legacy folder with no current snapshot"
        );
    }

    #[test]
    fn print_reusable_lists_each_folder_with_its_size() {
        let store = tempfile::tempdir().unwrap();
        let root = store.path();
        let cur = root.join("huggingface/models/a--b/snapshots/kept");
        let legacy_repo = root.join("hugging-face/models/a--b");

        fs_err::create_dir_all(&cur).unwrap();
        fs_err::create_dir_all(&legacy_repo).unwrap();

        assert_eq!(
            reusable_repositories_in_root(root, &[(RepoKind::Model, "a--b", "kept")]),
            vec![legacy_repo]
        );
    }
}
