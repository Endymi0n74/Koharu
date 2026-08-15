use std::{
    fs::{File, OpenOptions},
    future::Future,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use anyhow::{Context, Result, ensure};
use fs4::FileExt;
use tokio::io::AsyncReadExt;

static ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Expected identity of a downloaded artifact, used to reject files that are
/// truncated or corrupt. An empty expectation (the default) disables
/// verification.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct FileExpectation {
    /// Exact size in bytes, when known.
    pub(crate) size: Option<u64>,
    /// Hex SHA-256 of the artifact contents, when known.
    pub(crate) sha256: Option<String>,
}

impl FileExpectation {
    /// Returns whether an existing file at `path` satisfies the expectation
    /// without reading its contents. Size is compared when known; the SHA-256
    /// is only verified on freshly downloaded artifacts.
    fn satisfied_by_size(&self, path: &Path) -> bool {
        if !path.is_file() {
            return false;
        }
        match self.size {
            Some(size) => std::fs::metadata(path)
                .map(|metadata| metadata.len() == size)
                .unwrap_or(false),
            None => true,
        }
    }
}

/// Verifies a downloaded artifact against the expectation before it is
/// published to the store.
pub(crate) async fn verify_artifact(path: &Path, expected: &FileExpectation) -> Result<()> {
    if let Some(expected_size) = expected.size {
        let actual = tokio::fs::metadata(path).await?.len();
        ensure!(
            actual == expected_size,
            "downloaded artifact is {actual} bytes, expected {expected_size}"
        );
    }
    if let Some(expected_hash) = expected.sha256.as_deref() {
        let actual = sha256_hex(path).await?;
        ensure!(
            actual == expected_hash,
            "downloaded artifact hash {actual} does not match expected {expected_hash}"
        );
    }
    Ok(())
}

async fn sha256_hex(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};

    let mut file = tokio::fs::File::open(path).await?;
    let mut digest = Sha256::new();
    let mut buffer = vec![0u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(hex::encode(digest.finalize()))
}

/// Koharu's process-wide immutable package store.
pub struct Store;

impl Store {
    /// Selects the store before the first artifact or runtime is resolved.
    pub fn configure(root: impl Into<PathBuf>) -> Result<()> {
        let requested = root.into();
        ensure!(
            requested.is_absolute(),
            "runtime store must be absolute: {}",
            requested.display()
        );

        if let Some(active) = ROOT.get() {
            ensure!(
                active == &requested,
                "runtime store is already {}",
                active.display()
            );
            return Ok(());
        }

        if let Err(requested) = ROOT.set(requested) {
            let active = ROOT
                .get()
                .context("runtime store was configured concurrently without a value")?;
            ensure!(
                active == &requested,
                "runtime store is already {}",
                active.display()
            );
        }
        Ok(())
    }

    /// Returns the configured root, or the operating system cache by default.
    #[must_use]
    pub fn root() -> &'static Path {
        ROOT.get_or_init(|| {
            dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("koharu")
                .join("packages")
        })
    }

    /// Installs a directory of immutable artifacts at `target`, re-installing it
    /// whenever `valid` reports the existing copy is incomplete. The expected
    /// identity of the downloaded artifact is handed to the installer so it can
    /// verify the archive (size and SHA-256) before extraction, mirroring the
    /// verification `Store::file` applies to single-file artifacts.
    pub(crate) async fn directory<Valid, Install, Pending>(
        target: PathBuf,
        expected: FileExpectation,
        valid: Valid,
        install: Install,
    ) -> Result<PathBuf>
    where
        Valid: Fn(&Path) -> bool + Send + Sync,
        Install: FnOnce(PathBuf, FileExpectation) -> Pending + Send,
        Pending: Future<Output = Result<()>> + Send,
    {
        if valid(&target) {
            return Ok(target);
        }

        let _guard = Lock::acquire(&target).await?;
        if valid(&target) {
            return Ok(target);
        }

        let parent = target
            .parent()
            .with_context(|| format!("{} has no parent directory", target.display()))?;
        if target
            .try_exists()
            .with_context(|| format!("failed to inspect {}", target.display()))?
        {
            std::fs::remove_dir_all(&target)
                .with_context(|| format!("failed to replace incomplete {}", target.display()))?;
        }
        let stage = tempfile::Builder::new()
            .prefix(".install-")
            .tempdir_in(parent)
            .with_context(|| format!("failed to stage {}", target.display()))?;
        install(stage.path().to_owned(), expected).await?;
        ensure!(
            valid(stage.path()),
            "installer produced an incomplete package"
        );

        match std::fs::rename(stage.path(), &target) {
            Ok(()) => Ok(target),
            Err(_) if valid(&target) => Ok(target),
            Err(error) => {
                Err(error).with_context(|| format!("failed to publish {}", target.display()))
            }
        }
    }

    /// Publishes a single immutable artifact at `target`, re-downloading it
    /// whenever the existing file does not satisfy `expected`. Fresh downloads
    /// are verified against the expectation before they replace any cached copy.
    pub(crate) async fn file<Install, Pending>(
        target: PathBuf,
        expected: FileExpectation,
        install: Install,
    ) -> Result<PathBuf>
    where
        Install: FnOnce(PathBuf) -> Pending + Send,
        Pending: Future<Output = Result<()>> + Send,
    {
        if expected.satisfied_by_size(&target) {
            return Ok(target);
        }

        let _guard = Lock::acquire(&target).await?;
        if expected.satisfied_by_size(&target) {
            return Ok(target);
        }

        let parent = target
            .parent()
            .with_context(|| format!("{} has no parent directory", target.display()))?;
        let stage = tempfile::NamedTempFile::new_in(parent)
            .with_context(|| format!("failed to stage {}", target.display()))?
            .into_temp_path();
        install(stage.to_path_buf()).await?;
        ensure!(stage.is_file(), "installer did not create an artifact");
        verify_artifact(&stage, &expected).await?;

        if target.exists() {
            std::fs::remove_file(&target)
                .with_context(|| format!("failed to replace incomplete {}", target.display()))?;
        }
        stage
            .persist(&target)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to publish {}", target.display()))?;
        Ok(target)
    }
}

struct Lock(File);

impl Lock {
    async fn acquire(target: &Path) -> Result<Self> {
        let parent = target
            .parent()
            .with_context(|| format!("{} has no parent directory", target.display()))?;
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create {}", parent.display()))?;

        let name = target
            .file_name()
            .with_context(|| format!("{} has no file name", target.display()))?
            .to_string_lossy();
        let path = target.with_file_name(format!(".{name}.lock"));
        tokio::task::spawn_blocking(move || {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .with_context(|| format!("failed to open {}", path.display()))?;
            FileExt::lock(&file).with_context(|| format!("failed to lock {}", path.display()))?;
            Ok(Self(file))
        })
        .await
        .context("runtime store lock task failed")?
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_installation_wins_for_a_shared_target() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("runtime");
        let attempts = Arc::new(AtomicUsize::new(0));
        let run = || {
            let target = target.clone();
            let attempts = attempts.clone();
            async move {
                Store::directory(
                    target,
                    FileExpectation::default(),
                    |path| path.join("ready").is_file(),
                    move |stage, _| async move {
                        attempts.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        tokio::fs::write(stage.join("ready"), b"ok").await?;
                        Ok(())
                    },
                )
                .await
            }
        };

        let (left, right) = tokio::join!(run(), run());
        assert_eq!(left.unwrap(), target);
        assert_eq!(right.unwrap(), target);
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn a_valid_target_that_appears_during_installation_wins() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("runtime");
        let winner = target.clone();

        let installed = Store::directory(
            target.clone(),
            FileExpectation::default(),
            |path| path.join("ready").is_file(),
            move |stage, _| async move {
                tokio::fs::write(stage.join("ready"), b"staged").await?;
                tokio::fs::create_dir_all(&winner).await?;
                tokio::fs::write(winner.join("ready"), b"winner").await?;
                Ok(())
            },
        )
        .await
        .unwrap();

        assert_eq!(installed, target);
        assert_eq!(std::fs::read(target.join("ready")).unwrap(), b"winner");
        assert!(std::fs::read_dir(root.path()).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".install-")
        }));
    }

    #[test]
    fn expectations_require_an_existing_file_of_the_right_size() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("artifact");
        std::fs::write(&path, b"hello world").unwrap();

        let exact = FileExpectation {
            size: Some(11),
            sha256: None,
        };
        assert!(exact.satisfied_by_size(&path));
        assert!(!exact.satisfied_by_size(&root.path().join("missing")));

        let truncated = FileExpectation {
            size: Some(10),
            sha256: None,
        };
        assert!(!truncated.satisfied_by_size(&path));

        let unknown = FileExpectation::default();
        assert!(unknown.satisfied_by_size(&path));
        assert!(!unknown.satisfied_by_size(&root.path().join("missing")));
    }

    #[tokio::test]
    async fn re_downloads_a_cached_file_with_the_wrong_size() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("model.gguf");
        std::fs::write(&target, b"stale truncated model").unwrap();

        let installed = Store::file(
            target.clone(),
            FileExpectation {
                size: Some(3),
                sha256: None,
            },
            |stage| async move {
                tokio::fs::write(stage, b"new").await?;
                Ok(())
            },
        )
        .await
        .unwrap();
        assert_eq!(installed, target);
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
    }

    #[tokio::test]
    async fn rejects_a_download_that_fails_hash_verification() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("model.gguf");
        std::fs::write(&target, b"previous copy").unwrap();

        let error = Store::file(
            target.clone(),
            FileExpectation {
                size: Some(3),
                sha256: Some("ab".repeat(32)),
            },
            |stage| async move {
                tokio::fs::write(stage, b"new").await?;
                Ok(())
            },
        )
        .await
        .unwrap_err();

        assert!(error.to_string().contains("hash"));
        // the previous copy is preserved when the replacement is invalid
        assert_eq!(std::fs::read(&target).unwrap(), b"previous copy");
    }

    #[tokio::test]
    async fn verifies_sha256_of_a_fresh_download() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("model.gguf");
        // sha256("hello world")
        let expectation = FileExpectation {
            size: Some(11),
            sha256: Some(
                "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_owned(),
            ),
        };
        Store::file(target.clone(), expectation, |stage| async move {
            tokio::fs::write(stage, b"hello world").await?;
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello world");
    }

    #[tokio::test]
    async fn directory_hands_the_expectation_to_the_installer() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("runtime");
        let expected = FileExpectation {
            size: Some(4_096),
            sha256: Some("ab".repeat(32)),
        };

        Store::directory(
            target.clone(),
            expected.clone(),
            |path| path.join("ready").is_file(),
            move |stage, received| async move {
                assert_eq!(received, expected);
                tokio::fs::write(stage.join("ready"), b"ok").await?;
                Ok(())
            },
        )
        .await
        .unwrap();
        assert!(target.join("ready").is_file());
    }

    #[test]
    fn one_installation_wins_across_processes() {
        const TARGET: &str = "KOHARU_RUNTIME_STORE_TEST_TARGET";

        if let Some(target) = std::env::var_os(TARGET) {
            let target = PathBuf::from(target);
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(Store::directory(
                    target.clone(),
                    FileExpectation::default(),
                    |path| path.join("ready").is_file(),
                    move |stage, _| async move {
                        OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(target.with_file_name("install-attempt"))?;
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        tokio::fs::write(stage.join("ready"), b"ok").await?;
                        Ok(())
                    },
                ))
                .unwrap();
            return;
        }

        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("runtime");
        let executable = std::env::current_exe().unwrap();
        let spawn = || {
            std::process::Command::new(&executable)
                .args([
                    "--exact",
                    "store::tests::one_installation_wins_across_processes",
                ])
                .env(TARGET, &target)
                .spawn()
                .unwrap()
        };
        let mut first = spawn();
        let mut second = spawn();

        assert!(first.wait().unwrap().success());
        assert!(second.wait().unwrap().success());
        assert!(target.join("ready").is_file());
        assert!(root.path().join("install-attempt").is_file());
    }
}
