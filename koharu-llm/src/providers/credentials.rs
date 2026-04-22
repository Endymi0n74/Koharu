use std::sync::Once;

use keyring::Entry;

const API_KEY_SERVICE: &str = "koharu";

static INIT_CREDENTIAL_STORE: Once = Once::new();

pub fn get_saved_api_key(provider: &str) -> anyhow::Result<Option<String>> {
    let entry = provider_entry(provider)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

pub fn set_saved_api_key(provider: &str, api_key: &str) -> anyhow::Result<()> {
    let entry = provider_entry(provider)?;
    if api_key.trim().is_empty() {
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.into()),
        };
    }

    entry.set_password(api_key)?;
    Ok(())
}

fn provider_entry(provider: &str) -> anyhow::Result<Entry> {
    INIT_CREDENTIAL_STORE.call_once(configure_platform_store);

    let username = format!("llm_provider_api_key_{provider}");
    Ok(Entry::new(API_KEY_SERVICE, &username)?)
}

#[cfg(target_os = "linux")]
fn configure_platform_store() {
    let root = koharu_runtime::default_app_data_root()
        .as_std_path()
        .join("secrets")
        .join("keyring");
    keyring::set_default_credential_builder(Box::new(filesystem::Builder::new(root)));
}

#[cfg(not(target_os = "linux"))]
fn configure_platform_store() {}

#[cfg(any(target_os = "linux", test))]
mod filesystem {
    use std::fmt::Write as _;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use keyring::credential::{Credential, CredentialApi, CredentialBuilderApi};
    use keyring::{Error, Result};

    static TEMP_ID: AtomicU64 = AtomicU64::new(0);

    #[derive(Debug)]
    pub(super) struct Builder {
        root: PathBuf,
    }

    impl Builder {
        pub(super) fn new(root: impl Into<PathBuf>) -> Self {
            Self { root: root.into() }
        }
    }

    impl CredentialBuilderApi for Builder {
        fn build(
            &self,
            target: Option<&str>,
            service: &str,
            user: &str,
        ) -> Result<Box<Credential>> {
            Ok(Box::new(FileCredential {
                root: self.root.clone(),
                name: file_name(target, service, user),
            }))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[derive(Debug)]
    struct FileCredential {
        root: PathBuf,
        name: String,
    }

    impl FileCredential {
        fn path(&self) -> PathBuf {
            self.root.join(&self.name)
        }

        fn temp_path(&self) -> PathBuf {
            self.root.join(format!(
                "{}.tmp-{}-{}",
                self.name,
                std::process::id(),
                TEMP_ID.fetch_add(1, Ordering::Relaxed)
            ))
        }
    }

    impl CredentialApi for FileCredential {
        fn set_secret(&self, secret: &[u8]) -> Result<()> {
            fs::create_dir_all(&self.root).map_err(storage_error)?;
            set_mode(&self.root, 0o700)?;

            let path = self.path();
            let temp_path = self.temp_path();
            fs::write(&temp_path, secret).map_err(storage_error)?;
            set_mode(&temp_path, 0o600)?;

            fs::rename(&temp_path, &path).map_err(|err| {
                let _ = fs::remove_file(&temp_path);
                storage_error(err)
            })?;
            set_mode(&path, 0o600)
        }

        fn get_secret(&self) -> Result<Vec<u8>> {
            fs::read(self.path()).map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => Error::NoEntry,
                _ => storage_error(err),
            })
        }

        fn delete_credential(&self) -> Result<()> {
            fs::remove_file(self.path()).map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => Error::NoEntry,
                _ => storage_error(err),
            })
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[cfg(unix)]
    fn set_mode(path: &Path, mode: u32) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(mode)).map_err(storage_error)
    }

    #[cfg(not(unix))]
    fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
        Ok(())
    }

    fn storage_error(err: std::io::Error) -> Error {
        Error::NoStorageAccess(Box::new(err))
    }

    fn file_name(target: Option<&str>, service: &str, user: &str) -> String {
        let target = target
            .map(|value| format!("some-{}", encode(value)))
            .unwrap_or_else(|| "none".to_string());
        format!(
            "v1-target-{target}--service-{}--user-{}.secret",
            encode(service),
            encode(user)
        )
    }

    fn encode(value: &str) -> String {
        let mut encoded = String::with_capacity(value.len());
        for byte in value.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                    encoded.push(byte as char);
                }
                _ => {
                    let _ = write!(&mut encoded, "%{byte:02X}");
                }
            }
        }
        encoded
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn round_trips_secret_across_credentials() {
            let dir = tempfile::tempdir().unwrap();
            let builder = Builder::new(dir.path());

            let first = builder
                .build(None, "koharu", "llm_provider_api_key_openai")
                .unwrap();
            assert!(matches!(first.get_secret(), Err(Error::NoEntry)));
            first.set_secret(b"sk-test").unwrap();

            let second = builder
                .build(None, "koharu", "llm_provider_api_key_openai")
                .unwrap();
            assert_eq!(second.get_secret().unwrap(), b"sk-test");
            second.delete_credential().unwrap();
            assert!(matches!(second.get_secret(), Err(Error::NoEntry)));
        }

        #[test]
        fn file_names_escape_path_separators() {
            let name = file_name(Some("target/value"), "service\\name", "user name");

            assert!(name.contains("target%2Fvalue"));
            assert!(name.contains("service%5Cname"));
            assert!(name.contains("user%20name"));
            assert!(!name.contains('/'));
            assert!(!name.contains('\\'));
        }

        #[cfg(unix)]
        #[test]
        fn writes_private_permissions() {
            use std::os::unix::fs::PermissionsExt;

            let dir = tempfile::tempdir().unwrap();
            let builder = Builder::new(dir.path());
            let credential = builder
                .build(None, "koharu", "llm_provider_api_key_openai")
                .unwrap();

            credential.set_secret(b"sk-test").unwrap();

            let dir_mode = fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777;
            let file_path =
                dir.path()
                    .join(file_name(None, "koharu", "llm_provider_api_key_openai"));
            let file_mode = fs::metadata(file_path).unwrap().permissions().mode() & 0o777;

            assert_eq!(dir_mode, 0o700);
            assert_eq!(file_mode, 0o600);
        }
    }
}
