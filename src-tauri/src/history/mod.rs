use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
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
    index_path: PathBuf,
    index: Mutex<HashMap<PathBuf, Vec<HistoryEntry>>>,
}

impl HistoryService {
    pub fn new(app_data_dir: &Path) -> Self {
        let history_root = app_data_dir.join("history");
        if let Err(e) = fs::create_dir_all(&history_root) {
            eprintln!("Failed to create history directory: {}", e);
        }
        let index_path = history_root.join("index.json");

        // Load index if exists
        let index = if index_path.exists() {
            match fs::read_to_string(&index_path) {
                Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
                Err(e) => {
                    eprintln!("Failed to load history index: {}", e);
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };

        Self {
            history_root,
            index_path,
            index: Mutex::new(index),
        }
    }

    fn save_index(&self) {
        let index = self.index.lock().unwrap();
        if let Ok(content) = serde_json::to_string_pretty(&*index) {
            let _ = fs::write(&self.index_path, content);
        }
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

        let entry = HistoryEntry {
            id,
            file_path: file_path.to_path_buf(),
            timestamp,
            snapshot_path,
        };

        {
            let mut index = self.index.lock().unwrap();
            index
                .entry(file_path.to_path_buf())
                .or_default()
                .push(entry.clone());
        }
        self.save_index();

        Ok(entry)
    }

    pub fn revert_to(&self, entry_id: &str) -> Result<(), String> {
        let entry = {
            let index = self.index.lock().unwrap();
            let mut found = None;
            for entries in index.values() {
                if let Some(e) = entries.iter().find(|e| e.id == entry_id) {
                    found = Some(e.clone());
                    break;
                }
            }
            found
        };

        if let Some(entry) = entry {
            fs::copy(&entry.snapshot_path, &entry.file_path).map_err(|e| e.to_string())?;
            Ok(())
        } else {
            Err("Snapshot not found".to_string())
        }
    }

    pub fn get_history(&self, file_path: &Path) -> Vec<HistoryEntry> {
        let index = self.index.lock().unwrap();
        index.get(file_path).cloned().unwrap_or_default()
    }
}
