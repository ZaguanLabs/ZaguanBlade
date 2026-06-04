use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

fn default_file_existed() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub group_id: Option<String>,
    pub file_path: PathBuf,
    pub timestamp: u64,
    pub snapshot_path: PathBuf,
    #[serde(default = "default_file_existed")]
    pub file_existed: bool,
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

    pub fn create_snapshot(
        &self,
        file_path: &Path,
        group_id: Option<String>,
    ) -> Result<HistoryEntry, String> {
        let content = fs::read(file_path).map_err(|e| e.to_string())?;
        self.create_snapshot_from_bytes(file_path, group_id, &content, true)
    }

    pub fn create_missing_file_snapshot(
        &self,
        file_path: &Path,
        group_id: Option<String>,
    ) -> Result<HistoryEntry, String> {
        let content: &[u8] = b"";
        self.create_snapshot_from_bytes(file_path, group_id, content, false)
    }

    fn create_snapshot_from_bytes(
        &self,
        file_path: &Path,
        group_id: Option<String>,
        content: &[u8],
        file_existed: bool,
    ) -> Result<HistoryEntry, String> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let id = uuid::Uuid::new_v4().to_string();
        let snapshot_filename = format!("{}_{}", timestamp, id);
        let snapshot_path = self.history_root.join(&snapshot_filename);

        fs::write(&snapshot_path, content).map_err(|e| e.to_string())?;

        let entry = HistoryEntry {
            id,
            group_id,
            file_path: file_path.to_path_buf(),
            timestamp,
            snapshot_path,
            file_existed,
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
            if entry.file_existed {
                fs::copy(&entry.snapshot_path, &entry.file_path).map_err(|e| e.to_string())?;
            } else if entry.file_path.exists() {
                fs::remove_file(&entry.file_path).map_err(|e| e.to_string())?;
            }
            Ok(())
        } else {
            Err("Snapshot not found".to_string())
        }
    }

    pub fn get_snapshot_content(&self, entry_id: &str) -> Result<String, String> {
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

        match entry {
            Some(entry) => fs::read_to_string(&entry.snapshot_path).map_err(|e| e.to_string()),
            None => Err("Snapshot not found".to_string()),
        }
    }

    pub fn get_entry(&self, entry_id: &str) -> Option<HistoryEntry> {
        let index = self.index.lock().unwrap();
        for entries in index.values() {
            if let Some(entry) = entries.iter().find(|entry| entry.id == entry_id) {
                return Some(entry.clone());
            }
        }
        None
    }

    pub fn undo_batch(&self, group_id: &str) -> Result<Vec<String>, String> {
        let index = self.index.lock().unwrap();

        // Find all entries for this group
        let mut group_entries: Vec<HistoryEntry> = Vec::new();
        for entries in index.values() {
            for entry in entries {
                if let Some(gid) = &entry.group_id {
                    if gid == group_id {
                        group_entries.push(entry.clone());
                    }
                }
            }
        }

        if group_entries.is_empty() {
            return Err(format!(
                "No history entries found for group ID: {}",
                group_id
            ));
        }

        // Group by file path and find the earliest timestamp for each file
        let mut earliest_by_file: HashMap<PathBuf, HistoryEntry> = HashMap::new();

        for entry in group_entries {
            earliest_by_file
                .entry(entry.file_path.clone())
                .and_modify(|e| {
                    if entry.timestamp < e.timestamp {
                        *e = entry.clone();
                    }
                })
                .or_insert(entry);
        }

        let mut reverted_files = Vec::new();

        for (path, entry) in earliest_by_file {
            let result = if entry.file_existed {
                fs::copy(&entry.snapshot_path, &path).map(|_| ())
            } else if path.exists() {
                fs::remove_file(&path)
            } else {
                Ok(())
            };
            match result {
                Ok(()) => reverted_files.push(path.to_string_lossy().into_owned()),
                Err(e) => eprintln!("Failed to revert {}: {}", path.display(), e),
            }
        }

        Ok(reverted_files)
    }

    pub fn get_history(&self, file_path: &Path) -> Vec<HistoryEntry> {
        let index = self.index.lock().unwrap();
        index.get(file_path).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::HistoryService;
    use tempfile::tempdir;

    #[test]
    fn missing_file_snapshot_reverts_by_deleting_created_file() {
        let app_data_dir = tempdir().expect("app data dir");
        let workspace = tempdir().expect("workspace");
        let file_path = workspace.path().join("new-file.txt");
        let history = HistoryService::new(app_data_dir.path());

        let entry = history
            .create_missing_file_snapshot(&file_path, Some("change-1".to_string()))
            .expect("create missing file snapshot");
        std::fs::write(&file_path, "created\n").expect("write created file");

        history
            .revert_to(&entry.id)
            .expect("revert missing file snapshot");

        assert!(!file_path.exists());
    }
}
