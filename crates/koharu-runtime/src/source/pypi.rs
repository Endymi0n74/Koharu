use std::collections::HashMap;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::{network, store::FileExpectation};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Platform {
    WindowsX64,
    LinuxX64,
}

impl Platform {
    pub(crate) fn host() -> Result<Self> {
        if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
            Ok(Self::WindowsX64)
        } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            Ok(Self::LinuxX64)
        } else {
            anyhow::bail!("PyPI runtime packages do not support this target")
        }
    }

    fn accepts(self, filename: &str) -> bool {
        match self {
            Self::WindowsX64 => filename.contains("win_amd64"),
            Self::LinuxX64 => filename.contains("manylinux") && filename.contains("x86_64"),
        }
    }
}

#[derive(Deserialize)]
struct Metadata {
    urls: Vec<Distribution>,
}

#[derive(Deserialize)]
struct Distribution {
    filename: String,
    url: String,
    size: Option<u64>,
    #[serde(default)]
    digests: HashMap<String, String>,
}

/// The resolved wheel URL and the expected identity of its contents, used to
/// verify the download before it is extracted into the runtime store.
pub(crate) struct Wheel {
    pub url: String,
    pub expected: FileExpectation,
}

pub(crate) async fn wheel(project: &str, platform: Platform) -> Result<Wheel> {
    let url = format!("https://pypi.org/pypi/{project}/json");
    let metadata: Metadata = network::http()?
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to query {project}"))?
        .error_for_status()?
        .json()
        .await
        .with_context(|| format!("invalid PyPI metadata for {project}"))?;
    let file = metadata
        .urls
        .into_iter()
        .find(|file| file.filename.ends_with(".whl") && platform.accepts(&file.filename))
        .with_context(|| format!("{project} has no compatible wheel"))?;
    Ok(Wheel {
        url: file.url,
        expected: FileExpectation {
            size: file.size,
            sha256: file.digests.get("sha256").cloned(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_accepts_wheels_for_the_host() {
        assert!(Platform::WindowsX64.accepts("nvidia_cublas-13.0.0.19-py3-none-win_amd64.whl"));
        assert!(
            !Platform::WindowsX64
                .accepts("nvidia_cublas-13.0.0.19-py3-none-manylinux_2_27_x86_64.whl")
        );
        assert!(
            Platform::LinuxX64
                .accepts("nvidia_cublas-13.0.0.19-py3-none-manylinux_2_27_x86_64.whl")
        );
        assert!(!Platform::LinuxX64.accepts("nvidia_cublas-13.0.0.19-py3-none-win_amd64.whl"));
    }

    #[test]
    fn parses_pypi_digests_and_sizes() {
        let metadata: Metadata = serde_json::from_str(
            r#"{"urls": [{
                "filename": "nvidia_cublas-13.0.0.19-py3-none-win_amd64.whl",
                "url": "https://files.pythonhosted.org/packages/.../nvidia_cublas-13.0.0.19-py3-none-win_amd64.whl",
                "size": 400515981,
                "digests": {"sha256": "e6ecde441aaf0bb74ed538cfb3b18aa374f452aebf0162088bcb10942f7bbc33"}
            }]}"#,
        )
        .unwrap();
        let wheel = Wheel {
            url: metadata.urls[0].url.clone(),
            expected: FileExpectation {
                size: metadata.urls[0].size,
                sha256: metadata.urls[0].digests.get("sha256").cloned(),
            },
        };
        assert_eq!(wheel.expected.size, Some(400_515_981));
        assert_eq!(
            wheel.expected.sha256.as_deref(),
            Some("e6ecde441aaf0bb74ed538cfb3b18aa374f452aebf0162088bcb10942f7bbc33")
        );
    }
}
