use super::config::{ConfigError, IntegrationConfig, MAX_CONFIG_BYTES};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Serialize)]
pub struct ConfigSnapshot {
    pub revision: String,
    pub config: IntegrationConfig,
}

pub struct IntegrationStore {
    directory: PathBuf,
}

impl IntegrationStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn load(&self) -> Result<ConfigSnapshot, ConfigError> {
        let file = match File::open(self.directory.join("integrations.json")) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConfigSnapshot {
                    revision: "missing".into(),
                    config: IntegrationConfig::default(),
                })
            }
            Err(_) => return Err(ConfigError::ReadFailed),
        };
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| ConfigError::ReadFailed)?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::TooLarge);
        }
        let config: IntegrationConfig =
            serde_json::from_slice(&bytes).map_err(|_| ConfigError::InvalidConfig)?;
        config.validate()?;
        Ok(ConfigSnapshot {
            revision: fingerprint(&bytes),
            config,
        })
    }

    /// Serialize writers across windows and processes, then compare the exact
    /// loaded revision. Invalid, stale or non-durable data never replaces good data.
    pub fn save(
        &self,
        expected_revision: &str,
        config: IntegrationConfig,
    ) -> Result<ConfigSnapshot, ConfigError> {
        config.validate()?;
        let bytes = serde_json::to_vec_pretty(&config).map_err(|_| ConfigError::InvalidConfig)?;
        if bytes.len() > MAX_CONFIG_BYTES {
            return Err(ConfigError::TooLarge);
        }
        fs::create_dir_all(&self.directory).map_err(|_| ConfigError::WriteFailed)?;
        let lock = private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join("integrations.lock"))
            .map_err(|_| ConfigError::WriteFailed)?;
        lock.lock().map_err(|_| ConfigError::WriteFailed)?;
        if self.load()?.revision != expected_revision {
            return Err(ConfigError::Conflict);
        }
        atomic_replace(&self.directory.join("integrations.json"), &bytes)?;
        Ok(ConfigSnapshot {
            revision: fingerprint(&bytes),
            config,
        })
    }
}

pub(super) fn fingerprint(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn private_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}

fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<(), ConfigError> {
    let parent = path.parent().ok_or(ConfigError::WriteFailed)?;
    let temporary = parent.join(format!(".integrations.{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = private_options()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        // Same-directory replacement preserves the last complete configuration.
        fs::rename(&temporary, path)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok::<(), std::io::Error>(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|_| ConfigError::WriteFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_missing_settings_does_not_create_files() {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("config");
        let store = IntegrationStore::new(directory.clone());
        assert_eq!(store.load().unwrap().revision, "missing");
        assert!(!directory.exists());
    }

    #[test]
    fn stale_save_and_invalid_data_preserve_the_last_complete_configuration() {
        let root = tempfile::tempdir().unwrap();
        let store = IntegrationStore::new(root.path().into());
        let first = store.save("missing", IntegrationConfig::default()).unwrap();
        assert!(matches!(
            store.save("missing", IntegrationConfig::default()),
            Err(ConfigError::Conflict)
        ));
        let invalid = IntegrationConfig {
            schema_version: 99,
            ..IntegrationConfig::default()
        };
        assert!(matches!(
            store.save(&first.revision, invalid),
            Err(ConfigError::UnsupportedVersion)
        ));
        assert_eq!(store.load().unwrap().revision, first.revision);
        assert!(store
            .save(&first.revision, IntegrationConfig::default())
            .is_ok());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(root.path().join("integrations.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn corrupt_and_future_files_are_reported_and_never_overwritten_by_defaults() {
        let root = tempfile::tempdir().unwrap();
        let store = IntegrationStore::new(root.path().into());
        for bytes in ["{broken", "{\"schema_version\":99}"] {
            fs::write(root.path().join("integrations.json"), bytes).unwrap();
            assert!(store.load().is_err());
            assert!(store.save("missing", IntegrationConfig::default()).is_err());
            assert_eq!(
                fs::read_to_string(root.path().join("integrations.json")).unwrap(),
                bytes
            );
        }
    }

    #[test]
    fn concurrent_stale_writers_cannot_overwrite_one_another() {
        let root = tempfile::tempdir().unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let workers: Vec<_> = (0..2)
            .map(|_| {
                let directory = root.path().to_path_buf();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    IntegrationStore::new(directory).save("missing", IntegrationConfig::default())
                })
            })
            .collect();
        let results: Vec<_> = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(ConfigError::Conflict)))
                .count(),
            1
        );
    }
}
