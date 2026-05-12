use serde::{Deserialize, Serialize};
use similar::{Algorithm, TextDiff};
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
    #[serde(default)]
    pub file_modified_ms: Option<u64>,
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
        let file_path = change.file_path.clone();

        // Keep one canonical tracked change per file. If the same file is edited
        // multiple times, we replace the previous entry with the newest cumulative
        // representation rather than keeping multiple stale entries.
        changes.retain(|_, existing| existing.file_path != file_path);
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

    pub fn replace_all(&self, incoming: Vec<UncommittedChange>) {
        let mut changes = self.changes.lock().unwrap();
        changes.clear();
        for change in incoming {
            changes.insert(change.id.clone(), change);
        }
    }

    pub fn clear(&self) {
        let mut changes = self.changes.lock().unwrap();
        changes.clear();
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
            let changes = self.changes.lock().unwrap();
            changes.get(id).cloned()
        };

        match change {
            Some(c) => {
                history_service.revert_to(&c.snapshot_id)?;
                let mut changes = self.changes.lock().unwrap();
                changes.remove(id);
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
            let changes = self.changes.lock().unwrap();
            changes
                .values()
                .find(|c| &c.file_path == path)
                .cloned()
        };

        match change {
            Some(c) => {
                history_service.revert_to(&c.snapshot_id)?;
                let mut changes = self.changes.lock().unwrap();
                changes.remove(&c.id);
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
            let changes = self.changes.lock().unwrap();
            changes.values().cloned().collect::<Vec<_>>()
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
            let mut changes = self.changes.lock().unwrap();
            for change in &rejected {
                changes.remove(&change.id);
            }
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

pub fn generate_unified_diff(base_content: &str, new_content: &str) -> String {
    // Patience diff is much more stable for repeated boilerplate blocks
    // (e.g. changelog sections) and avoids under-highlighting large insertions.
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_lines(base_content, new_content);

    diff.unified_diff()
        .context_radius(3)
        .header("a/file", "b/file")
        .to_string()
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

pub fn file_modified_ms(path: &PathBuf) -> Option<u64> {
    let metadata = std::fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?;
    modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
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
            file_modified_ms: None,
        };

        tracker.track(change.clone());
        assert_eq!(tracker.count(), 1);

        let retrieved = tracker.get("test-1").unwrap();
        assert_eq!(retrieved.file_path, PathBuf::from("/test/file.rs"));

        let accepted = tracker.accept("test-1").unwrap();
        assert_eq!(accepted.id, "test-1");
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn test_generate_unified_diff_marks_full_repeated_block_insertions() {
        let base = r#"<section class=\"changelog-section\">
  <div class=\"version-card\">
    <div class=\"version-header\">
      <h2 class=\"version-number\">v0.3.1</h2>
      <span class=\"version-date\">February 25, 2026</span>
    </div>
    <div class=\"version-content\">
      <ul class=\"changes-list\">
        <li>Previous entry</li>
      </ul>
    </div>
  </div>
</section>
"#;

        let next = r#"<section class=\"changelog-section\">
  <div class=\"version-card version-dev\">
    <div class=\"version-header\">
      <h2 class=\"version-number\">v0.3.2</h2>
      <span class=\"version-date\">TBD</span>
    </div>
    <div class=\"version-content\">
      <ul class=\"changes-list\">
        <li>Welcome screen alignment fix</li>
        <li>Clipboard image paste fix</li>
        <li>Keep focus on active tab</li>
      </ul>
    </div>
  </div>
</section>

<section class=\"changelog-section\">
  <div class=\"version-card\">
    <div class=\"version-header\">
      <h2 class=\"version-number\">v0.3.1</h2>
      <span class=\"version-date\">February 25, 2026</span>
    </div>
    <div class=\"version-content\">
      <ul class=\"changes-list\">
        <li>Previous entry</li>
      </ul>
    </div>
  </div>
</section>
"#;

        let diff = generate_unified_diff(base, next);
        let (added, removed) = count_diff_stats(&diff);

        assert!(
            added >= 10,
            "expected multi-line insertion to be represented in unified diff, got {added} added lines. diff:\n{diff}"
        );
        assert_eq!(
            removed, 0,
            "insertion-only change should not remove lines. diff:\n{diff}"
        );
    }
}
