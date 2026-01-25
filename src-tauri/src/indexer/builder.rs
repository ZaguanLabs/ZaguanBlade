use crate::indexer::types::{DirectoryTree, FileMetadata, ProjectIndex, is_code_file};
use ignore::WalkBuilder;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn index_workspace(root: &Path) -> Result<ProjectIndex, Box<dyn std::error::Error>> {
    let mut index = ProjectIndex::new(root.to_path_buf());
    
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .build();
    
    for entry in walker {
        let entry = entry?;
        let path = entry.path();
        
        if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            if is_code_file(&path.to_path_buf()) {
                match FileMetadata::from_path(&path.to_path_buf()) {
                    Ok(metadata) => {
                        index.files.insert(path.to_path_buf(), metadata);
                    }
                    Err(e) => {
                        eprintln!("Failed to read metadata for {:?}: {}", path, e);
                    }
                }
            }
        }
    }
    
    index.tree = build_tree(&index.files, root);
    index.mark_clean();
    
    Ok(index)
}

pub fn build_tree(files: &HashMap<PathBuf, FileMetadata>, root: &Path) -> DirectoryTree {
    let mut root_tree = DirectoryTree::new(
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string()
    );
    
    let mut dir_map: HashMap<PathBuf, DirectoryTree> = HashMap::new();
    
    for path in files.keys() {
        if let Ok(rel_path) = path.strip_prefix(root) {
            let mut current_path = root.to_path_buf();
            
            for component in rel_path.parent().into_iter().flat_map(|p| p.components()) {
                current_path.push(component);
                
                if !dir_map.contains_key(&current_path) {
                    let name = component.as_os_str().to_string_lossy().to_string();
                    dir_map.insert(current_path.clone(), DirectoryTree::new(name));
                }
            }
        }
    }
    
    for path in files.keys() {
        if let Ok(rel_path) = path.strip_prefix(root) {
            if let Some(parent) = rel_path.parent() {
                let parent_path = root.join(parent);
                
                if let Some(dir_tree) = dir_map.get_mut(&parent_path) {
                    if let Some(file_name) = path.file_name() {
                        dir_tree.add_file(file_name.to_string_lossy().to_string());
                    }
                }
            } else {
                if let Some(file_name) = path.file_name() {
                    root_tree.add_file(file_name.to_string_lossy().to_string());
                }
            }
        }
    }
    
    let mut sorted_dirs: Vec<_> = dir_map.into_iter().collect();
    sorted_dirs.sort_by(|a, b| b.0.components().count().cmp(&a.0.components().count()));
    
    for (dir_path, dir_tree) in sorted_dirs {
        if let Some(parent) = dir_path.parent() {
            if parent == root {
                root_tree.add_child(dir_tree);
            } else {
                if let Some(parent_tree) = find_tree_mut(&mut root_tree, parent, root) {
                    parent_tree.add_child(dir_tree);
                }
            }
        }
    }
    
    root_tree
}

fn find_tree_mut<'a>(
    tree: &'a mut DirectoryTree,
    target_path: &Path,
    current_path: &Path,
) -> Option<&'a mut DirectoryTree> {
    if current_path == target_path {
        return Some(tree);
    }
    
    for child in &mut tree.children {
        let child_path = current_path.join(&child.name);
        if let Some(found) = find_tree_mut(child, target_path, &child_path) {
            return Some(found);
        }
    }
    
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_index_empty_workspace() {
        let temp_dir = TempDir::new().unwrap();
        let index = index_workspace(temp_dir.path()).unwrap();
        
        assert_eq!(index.file_count(), 0);
        assert_eq!(index.root, temp_dir.path());
    }

    #[test]
    fn test_index_with_code_files() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test.rs");
        fs::write(&test_file, "fn main() {}").unwrap();
        
        let index = index_workspace(temp_dir.path()).unwrap();
        
        assert_eq!(index.file_count(), 1);
        assert!(index.files.contains_key(&test_file));
    }

    #[test]
    fn test_ignores_non_code_files() {
        let temp_dir = TempDir::new().unwrap();
        fs::write(temp_dir.path().join("test.txt"), "text").unwrap();
        fs::write(temp_dir.path().join("test.rs"), "fn main() {}").unwrap();
        
        let index = index_workspace(temp_dir.path()).unwrap();
        
        assert_eq!(index.file_count(), 1);
    }

    #[test]
    fn test_build_tree_structure() {
        let temp_dir = TempDir::new().unwrap();
        let src_dir = temp_dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();
        
        let main_file = src_dir.join("main.rs");
        fs::write(&main_file, "fn main() {}").unwrap();
        
        let index = index_workspace(temp_dir.path()).unwrap();
        
        assert!(index.tree.children.iter().any(|c| c.name == "src"));
    }
}
