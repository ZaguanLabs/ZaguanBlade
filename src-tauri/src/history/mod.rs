use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub file_path: PathBuf,
    pub timestamp: u64,
    pub snapshot_path: PathBuf,
}

pub struct HistoryService {
    history_root: PathBuf,
}

impl HistoryService {
    pub fn new(app_data_dir: &Path) -> Self {
        let history_root = app_data_dir.join("history");
        if let Err(e) = fs::create_dir_all(&history_root) {
            eprintln!("Failed to create history directory: {}", e);
        }
        Self { history_root }
    }

    pub fn create_snapshot(&self, file_path: &Path) -> Result<HistoryEntry, String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let id = uuid::Uuid::new_v4().to_string();
        let snapshot_filename = format!("{}_{}", timestamp, id);
        let snapshot_path = self.history_root.join(&snapshot_filename);

        // Copy the current file content to the snapshot path
        fs::copy(file_path, &snapshot_path).map_err(|e| e.to_string())?;

        Ok(HistoryEntry {
            id,
            file_path: file_path.to_path_buf(),
            timestamp,
            snapshot_path,
        })
    }

    pub fn revert_to(&self, entry: &HistoryEntry) -> Result<(), String> {
        fs::copy(&entry.snapshot_path, &entry.file_path).map_err(|e| e.to_string())?;
        Ok(())
    }
}
