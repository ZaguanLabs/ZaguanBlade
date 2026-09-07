use serde::Serialize;
use std::path::Path;
use uuid::Uuid;

/// Stable project identity plus a unique lifetime. Returning to the same project
/// creates a new generation, so an old callback cannot become current again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkspaceIdentity {
    pub workspace_id: String,
    pub generation: Uuid,
}

impl WorkspaceIdentity {
    pub fn new(canonical_root: &Path) -> Self {
        Self {
            workspace_id: super::store::fingerprint(canonical_root.as_os_str().as_encoded_bytes()),
            generation: Uuid::new_v4(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returning_to_a_workspace_never_reuses_an_old_generation() {
        let root = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let first = WorkspaceIdentity::new(root.path());
        let reopened = WorkspaceIdentity::new(root.path());
        assert_eq!(first.workspace_id, reopened.workspace_id);
        assert_ne!(first.generation, reopened.generation);
        assert_ne!(
            first.workspace_id,
            WorkspaceIdentity::new(other.path()).workspace_id
        );
    }
}
