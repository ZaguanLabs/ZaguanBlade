use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::history::HistoryService;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UncommittedChange {
    pub id: String,
    pub file_path: PathBuf,
    pub snapshot_id: String,
    pub unified_diff: String,
    pub added_lines: usize,
    pub removed_lines: usize,
    pub timestamp: u64,
}

pub struct UncommittedChangeTracker {
    changes: Mutex<HashMap<String, UncommittedChange>>,
}

impl UncommittedChangeTracker {
    pub fn new() -> Self {
        Self {
            changes: Mutex::new(HashMap::new()),
        }
    }

    pub fn track(&self, change: UncommittedChange) {
        let mut changes = self.changes.lock().unwrap();
        changes.insert(change.id.clone(), change);
    }

    pub fn get(&self, id: &str) -> Option<UncommittedChange> {
        let changes = self.changes.lock().unwrap();
        changes.get(id).cloned()
    }

    pub fn get_by_path(&self, path: &PathBuf) -> Option<UncommittedChange> {
        let changes = self.changes.lock().unwrap();
        changes.values().find(|c| &c.file_path == path).cloned()
    }

    pub fn get_all(&self) -> Vec<UncommittedChange> {
        let changes = self.changes.lock().unwrap();
        let mut result: Vec<_> = changes.values().cloned().collect();
        result.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        result
    }

    pub fn accept(&self, id: &str) -> Option<UncommittedChange> {
        let mut changes = self.changes.lock().unwrap();
        changes.remove(id)
    }

    pub fn accept_by_path(&self, path: &PathBuf) -> Option<UncommittedChange> {
        let mut changes = self.changes.lock().unwrap();
        let id = changes
            .values()
            .find(|c| &c.file_path == path)
            .map(|c| c.id.clone());
        if let Some(id) = id {
            changes.remove(&id)
        } else {
            None
        }
    }

    pub fn accept_all(&self) -> Vec<UncommittedChange> {
        let mut changes = self.changes.lock().unwrap();
        let all: Vec<_> = changes.drain().map(|(_, v)| v).collect();
        all
    }

    pub fn reject(
        &self,
        id: &str,
        history_service: &HistoryService,
    ) -> Result<UncommittedChange, String> {
        let change = {
            let mut changes = self.changes.lock().unwrap();
            changes.remove(id)
        };

        match change {
            Some(c) => {
                history_service.revert_to(&c.snapshot_id)?;
                Ok(c)
            }
            None => Err(format!("Change not found: {}", id)),
        }
    }

    pub fn reject_by_path(
        &self,
        path: &PathBuf,
        history_service: &HistoryService,
    ) -> Result<UncommittedChange, String> {
        let change = {
            let mut changes = self.changes.lock().unwrap();
            let id = changes
                .values()
                .find(|c| &c.file_path == path)
                .map(|c| c.id.clone());
            if let Some(id) = id {
                changes.remove(&id)
            } else {
                None
            }
        };

        match change {
            Some(c) => {
                history_service.revert_to(&c.snapshot_id)?;
                Ok(c)
            }
            None => Err(format!("No uncommitted change for path: {:?}", path)),
        }
    }

    pub fn reject_all(
        &self,
        history_service: &HistoryService,
    ) -> Result<Vec<UncommittedChange>, String> {
        let all_changes = {
            let mut changes = self.changes.lock().unwrap();
            changes.drain().map(|(_, v)| v).collect::<Vec<_>>()
        };

        let mut rejected = Vec::new();
        let mut errors = Vec::new();

        for change in all_changes {
            match history_service.revert_to(&change.snapshot_id) {
                Ok(_) => rejected.push(change),
                Err(e) => errors.push(format!("{}: {}", change.file_path.display(), e)),
            }
        }

        if errors.is_empty() {
            Ok(rejected)
        } else {
            Err(format!("Some reverts failed: {}", errors.join(", ")))
        }
    }

    pub fn count(&self) -> usize {
        let changes = self.changes.lock().unwrap();
        changes.len()
    }
}

impl Default for UncommittedChangeTracker {
    fn default() -> Self {
        Self::new()
    }
}

pub fn count_diff_stats(diff: &str) -> (usize, usize) {
    let mut added = 0;
    let mut removed = 0;

    for line in diff.lines() {
        if line.starts_with('+') && !line.starts_with("+++") {
            added += 1;
        } else if line.starts_with('-') && !line.starts_with("---") {
            removed += 1;
        }
    }

    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_count_diff_stats() {
        let diff = r#"--- a/file.txt
+++ b/file.txt
@@ -1,3 +1,4 @@
 line1
-old line
+new line
+added line
 line3
"#;
        let (added, removed) = count_diff_stats(diff);
        assert_eq!(added, 2);
        assert_eq!(removed, 1);
    }

    #[test]
    fn test_tracker_basic() {
        let tracker = UncommittedChangeTracker::new();

        let change = UncommittedChange {
            id: "test-1".to_string(),
            file_path: PathBuf::from("/test/file.rs"),
            snapshot_id: "snap-1".to_string(),
            unified_diff: "+added\n-removed".to_string(),
            added_lines: 1,
            removed_lines: 1,
            timestamp: 12345,
        };

        tracker.track(change.clone());
        assert_eq!(tracker.count(), 1);

        let retrieved = tracker.get("test-1").unwrap();
        assert_eq!(retrieved.file_path, PathBuf::from("/test/file.rs"));

        let accepted = tracker.accept("test-1").unwrap();
        assert_eq!(accepted.id, "test-1");
        assert_eq!(tracker.count(), 0);
    }
}
