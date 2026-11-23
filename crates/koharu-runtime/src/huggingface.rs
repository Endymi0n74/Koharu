use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::{Context, Result};
use hf_hub::{
    HFClient, RepoType, RepoTypeAny,
    progress::{DownloadEvent, ProgressEvent, ProgressHandler},
};
use tokio::sync::OnceCell;

use crate::{
    store::{self, Expectation},
    transfer::{self, Activity, USER_AGENT},
};

/// A Hugging Face model or dataset at a pinned commit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Repository {
    kind: RepoTypeAny,
    id: &'static str,
    commit: &'static str,
}

/// A repository together with the kind that decides its store path.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RepositoryKind {
    Model(&'static str, &'static str),
    Dataset(&'static str, &'static str),
}

impl Repository {
    #[must_use]
    pub const fn model(id: &'static str, commit: &'static str) -> Self {
        Self::pinned(RepoTypeAny::Model, id, commit)
    }

    #[must_use]
    pub const fn dataset(id: &'static str, commit: &'static str) -> Self {
        Self::pinned(RepoTypeAny::Dataset, id, commit)
    }

    /// A repository is pinned only by a full commit hash, which a constant
    /// checks as it builds.
    const fn pinned(kind: RepoTypeAny, id: &'static str, commit: &'static str) -> Self {
        let hash = commit.as_bytes();
        assert!(hash.len() == 40, "a commit hash has 40 digits");
        let mut index = 0;
        while index < hash.len() {
            assert!(
                hash[index].is_ascii_hexdigit(),
                "a commit hash is hexadecimal"
            );
            index += 1;
        }
        Self { kind, id, commit }
    }

    pub const fn kind(&self) -> RepositoryKind {
        match self.kind {
            RepoTypeAny::Model => RepositoryKind::Model(self.id, self.commit),
            RepoTypeAny::Dataset => RepositoryKind::Dataset(self.id, self.commit),
            RepoTypeAny::Space | RepoTypeAny::Kernel => {
                panic!("a curated model never lives on a space or kernel repo")
            }
        }
    }

    /// The paths of the files `names` names, in the same order, each fetched
    /// first if the store doesn't have it; files transfer one after another.
    #[tracing::instrument(skip_all, fields(repository = self.id))]
    pub async fn resolve<const N: usize>(&self, names: [&str; N]) -> Result<[PathBuf; N]> {
        let paths = names.map(|name| self.path(name));
        let mut expectations = None;
        for (name, path) in names.iter().zip(&paths) {
            // A file the store already has isn't transferred, so its listing
            // isn't read either.
            let expectation = if path.is_file() {
                Expectation::default()
            } else {
                if expectations.is_none() {
                    expectations = Some(self.expectations().await?);
                }
                expectations
                    .as_ref()
                    .and_then(|files| files.get(*name))
                    .cloned()
                    .unwrap_or_default()
            };
            store::put(
                path,
                path.clone(),
                |path| path.is_file(),
                |folder| self.fetch(name, folder, expectation),
            )
            .await?;
        }
        Ok(paths)
    }

    /// Whether the store has the file `name`, without going over the network.
    #[must_use]
    pub fn exists(&self, name: &str) -> bool {
        self.path(name).is_file()
    }

    /// Fetches `name` into `folder` through the Hub's client and returns where
    /// it put it, once it matches `expected`. The client's local-folder
    /// download follows the Hub's redirect before it reads the Xet hash on it,
    /// so it never takes Xet's chunks: the file comes as one plain stream.
    async fn fetch(&self, name: &str, folder: PathBuf, expected: Expectation) -> Result<PathBuf> {
        let (owner, repository) = hf_hub::split_id(self.id);
        let activity = Arc::new(Activity::start(name.rsplit('/').next().unwrap_or(name)));
        let fetched = async {
            let path = client()
                .await?
                .repository(self.kind, owner, repository)
                .download_file()
                .filename(name)
                .revision(self.commit)
                .local_dir(folder)
                .progress(Arc::clone(&activity))
                .send()
                .await?;
            expected.verify(&path).await?;
            Ok(path)
        };
        activity.finish(fetched.await)
    }

    /// The size and SHA-256 the Hub lists for each file of this repository at
    /// its pinned commit, from its tree listing. A transfer that doesn't match
    /// them never reaches the store.
    async fn expectations(&self) -> Result<HashMap<String, Expectation>> {
        /// One file's entry in the Hub's tree listing.
        #[derive(serde::Deserialize)]
        struct Entry {
            #[serde(rename = "type")]
            kind: String,
            path: String,
            size: Option<u64>,
            lfs: Option<Lfs>,
        }

        /// The digest the Hub keeps for a file it stores in LFS, which is the
        /// file's SHA-256.
        #[derive(serde::Deserialize)]
        struct Lfs {
            oid: String,
            size: Option<u64>,
        }

        let url = format!(
            "https://huggingface.co/api/{}/{}/tree/{}?recursive=1",
            self.kind.plural(),
            self.id,
            self.commit
        );
        let entries: Vec<Entry> = transfer::client()
            .await?
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("the Hub's tree listing is malformed")?;
        Ok(entries
            .into_iter()
            .filter(|entry| entry.kind == "file")
            .map(|entry| {
                let size = entry
                    .size
                    .or_else(|| entry.lfs.as_ref().and_then(|lfs| lfs.size));
                let sha256 = entry.lfs.map(|lfs| lfs.oid);
                (entry.path, Expectation { size, sha256 })
            })
            .collect())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.folder().join(name)
    }

    /// The store folder this repository's pinned commit's files sit in, whether
    /// or not any of them was ever downloaded. A prune keeps it and drops the
    /// commits beside it that no catalog pins any more.
    #[must_use]
    pub fn folder(&self) -> PathBuf {
        store::root()
            .join("huggingface")
            .join(self.kind.plural())
            .join(self.id.replace(['/', '\\'], "--"))
            .join("snapshots")
            .join(self.commit)
    }
}

/// The Hub's client, over the transfers' own. Files go to the store, not the
/// Hub's own cache.
async fn client() -> Result<&'static HFClient> {
    static CLIENT: OnceCell<HFClient> = OnceCell::const_new();
    CLIENT
        .get_or_try_init(|| async {
            HFClient::builder()
                .client(transfer::client().await?.clone())
                // The client sends it already; the Hub's own client for
                // redirects takes it from here.
                .user_agent(USER_AGENT)
                .cache_enabled(false)
                .build()
                .context("couldn't build the Hugging Face client")
        })
        .await
}

impl ProgressHandler for Activity {
    fn on_progress(&self, event: &ProgressEvent) {
        let ProgressEvent::Download(event) = event else {
            return;
        };
        match event {
            DownloadEvent::Start { total_bytes, .. } => self.progress(0, *total_bytes),
            DownloadEvent::Progress { files } => {
                for file in files {
                    self.progress(file.bytes_completed, file.total_bytes);
                }
            }
            DownloadEvent::AggregateProgress {
                bytes_completed,
                total_bytes,
                ..
            } => self.progress(*bytes_completed, *total_bytes),
            DownloadEvent::Complete => {}
        }
    }
}
