use crate::indexer::types::ProjectIndex;
use std::fs;
use std::path::Path;

pub fn save_cache(index: &ProjectIndex) -> Result<(), Box<dyn std::error::Error>> {
    let cache_dir = index.root.join(".zblade/cache");
    fs::create_dir_all(&cache_dir)?;
    
    let cache_path = cache_dir.join("index.json");
    let json = serde_json::to_string(index)?;
    fs::write(&cache_path, json)?;
    
    Ok(())
}

pub fn load_cache(root: &Path) -> Result<ProjectIndex, Box<dyn std::error::Error>> {
    let cache_path = root.join(".zblade/cache/index.json");
    let json = fs::read_to_string(&cache_path)?;
    let mut index: ProjectIndex = serde_json::from_str(&json)?;
    
    if !is_cache_valid(&index) {
        return Err("Cache is stale".into());
    }
    
    index.root = root.to_path_buf();
    
    Ok(index)
}

fn is_cache_valid(index: &ProjectIndex) -> bool {
    for (path, metadata) in &index.files {
        if !path.exists() {
            return false;
        }
        
        if let Ok(current_metadata) = fs::metadata(path) {
            if let Ok(current_modified) = current_metadata.modified() {
                if current_modified != metadata.modified {
                    return false;
                }
            } else {
                return false;
            }
        } else {
            return false;
        }
    }
    
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::builder::index_workspace;
    use std::fs;
    use std::thread::sleep;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn test_save_and_load_cache() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").unwrap();
        
        let index = index_workspace(temp_dir.path()).unwrap();
        
        save_cache(&index).unwrap();
        
        let loaded = load_cache(temp_dir.path()).unwrap();
        assert_eq!(loaded.file_count(), index.file_count());
    }

    #[test]
    fn test_cache_invalidation_on_file_change() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").unwrap();
        
        let index = index_workspace(temp_dir.path()).unwrap();
        save_cache(&index).unwrap();
        
        sleep(Duration::from_millis(100));
        fs::write(&test_file, "fn main() { println!(\"changed\"); }").unwrap();
        
        let result = load_cache(temp_dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn test_cache_invalidation_on_file_deletion() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").unwrap();
        
        let index = index_workspace(temp_dir.path()).unwrap();
        save_cache(&index).unwrap();
        
        fs::remove_file(&test_file).unwrap();
        
        let result = load_cache(temp_dir.path());
        assert!(result.is_err());
    }
}
