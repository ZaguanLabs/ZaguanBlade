//! Launches the user chose to start again automatically. Records live in the
//! machine-local config directory, never in a repository, and bind one
//! integration to one workspace and the trust fingerprint of its resolved
//! launch. Any change to the launch voids the record.
use super::{
    store::{atomic_replace, private_options},
    RuntimeError,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
use uuid::Uuid;

const FILE: &str = "launch_trust.json";
const LOCK: &str = "launch_trust.lock";
const MAX_RECORDS: usize = 256;
const MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustRecord {
    pub integration: Uuid,
    pub workspace_id: String,
    pub fingerprint: String,
}

#[derive(Default, Serialize, Deserialize)]
struct TrustFile {
    schema_version: u32,
    records: Vec<TrustRecord>,
}

pub struct LaunchTrust {
    directory: PathBuf,
}

impl LaunchTrust {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// A missing, oversized or unreadable file grants nothing.
    fn load(path: &Path) -> Vec<TrustRecord> {
        let Ok(file) = fs::File::open(path) else {
            return Vec::new();
        };
        let mut bytes = Vec::new();
        if file
            .take(MAX_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .is_err()
            || bytes.len() > MAX_BYTES
        {
            return Vec::new();
        }
        serde_json::from_slice::<TrustFile>(&bytes)
            .ok()
            .filter(|file| file.schema_version == 1)
            .map(|file| file.records)
            .unwrap_or_default()
    }

    fn update(&self, edit: impl FnOnce(&mut Vec<TrustRecord>)) -> Result<(), RuntimeError> {
        fs::create_dir_all(&self.directory).map_err(|_| RuntimeError::PolicyUnavailable)?;
        let lock = private_options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join(LOCK))
            .map_err(|_| RuntimeError::PolicyUnavailable)?;
        lock.lock().map_err(|_| RuntimeError::PolicyUnavailable)?;
        let path = self.directory.join(FILE);
        let mut records = Self::load(&path);
        edit(&mut records);
        if records.len() > MAX_RECORDS {
            records.drain(..records.len() - MAX_RECORDS);
        }
        let bytes = serde_json::to_vec_pretty(&TrustFile {
            schema_version: 1,
            records,
        })
        .map_err(|_| RuntimeError::PolicyUnavailable)?;
        atomic_replace(&path, &bytes).map_err(|_| RuntimeError::PolicyUnavailable)
    }

    /// Replaces any earlier record for the same integration and workspace.
    pub fn remember(&self, record: TrustRecord) -> Result<(), RuntimeError> {
        self.update(|records| {
            records.retain(|existing| {
                existing.integration != record.integration
                    || existing.workspace_id != record.workspace_id
            });
            records.push(record);
        })
    }

    pub fn forget(&self, integration: Uuid, workspace_id: &str) -> Result<(), RuntimeError> {
        if !self
            .for_workspace(workspace_id)
            .iter()
            .any(|record| record.integration == integration)
        {
            return Ok(());
        }
        self.update(|records| {
            records.retain(|record| {
                record.integration != integration || record.workspace_id != workspace_id
            })
        })
    }

    pub fn for_workspace(&self, workspace_id: &str) -> Vec<TrustRecord> {
        Self::load(&self.directory.join(FILE))
            .into_iter()
            .filter(|record| record.workspace_id == workspace_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(integration: Uuid, workspace: &str, fingerprint: &str) -> TrustRecord {
        TrustRecord {
            integration,
            workspace_id: workspace.into(),
            fingerprint: fingerprint.into(),
        }
    }

    #[test]
    fn records_are_per_workspace_replaced_forgotten_and_capped() {
        let root = tempfile::tempdir().unwrap();
        let trust = LaunchTrust::new(root.path().join("config"));
        assert!(trust.for_workspace("w").is_empty());
        let id = Uuid::new_v4();
        trust.remember(record(id, "w", "a")).unwrap();
        trust.remember(record(id, "other", "a")).unwrap();
        trust.remember(record(id, "w", "b")).unwrap();
        assert_eq!(trust.for_workspace("w"), vec![record(id, "w", "b")]);
        trust.forget(id, "w").unwrap();
        assert!(trust.for_workspace("w").is_empty());
        assert_eq!(trust.for_workspace("other").len(), 1);
        for _ in 0..MAX_RECORDS + 3 {
            trust.remember(record(Uuid::new_v4(), "w", "c")).unwrap();
        }
        assert_eq!(trust.for_workspace("w").len(), MAX_RECORDS);
        assert!(trust.for_workspace("other").is_empty());
    }

    #[test]
    fn an_unreadable_file_grants_nothing() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join(FILE), b"{not json").unwrap();
        let trust = LaunchTrust::new(root.path().into());
        assert!(trust.for_workspace("w").is_empty());
        fs::write(
            root.path().join(FILE),
            br#"{"schema_version":2,"records":[{"integration":"00000000-0000-0000-0000-000000000001","workspace_id":"w","fingerprint":"f"}]}"#,
        )
        .unwrap();
        assert!(trust.for_workspace("w").is_empty());
    }
}
