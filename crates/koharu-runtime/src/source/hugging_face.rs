use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, LazyLock, Mutex, OnceLock},
};

use anyhow::Context;
use hf_hub::{HFClient, HFRepository, RepoType, repository::download::HFByteStream, split_id};
use serde::Deserialize;
use tokio::sync::OnceCell;

use crate::{Store, download, network, store::FileExpectation};

static CLIENT: OnceLock<HFClient> = OnceLock::new();

fn client() -> anyhow::Result<HFClient> {
    if let Some(client) = CLIENT.get() {
        return Ok(client.clone());
    }
    let max_retries = network::config()?.max_retries as usize;
    let http = network::http()?;
    let client = HFClient::builder()
        .client(http)
        .cache_enabled(false)
        .retry_max_attempts(max_retries)
        .build()?;
    Ok(CLIENT.get_or_init(|| client).clone())
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
enum RepositoryKind {
    Model,
    Dataset,
}

impl RepositoryKind {
    /// Route segment of the Hugging Face REST API for this kind of repository.
    const fn api_route(self) -> &'static str {
        match self {
            Self::Model => "models",
            Self::Dataset => "datasets",
        }
    }
}

/// One `/tree/{revision}` listing per repository revision, resolved at most
/// once per process so that verifying several files of one model does not
/// repeat the metadata request.
type TreeCache = LazyLock<Mutex<HashMap<String, Arc<OnceCell<Arc<Vec<TreeEntry>>>>>>>;
static TREES: TreeCache = LazyLock::new(|| Mutex::new(HashMap::new()));

/// One entry of a repository's `/tree/{revision}` listing.
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

/// An immutable file snapshot hosted by Hugging Face.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct HuggingFaceFile<'a> {
    kind: RepositoryKind,
    repository: &'a str,
    revision: &'a str,
    filename: &'a str,
}

impl<'a> HuggingFaceFile<'a> {
    #[must_use]
    pub const fn pinned(repository: &'a str, revision: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Model,
            repository,
            revision,
            filename,
        }
    }

    #[must_use]
    pub const fn pinned_dataset(repository: &'a str, revision: &'a str, filename: &'a str) -> Self {
        Self {
            kind: RepositoryKind::Dataset,
            ..Self::pinned(repository, revision, filename)
        }
    }

    #[must_use]
    pub fn exists(self) -> bool {
        self.path().is_file()
    }

    #[tracing::instrument(skip_all)]
    pub async fn resolve(self) -> anyhow::Result<PathBuf> {
        let expected = self.expectation().await;
        Store::file(self.path(), expected, move |stage| async move {
            let client = client()?;
            let (owner, name) = split_id(self.repository);
            download::receive(self.filename, &stage, async {
                match self.kind {
                    RepositoryKind::Model => self.download(client.model(owner, name)).await,
                    RepositoryKind::Dataset => self.download(client.dataset(owner, name)).await,
                }
            })
            .await
        })
        .await
    }

    /// Resolves the expected size and SHA-256 of this file from the repository
    /// listing, so a download can be rejected before it replaces a cached
    /// snapshot. Metadata that cannot be resolved degrades to no verification
    /// rather than failing the download.
    async fn expectation(self) -> FileExpectation {
        match tree_entries(self.kind, self.repository, self.revision).await {
            Ok(entries) => expected_for(&entries, self.filename),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    "failed to resolve file metadata for {}/{}; continuing without integrity verification",
                    self.repository,
                    self.filename
                );
                FileExpectation::default()
            }
        }
    }

    async fn download(
        self,
        repository: HFRepository<impl RepoType>,
    ) -> anyhow::Result<(Option<u64>, HFByteStream)> {
        repository
            .download_file_stream()
            .filename(self.filename)
            .revision(self.revision)
            .send()
            .await
            .map_err(Into::into)
    }

    fn path(self) -> PathBuf {
        Store::root()
            .join("hugging-face")
            .join(self.kind.api_route())
            .join(self.repository.replace(['/', '\\'], "--"))
            .join("snapshots")
            .join(self.revision)
            .join(self.filename)
    }
}

/// Lists the files of `repository` at `revision`, cached once per process.
async fn tree_entries(
    kind: RepositoryKind,
    repository: &str,
    revision: &str,
) -> anyhow::Result<Arc<Vec<TreeEntry>>> {
    let key = format!("{}/{repository}/{revision}", kind.api_route());
    let cell = {
        let mut trees = TREES.lock().expect("tree cache is not poisoned");
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
        let entries = network::http()?
            .get(&url)
            .send()
            .await
            .with_context(|| format!("failed to resolve {repository} file metadata"))?
            .error_for_status()
            .with_context(|| format!("failed to resolve {repository} file metadata"))?
            .json::<Vec<TreeEntry>>()
            .await
            .with_context(|| format!("invalid file metadata for {repository}"))?;
        Ok::<_, anyhow::Error>(Arc::new(entries))
    })
    .await
    .cloned()
}

/// Derives the expected size and SHA-256 of `filename` from the repository
/// listing. Plain git blobs expose a size but no content hash, and files that
/// are absent from the listing degrade to no verification.
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
}
