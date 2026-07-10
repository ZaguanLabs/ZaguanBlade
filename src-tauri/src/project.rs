use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// Stable workspace identity stored in `.zblade/project.json`.
#[derive(Debug, Serialize, Deserialize)]
struct ProjectManifest {
    project_id: String,
}

/// Get or create the project manifest for a workspace
pub fn get_existing_project_id(workspace_root: &Path) -> Option<String> {
    let zblade_dir = workspace_root.join(".zblade");
    let manifest_path = zblade_dir.join("project.json");

    if manifest_path.exists() {
        match fs::read_to_string(&manifest_path) {
            Ok(content) => {
                if let Ok(manifest) = serde_json::from_str::<ProjectManifest>(&content) {
                    if is_valid_project_id(&manifest.project_id) {
                        return Some(manifest.project_id);
                    } else {
                        eprintln!("Invalid project ID in manifest, regenerating");
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to read manifest: {}", e);
            }
        }
    }

    None
}

/// Get or create the project manifest for a workspace
pub fn get_or_create_project_id(
    workspace_root: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(project_id) = get_existing_project_id(workspace_root) {
        return Ok(project_id);
    }

    let zblade_dir = workspace_root.join(".zblade");
    let manifest_path = zblade_dir.join("project.json");

    // Create new project
    let project_id = generate_project_id();
    let manifest = ProjectManifest {
        project_id: project_id.clone(),
    };

    eprintln!("Creating new project: {}", project_id);

    // Create .zblade directory
    fs::create_dir_all(&zblade_dir)?;

    // Write manifest
    let manifest_json = serde_json::to_string_pretty(&manifest)?;
    fs::write(&manifest_path, manifest_json)?;

    // Create .gitignore to exclude temporary files
    let gitignore_path = zblade_dir.join(".gitignore");
    let gitignore_content = "# zblade internal files (temporary)\n*.tmp\n*.cache\n*.lock\n";
    fs::write(gitignore_path, gitignore_content)?;

    eprintln!("Created .zblade/project.json with ID: {}", project_id);

    Ok(project_id)
}

/// Generate a unique project ID
fn generate_project_id() -> String {
    let uuid = Uuid::new_v4();
    format!("proj_{}", uuid.simple())
}

/// Validate project ID format
fn is_valid_project_id(id: &str) -> bool {
    if id.len() < 10 || id.len() > 100 {
        return false;
    }
    if !id.starts_with("proj_") {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_generate_project_id() {
        let id = generate_project_id();
        assert!(id.starts_with("proj_"));
        assert!(id.len() > 10);
    }

    #[test]
    fn test_is_valid_project_id() {
        assert!(is_valid_project_id("proj_abc123def456"));
        assert!(!is_valid_project_id("invalid"));
        assert!(!is_valid_project_id("proj_"));
        assert!(!is_valid_project_id(""));
    }

    #[test]
    fn test_get_or_create_project_id() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path();

        // First call should create manifest
        let id1 = get_or_create_project_id(workspace).unwrap();
        assert!(id1.starts_with("proj_"));

        // Verify .zblade directory was created
        assert!(workspace.join(".zblade").exists());
        assert!(workspace.join(".zblade/project.json").exists());
        assert!(workspace.join(".zblade/.gitignore").exists());

        // Second call should return same ID
        let id2 = get_or_create_project_id(workspace).unwrap();
        assert_eq!(id1, id2);

        // Verify manifest content
        let manifest_content = fs::read_to_string(workspace.join(".zblade/project.json")).unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&manifest_content).unwrap();
        assert_eq!(manifest, serde_json::json!({ "project_id": id1 }));
    }

    #[test]
    fn test_existing_legacy_manifest_keeps_project_id() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path();
        let zblade_dir = workspace.join(".zblade");
        fs::create_dir_all(&zblade_dir).unwrap();
        fs::write(
            zblade_dir.join("project.json"),
            r#"{
                "project_id": "proj_abc123def456",
                "created_at": "2026-02-12T19:36:55Z",
                "name": "legacy-project",
                "version": "1.0.0",
                "zblade_version": "0.1.0"
            }"#,
        )
        .unwrap();

        assert_eq!(
            get_existing_project_id(workspace),
            Some("proj_abc123def456".to_string())
        );
    }

    #[test]
    fn test_get_existing_project_id_without_manifest() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path();

        assert_eq!(get_existing_project_id(workspace), None);
        assert!(!workspace.join(".zblade").exists());
    }

    #[test]
    fn test_invalid_manifest_regenerates() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path();
        let zblade_dir = workspace.join(".zblade");
        let manifest_path = zblade_dir.join("project.json");

        // Create directory and invalid manifest
        fs::create_dir_all(&zblade_dir).unwrap();
        fs::write(&manifest_path, r#"{"project_id": "invalid"}"#).unwrap();

        // Should regenerate with valid ID
        let id = get_or_create_project_id(workspace).unwrap();
        assert!(id.starts_with("proj_"));
        assert_ne!(id, "invalid");
    }
}
