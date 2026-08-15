use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{downloads::Transfer, store::FileExpectation};

#[derive(Deserialize)]
struct Release {
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    size: u64,
    #[serde(default)]
    digest: Option<String>,
}

static CACHE: OnceLock<Mutex<HashMap<String, FileExpectation>>> = OnceLock::new();

/// Resolves the expected identity (size + SHA-256) of a GitHub release asset.
///
/// GitHub publishes a SHA-256 `digest` for every release asset, so both are
/// known up front and the downloaded archive can be verified before use. The
/// release metadata is fetched once per release and cached for the process.
/// When the API is unreachable the expectation degrades to unverified, so a
/// metadata hiccup never blocks installing a package.
pub(crate) async fn release_asset(
    owner: &str,
    repo: &str,
    tag: &str,
    asset: &str,
) -> FileExpectation {
    let key = format!("{owner}/{repo}/{tag}");
    if let Some(found) = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .ok()
        .and_then(|cache| cache.get(&key).cloned())
    {
        return found;
    }

    let expected = match fetch(owner, repo, tag, asset).await {
        Ok(expected) => expected,
        Err(error) => {
            tracing::warn!(%error, "failed to resolve release metadata for {tag}; skipping verification");
            FileExpectation::default()
        }
    };
    if let Ok(mut cache) = CACHE.get_or_init(|| Mutex::new(HashMap::new())).lock() {
        cache.insert(key, expected.clone());
    }
    expected
}

async fn fetch(owner: &str, repo: &str, tag: &str, asset: &str) -> Result<FileExpectation> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/tags/{tag}");
    let release: Release = Transfer::new()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to query GitHub release {tag}"))?
        .error_for_status()?
        .json()
        .await
        .with_context(|| format!("invalid GitHub release metadata for {tag}"))?;
    let asset = release
        .assets
        .into_iter()
        .find(|candidate| candidate.name == asset)
        .with_context(|| format!("release {tag} has no asset {asset}"))?;
    Ok(FileExpectation {
        size: Some(asset.size),
        sha256: asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .map(str::to_owned),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_asset_digests() {
        let release: Release = serde_json::from_str(
            r#"{"assets": [
                {"name": "llama-cuda-windows-2022.tar.gz", "size": 161526328,
                 "digest": "sha256:ff52f0a11f9b05729beb09f22f22391392aab638dca675b5fd3bc22aa13fe88f"},
                {"name": "llama-hip-windows-2022.tar.gz", "size": 131826309, "digest": null}
            ]}"#,
        )
        .unwrap();
        let cuda = release
            .assets
            .iter()
            .find(|asset| asset.name == "llama-cuda-windows-2022.tar.gz")
            .unwrap();
        assert_eq!(cuda.size, 161_526_328);
        assert_eq!(
            cuda.digest.as_deref(),
            Some("sha256:ff52f0a11f9b05729beb09f22f22391392aab638dca675b5fd3bc22aa13fe88f")
        );
        let hip = release
            .assets
            .iter()
            .find(|asset| asset.name == "llama-hip-windows-2022.tar.gz")
            .unwrap();
        assert!(hip.digest.is_none());
    }
}
