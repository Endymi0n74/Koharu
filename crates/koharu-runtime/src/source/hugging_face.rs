use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::{Arc, LazyLock},
};

use anyhow::{Context, ensure};
use serde::Deserialize;
use tokio::sync::{Mutex, OnceCell};

use crate::{Store, downloads::Transfer, store::FileExpectation};

static REVISIONS: LazyLock<Mutex<HashMap<String, Arc<OnceCell<String>>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// A per-process, per-repository cache of one `/tree/{revision}` listing.
type TreeCache = LazyLock<Mutex<HashMap<String, Arc<OnceCell<Arc<Vec<TreeEntry>>>>>>>;
static FILE_TREES: TreeCache = LazyLock::new(|| Mutex::new(HashMap::new()));

/// One entry of the Hugging Face `/tree/{revision}` listing for a repository.
#[derive(Clone, Debug, Deserialize)]
struct TreeEntry {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "path")]
    path: String,
    size: Option<u64>,
    lfs: Option<LfsEntry>,
}

/// LFS metadata for a file; `oid` is the SHA-256 of the artifact contents.
#[derive(Clone, Debug, Deserialize)]
struct LfsEntry {
    oid: String,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum Revision<'a> {
    Pinned(&'a str),
    Latest,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RepositoryKind {
    Model,
    Dataset,
}

impl RepositoryKind {
    const fn api_route(self) -> &'static str {
        match self {
            Self::Model => "models",
            Self::Dataset => "datasets",
        }
    }

    const fn resolve_prefix(self) -> &'static str {
        match self {
            Self::Model => "",
            Self::Dataset => "datasets/",
        }
    }
}

/// An immutable file snapshot hosted by Hugging Face.
///
/// A latest file resolves the repository head once per process. Every file in
/// that repository then uses the same commit, so a model cannot be assembled
/// from different revisions if its repository changes during initialization.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HuggingFaceFile<'a> {
    kind: RepositoryKind,
    repository: &'a str,
    revision: Revision<'a>,
    filename: &'a str,
}

impl<'a> HuggingFaceFile<'a> {
    #[must_use]
    pub const fn pinned(repository: &'a str, revision: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Model,
            repository,
            revision: Revision::Pinned(revision),
            filename,
        }
    }

    #[must_use]
    pub const fn latest(repository: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Model,
            repository,
            revision: Revision::Latest,
            filename,
        }
    }

    #[must_use]
    pub const fn latest_dataset(repository: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Dataset,
            repository,
            revision: Revision::Latest,
            filename,
        }
    }

    #[must_use]
    pub const fn pinned_dataset(repository: &'a str, revision: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Dataset,
            repository,
            revision: Revision::Pinned(revision),
            filename,
        }
    }

    #[tracing::instrument(skip_all)]
    pub async fn resolve(self) -> anyhow::Result<PathBuf> {
        let mut repository = self.repository.split('/');
        let owner = repository.next().unwrap_or_default();
        let name = repository.next().unwrap_or_default();
        ensure!(
            repository.next().is_none()
                && [owner, name].into_iter().all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && part.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                        })
                }),
            "invalid Hugging Face repository {}",
            self.repository
        );
        ensure!(
            !self.filename.is_empty()
                && !self.filename.contains('\\')
                && Path::new(self.filename)
                    .components()
                    .all(|component| matches!(component, Component::Normal(_))),
            "invalid Hugging Face filename {}",
            self.filename
        );

        let revision = match self.revision {
            Revision::Pinned(revision) => {
                ensure!(is_commit(revision), "invalid pinned revision {revision}");
                revision.to_owned()
            }
            Revision::Latest => latest_revision(self.kind, self.repository).await?,
        };
        let expected = file_expectation(self.kind, self.repository, &revision, self.filename).await;
        let target = snapshot_path(self.kind, self.repository, &revision, self.filename);
        Store::file(target, expected, move |stage| async move {
            let url = format!(
                "https://huggingface.co/{}{}/resolve/{revision}/{}",
                self.kind.resolve_prefix(),
                self.repository,
                self.filename
            );
            Transfer::new()?.fetch(&url, &stage).await
        })
        .await
    }
}

async fn latest_revision(kind: RepositoryKind, repository: &str) -> anyhow::Result<String> {
    let key = format!("{}/{repository}", kind.api_route());
    let revision = {
        let mut revisions = REVISIONS.lock().await;
        revisions
            .entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    };

    revision
        .get_or_try_init(|| async {
            #[derive(Deserialize)]
            struct Repository {
                sha: String,
            }

            let url = format!(
                "https://huggingface.co/api/{}/{repository}",
                kind.api_route()
            );
            let response = Transfer::new()?
                .get(&url)
                .send()
                .await
                .with_context(|| format!("failed to resolve {repository}"))?
                .error_for_status()
                .with_context(|| format!("failed to resolve {repository}"))?
                .json::<Repository>()
                .await
                .with_context(|| format!("invalid metadata for {repository}"))?;
            ensure!(
                is_commit(&response.sha),
                "{repository} returned invalid commit {}",
                response.sha
            );
            Ok(response.sha)
        })
        .await
        .cloned()
}

/// Lists the files of `repository` at `revision`, cached once per process.
async fn tree_entries(
    kind: RepositoryKind,
    repository: &str,
    revision: &str,
) -> anyhow::Result<Arc<Vec<TreeEntry>>> {
    let key = format!("{}/{repository}/{revision}", kind.api_route());
    let cell = {
        let mut trees = FILE_TREES.lock().await;
        trees
            .entry(key)
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone()
    };

    cell.get_or_try_init(|| async {
        let url = format!(
            "https://huggingface.co/api/{}/{repository}/tree/{revision}?recursive=true",
            kind.api_route()
        );
        let response = Transfer::new()?
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to resolve {repository} file metadata"))?
            .error_for_status()
            .with_context(|| format!("failed to resolve {repository} file metadata"))?
            .json::<Vec<TreeEntry>>()
            .await
            .with_context(|| format!("invalid file metadata for {repository}"))?;
        Ok::<_, anyhow::Error>(Arc::new(response))
    })
    .await
    .cloned()
}

/// Derives the expected size and SHA-256 of `filename` from the repository
/// listing, so downloads and cached copies can be validated against them.
fn expected_for(entries: &[TreeEntry], filename: &str) -> FileExpectation {
    entries
        .iter()
        .find(|entry| entry.kind == "file" && entry.path == filename)
        .map(|entry| FileExpectation {
            size: entry.size,
            sha256: entry.lfs.as_ref().map(|lfs| lfs.oid.clone()),
        })
        .unwrap_or_default()
}

/// Fetches the expectation for `filename`, degrading to no verification when
/// the repository metadata is unavailable.
async fn file_expectation(
    kind: RepositoryKind,
    repository: &str,
    revision: &str,
    filename: &str,
) -> FileExpectation {
    match tree_entries(kind, repository, revision).await {
        Ok(entries) => expected_for(&entries, filename),
        Err(error) => {
            tracing::warn!(
                error = %error,
                "failed to resolve file metadata for {repository}/{filename}; continuing without integrity verification"
            );
            FileExpectation::default()
        }
    }
}

fn snapshot_path(
    kind: RepositoryKind,
    repository: &str,
    revision: &str,
    filename: &str,
) -> PathBuf {
    Store::root()
        .join("hugging-face")
        .join(kind.api_route())
        .join(repository.replace('/', "--"))
        .join("snapshots")
        .join(revision)
        .join(filename)
}

fn is_commit(revision: &str) -> bool {
    revision.len() == 40 && revision.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_expectations_from_tree_entries() {
        let entries = vec![
            TreeEntry {
                kind: "directory".into(),
                path: "subdir".into(),
                size: Some(0),
                lfs: None,
            },
            TreeEntry {
                kind: "file".into(),
                path: "model.gguf".into(),
                size: Some(1234),
                lfs: Some(LfsEntry {
                    oid: "ab".repeat(32),
                }),
            },
            TreeEntry {
                kind: "file".into(),
                path: "config.json".into(),
                size: Some(10),
                lfs: None,
            },
        ];

        // LFS files expose both size and content hash
        let model = expected_for(&entries, "model.gguf");
        assert_eq!(model.size, Some(1234));
        assert_eq!(model.sha256.as_deref(), Some("ab".repeat(32).as_str()));

        // plain git blob files have a size but no content hash
        let config = expected_for(&entries, "config.json");
        assert_eq!(config.size, Some(10));
        assert_eq!(config.sha256, None);

        // unknown files and directories degrade to no verification
        assert_eq!(
            expected_for(&entries, "missing.gguf"),
            FileExpectation::default()
        );
        assert_eq!(expected_for(&entries, "subdir"), FileExpectation::default());
    }

    #[test]
    fn parses_realistic_tree_entries() {
        let json = r#"[
            {
                "type": "directory",
                "oid": "85afd906013645ffaf0242939d595de8bf36f69a",
                "size": 0,
                "path": "MTP"
            },
            {
                "type": "file",
                "oid": "809be4237a2026c3bb9e829ae862fbe9e66cd63",
                "size": 2620370976,
                "lfs": {
                    "oid": "e531007218dfab990486a5de7676a6932d6ea8dea233d1f698d7c21cf8a16889",
                    "size": 2620370976,
                    "pointerSize": 135
                },
                "path": "gemma-4-E2B-it-qat-UD-Q4_K_XL.gguf"
            },
            {
                "type": "file",
                "oid": "70fa689f511b45526a5e954b79bc98453cbdf181",
                "size": 985654080,
                "lfs": {
                    "oid": "13c8966d1635d02e",
                    "size": 985654080,
                    "pointerSize": 134
                },
                "path": "mmproj-F16.gguf"
            }
        ]"#;
        let entries: Vec<TreeEntry> = serde_json::from_str(json).unwrap();
        assert_eq!(entries.len(), 3);

        let expectation = expected_for(&entries, "gemma-4-E2B-it-qat-UD-Q4_K_XL.gguf");
        assert_eq!(expectation.size, Some(2_620_370_976));
        assert_eq!(
            expectation.sha256.as_deref(),
            Some("e531007218dfab990486a5de7676a6932d6ea8dea233d1f698d7c21cf8a16889")
        );
        assert_eq!(expected_for(&entries, "MTP"), FileExpectation::default());
    }

    #[test]
    fn validates_full_commit_hashes() {
        assert!(is_commit("0123456789abcdef0123456789abcdef01234567"));
        assert!(is_commit("0123456789ABCDEF0123456789ABCDEF01234567"));
        assert!(!is_commit("01234567"));
        assert!(!is_commit("z123456789abcdef0123456789abcdef01234567"));
    }

    #[test]
    fn stores_files_by_immutable_snapshot() {
        let path = snapshot_path(
            RepositoryKind::Model,
            "owner/model",
            "0123456789abcdef0123456789abcdef01234567",
            "subdir/model.safetensors",
        );
        assert!(path.ends_with(
            "owner--model/snapshots/0123456789abcdef0123456789abcdef01234567/subdir/model.safetensors"
        ));
    }

    #[test]
    fn stores_dataset_files_in_the_dataset_namespace() {
        let path = snapshot_path(
            RepositoryKind::Dataset,
            "owner/dataset",
            "0123456789abcdef0123456789abcdef01234567",
            "previews/example.webp",
        );
        assert!(path.ends_with(
            "datasets/owner--dataset/snapshots/0123456789abcdef0123456789abcdef01234567/previews/example.webp"
        ));
    }

    #[tokio::test]
    async fn rejects_paths_that_escape_the_snapshot() {
        let revision = "0123456789abcdef0123456789abcdef01234567";
        assert!(
            HuggingFaceFile::pinned("owner/model", revision, "../model.bin")
                .resolve()
                .await
                .is_err()
        );
        assert!(
            HuggingFaceFile::pinned("owner\\model", revision, "model.bin")
                .resolve()
                .await
                .is_err()
        );
        assert!(
            HuggingFaceFile::pinned("owner/model", "main", "model.bin")
                .resolve()
                .await
                .is_err()
        );
    }
}
