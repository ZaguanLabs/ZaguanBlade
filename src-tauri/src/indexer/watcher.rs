use crate::indexer::types::{FileMetadata, ProjectIndex, is_code_file};
use crate::indexer::builder::build_tree;
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub struct IndexWatcher {
    _watcher: RecommendedWatcher,
}

impl IndexWatcher {
    pub fn new(index: Arc<RwLock<ProjectIndex>>) -> Result<Self, Box<dyn std::error::Error>> {
        let (tx, rx) = mpsc::channel();
        
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            Config::default(),
        )?;
        
        let root = {
            let idx = index.read().unwrap();
            idx.root.clone()
        };
        
        watcher.watch(&root, RecursiveMode::Recursive)?;
        
        std::thread::spawn(move || {
            debounced_update_loop(index, rx);
        });
        
        Ok(Self { _watcher: watcher })
    }
}

fn debounced_update_loop(
    index: Arc<RwLock<ProjectIndex>>,
    rx: mpsc::Receiver<Event>,
) {
    let mut pending_changes: HashSet<PathBuf> = HashSet::new();
    let mut last_change = Instant::now();
    let debounce_duration = Duration::from_secs(2);
    
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(event) => {
                let paths = extract_paths(&event);
                pending_changes.extend(paths);
                last_change = Instant::now();
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !pending_changes.is_empty() && last_change.elapsed() > debounce_duration {
                    apply_changes(&index, &pending_changes);
                    pending_changes.clear();
                }
            }
        }
    }
}

fn extract_paths(event: &Event) -> Vec<PathBuf> {
    event.paths.iter()
        .filter(|p| is_code_file(p))
        .cloned()
        .collect()
}

fn apply_changes(index: &Arc<RwLock<ProjectIndex>>, paths: &HashSet<PathBuf>) {
    let mut idx = match index.write() {
        Ok(guard) => guard,
        Err(e) => {
            eprintln!("Failed to acquire write lock: {}", e);
            return;
        }
    };
    
    for path in paths {
        if path.exists() {
            match FileMetadata::from_path(path) {
                Ok(metadata) => {
                    idx.update_file(path.clone(), metadata);
                }
                Err(e) => {
                    eprintln!("Failed to read metadata for {:?}: {}", path, e);
                }
            }
        } else {
            idx.remove_file(path);
        }
    }
    
    let root = idx.root.clone();
    idx.tree = build_tree(&idx.files, &root);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::builder::index_workspace;
    use std::fs;
    use std::thread::sleep;
    use tempfile::TempDir;

    #[test]
    fn test_watcher_detects_new_file() {
        let temp_dir = TempDir::new().unwrap();
        let index = Arc::new(RwLock::new(index_workspace(temp_dir.path()).unwrap()));
        
        let _watcher = IndexWatcher::new(index.clone()).unwrap();
        
        let new_file = temp_dir.path().join("new.rs");
        fs::write(&new_file, "fn test() {}").unwrap();
        
        sleep(Duration::from_secs(3));
        
        let idx = index.read().unwrap();
        assert!(idx.files.contains_key(&new_file));
    }

    #[test]
    fn test_watcher_detects_file_deletion() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").unwrap();
        
        let index = Arc::new(RwLock::new(index_workspace(temp_dir.path()).unwrap()));
        let _watcher = IndexWatcher::new(index.clone()).unwrap();
        
        fs::remove_file(&test_file).unwrap();
        
        sleep(Duration::from_secs(3));
        
        let idx = index.read().unwrap();
        assert!(!idx.files.contains_key(&test_file));
    }
}
