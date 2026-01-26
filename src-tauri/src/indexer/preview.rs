use crate::indexer::types::{CachedPreview, ProjectIndex};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub fn get_or_load_preview(
    index: &Arc<RwLock<ProjectIndex>>,
    path: &PathBuf,
    max_lines: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    {
        let idx = index.read().unwrap();
        if let Some(preview) = idx.previews.get(path) {
            if let Ok(metadata) = fs::metadata(path) {
                if let Ok(modified) = metadata.modified() {
                    if preview.is_valid(modified) {
                        return Ok(preview.lines.join("\n"));
                    }
                }
            }
        }
    }
    
    let content = fs::read_to_string(path)?;
    let lines: Vec<String> = content.lines().take(max_lines).map(String::from).collect();
    
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified()?;
    
    let preview = CachedPreview::new(lines.clone(), modified);
    let result = lines.join("\n");
    
    {
        let mut idx = index.write().unwrap();
        idx.previews.insert(path.clone(), preview);
    }
    
    Ok(result)
}

pub fn invalidate_preview(index: &Arc<RwLock<ProjectIndex>>, path: &PathBuf) {
    let mut idx = index.write().unwrap();
    idx.previews.remove(path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::builder::index_workspace;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_load_preview() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "line1\nline2\nline3").unwrap();
        
        let index = Arc::new(RwLock::new(index_workspace(temp_dir.path()).unwrap()));
        
        let preview = get_or_load_preview(&index, &test_file, 50).unwrap();
        assert_eq!(preview, "line1\nline2\nline3");
    }

    #[test]
    fn test_preview_caching() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "line1\nline2\nline3").unwrap();
        
        let index = Arc::new(RwLock::new(index_workspace(temp_dir.path()).unwrap()));
        
        let preview1 = get_or_load_preview(&index, &test_file, 50).unwrap();
        
        {
            let idx = index.read().unwrap();
            assert!(idx.previews.contains_key(&test_file));
        }
        
        let preview2 = get_or_load_preview(&index, &test_file, 50).unwrap();
        assert_eq!(preview1, preview2);
    }

    #[test]
    fn test_preview_max_lines() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "line1\nline2\nline3\nline4\nline5").unwrap();
        
        let index = Arc::new(RwLock::new(index_workspace(temp_dir.path()).unwrap()));
        
        let preview = get_or_load_preview(&index, &test_file, 3).unwrap();
        assert_eq!(preview, "line1\nline2\nline3");
    }

    #[test]
    fn test_invalidate_preview() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "line1").unwrap();
        
        let index = Arc::new(RwLock::new(index_workspace(temp_dir.path()).unwrap()));
        
        get_or_load_preview(&index, &test_file, 50).unwrap();
        
        invalidate_preview(&index, &test_file);
        
        let idx = index.read().unwrap();
        assert!(!idx.previews.contains_key(&test_file));
    }
}
