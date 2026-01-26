use crate::indexer::preview::get_or_load_preview;
use crate::indexer::types::{ProjectIndex, detect_language};
use chrono::Utc;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

pub struct GetFullContextOptions {
    pub max_files: usize,
    pub preview_lines: usize,
}

impl Default for GetFullContextOptions {
    fn default() -> Self {
        Self {
            max_files: 100,
            preview_lines: 50,
        }
    }
}

pub async fn handle_get_full_context(
    index: &Arc<RwLock<ProjectIndex>>,
    options: GetFullContextOptions,
) -> Result<String, Box<dyn std::error::Error>> {
    let (root, file_count, tree_render, file_paths) = {
        let idx = index.read().unwrap();
        
        let root = idx.root.clone();
        let file_count = idx.file_count();
        let tree_render = idx.tree.render(3);
        let file_paths: Vec<PathBuf> = idx.files.keys()
            .take(options.max_files)
            .cloned()
            .collect();
        
        (root, file_count, tree_render, file_paths)
    };
    
    let mut output = String::new();
    
    output.push_str(&format!("# Project Index: {}\n\n", 
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
    ));
    output.push_str(&format!("**Generated**: {}\n", Utc::now().format("%Y-%m-%d %H:%M:%S UTC")));
    output.push_str(&format!("**Total Files**: {}\n\n", file_count));
    
    output.push_str("## Directory Structure\n\n```\n");
    output.push_str(&tree_render);
    output.push_str("```\n\n");
    
    output.push_str("## File Previews\n\n");
    
    for path in file_paths {
        match get_or_load_preview(index, &path, options.preview_lines) {
            Ok(preview) => {
                let relative_path = path.strip_prefix(&root)
                    .unwrap_or(&path)
                    .display();
                
                output.push_str(&format!("### {}\n\n", relative_path));
                output.push_str(&format!("```{}\n", detect_language(&path)));
                output.push_str(&preview);
                output.push_str("\n```\n\n");
            }
            Err(e) => {
                eprintln!("Failed to load preview for {:?}: {}", path, e);
            }
        }
    }
    
    let output_dir = root.join(".zblade/context");
    fs::create_dir_all(&output_dir)?;
    
    let output_path = output_dir.join("project_index.md");
    fs::write(&output_path, &output)?;
    
    {
        let mut idx = index.write().unwrap();
        idx.mark_clean();
    }
    
    Ok(output_path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::builder::index_workspace;
    use std::fs;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_handle_get_full_context() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {\n    println!(\"Hello\");\n}").unwrap();
        
        let index = Arc::new(RwLock::new(index_workspace(temp_dir.path()).unwrap()));
        
        let options = GetFullContextOptions::default();
        let result = handle_get_full_context(&index, options).await.unwrap();
        
        assert!(result.contains(".zblade/context/project_index.md"));
        
        let output_path = PathBuf::from(&result);
        assert!(output_path.exists());
        
        let content = fs::read_to_string(&output_path).unwrap();
        assert!(content.contains("# Project Index"));
        assert!(content.contains("test.rs"));
        assert!(content.contains("fn main()"));
    }

    #[tokio::test]
    async fn test_marks_index_clean() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").unwrap();
        
        let index = Arc::new(RwLock::new(index_workspace(temp_dir.path()).unwrap()));
        
        {
            let mut idx = index.write().unwrap();
            idx.mark_dirty();
        }
        
        let options = GetFullContextOptions::default();
        handle_get_full_context(&index, options).await.unwrap();
        
        let idx = index.read().unwrap();
        assert!(!idx.dirty);
    }
}
