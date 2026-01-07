use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub children: Option<Vec<FileEntry>>,
}

pub fn list_directory(path: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();

    if let Ok(read_dir) = std::fs::read_dir(path) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = path.is_dir();

            // Skip hidden files/dirs (simple check)
            if name.starts_with('.') {
                continue;
            }

            let children = None;
            if is_dir {
                // Recursive call
                // Limit depth effectively by UI lazy loading?
                // For now, let's do a shallow listing or 1-level deep?
                // Or maybe the frontend requests specific paths?
                // A full recursive tree for a massive project is slow.
                // Let's implement a "shallow" list for now, and the frontend can request children.
                // Actually, let's effectively do purely shallow here.
            }

            entries.push(FileEntry {
                name,
                path: path.to_string_lossy().to_string(),
                is_dir,
                children,
            });
        }
    }

    // Sort directories first, then files
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    entries
}
