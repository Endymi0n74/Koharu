//! The store packages and models download into, one version folder each.

mod lock;
mod remove;

use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use anyhow::{Context, Result, ensure};

use lock::lock;
pub(crate) use remove::{remove, remove_others};

/// The one store everything downloads into: `KOHARU_STORE` when it names a
/// folder, KoharuBatch's root's `store` when the executable's folder is writable,
/// and the OS cache folder's `koharu/store` otherwise, which is a per-user
/// install's store too.
pub fn root() -> &'static Path {
    static ROOT: LazyLock<PathBuf> = LazyLock::new(|| {
        if let Some(folder) = std::env::var_os("KOHARU_STORE").filter(|folder| !folder.is_empty()) {
            return PathBuf::from(folder);
        }
        match crate::root() {
            Some(folder) => folder.join("store"),
            None => dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("koharu")
                .join("store"),
        }
    });
    &ROOT
}

/// What a downloaded file must be: the size and SHA-256 its publisher lists,
/// either of which may be unknown. A file that doesn't match never reaches the
/// store.
#[derive(Clone, Debug, Default)]
pub(crate) struct Expectation {
    pub(crate) size: Option<u64>,
    pub(crate) sha256: Option<String>,
}

impl Expectation {
    /// Checks `path` against what's known of it: the size from its metadata,
    /// the digest by reading it once.
    pub(crate) async fn verify(&self, path: &Path) -> Result<()> {
        if let Some(expected) = self.size {
            let size = fs_err::tokio::metadata(path).await?.len();
            ensure!(
                size == expected,
                "{} is {size} bytes, not the {expected} its publisher lists",
                path.display()
            );
        }
        if let Some(expected) = &self.sha256 {
            let digest = sha256(path).await?;
            ensure!(
                digest.eq_ignore_ascii_case(expected),
                "{} has SHA-256 {digest}, not the {expected} its publisher lists",
                path.display()
            );
        }
        Ok(())
    }
}

/// The file's SHA-256, in lowercase hexadecimal.
async fn sha256(path: &Path) -> Result<String> {
    use std::fmt::Write as _;

    use sha2::{Digest as _, Sha256};
    use tokio::io::AsyncReadExt as _;

    let mut file = fs_err::tokio::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let mut digest = String::with_capacity(64);
    for byte in hasher.finalize() {
        write!(digest, "{byte:02x}")?;
    }
    Ok(digest)
}

/// Puts `target`, a file or a folder, in the store once: `write` fills a
/// staging folder on the store's own volume and returns what it wrote there,
/// which moves into place once `valid` says it's complete. A lock on `scope`,
/// which is `target` or the version folder it's in, holds off every other
/// download into it and its removal, in this process or another.
pub(crate) async fn put<Valid, Write, Pending>(
    scope: &Path,
    target: PathBuf,
    valid: Valid,
    write: Write,
) -> Result<PathBuf>
where
    Valid: Fn(&Path) -> bool + Send + Sync,
    Write: FnOnce(PathBuf) -> Pending + Send,
    Pending: Future<Output = Result<PathBuf>> + Send,
{
    if valid(&target) {
        return Ok(target);
    }

    let _lock = lock(scope).await?;
    if valid(&target) {
        return Ok(target);
    }

    // Not valid, so it's what's left of an incomplete one.
    if fs_err::exists(&target)? {
        fs_err::remove_dir_all(&target)?;
    }
    let parent = target
        .parent()
        .with_context(|| format!("{} has no parent folder", target.display()))?;
    fs_err::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".stage-")
        .tempdir_in(parent)?;
    let staged = write(stage.path().to_owned()).await?;
    ensure!(
        valid(&staged),
        "the download produced an incomplete package"
    );
    fs_err::rename(&staged, &target)?;
    Ok(target)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use fs_err::OpenOptions;

    use super::*;

    #[tokio::test]
    async fn a_file_must_match_its_published_size_and_digest() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("weights.gguf");
        fs_err::tokio::write(&file, b"abc").await.unwrap();

        // "abc"'s SHA-256, as every publisher lists it.
        let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        let exact = Expectation {
            size: Some(3),
            sha256: Some(digest.to_owned()),
        };
        exact.verify(&file).await.unwrap();

        let short = Expectation {
            size: Some(4),
            sha256: None,
        };
        assert!(short.verify(&file).await.is_err());

        let wrong = Expectation {
            size: None,
            sha256: Some("00".repeat(32)),
        };
        assert!(wrong.verify(&file).await.is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn one_download_wins_for_a_shared_target() {
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("runtime");
        let attempts = Arc::new(AtomicUsize::new(0));
        let run = || {
            let target = target.clone();
            let attempts = attempts.clone();
            async move {
                put(
                    &target,
                    target.clone(),
                    |path| path.join("ready").is_file(),
                    move |stage| async move {
                        attempts.fetch_add(1, Ordering::Relaxed);
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                        fs_err::tokio::write(stage.join("ready"), b"ok").await?;
                        Ok(stage)
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

    #[test]
    fn one_download_wins_across_processes() {
        const TARGET: &str = "KOHARU_RUNTIME_STORE_TEST_TARGET";

        if let Some(target) = std::env::var_os(TARGET) {
            let target = PathBuf::from(target);
            let attempt = target.with_file_name("attempt");
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(put(
                    &target,
                    target.clone(),
                    |path| path.join("ready").is_file(),
                    move |stage| async move {
                        OpenOptions::new()
                            .write(true)
                            .create_new(true)
                            .open(attempt)?;
                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        fs_err::tokio::write(stage.join("ready"), b"ok").await?;
                        Ok(stage)
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
                    "store::tests::one_download_wins_across_processes",
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
        assert!(root.path().join("attempt").is_file());
    }
}
