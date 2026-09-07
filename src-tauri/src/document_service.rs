//! Workspace document snapshots, independent of symbol parsing and persistence.
//!
//! Editor synchronization writes here even when code intelligence is unavailable.
//! Disk reads are transient: indexing a workspace must not retain every file in RAM.
//! This is a storage service, not a filesystem permission boundary; callers exposing
//! it to external agents must authorize paths before reading them.

use crate::buffer_snapshot::{BufferSnapshot, BufferSnapshotSource};
use crate::worktree::normalize_path;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub struct DocumentService {
    workspace_root: PathBuf,
    identity: crate::integrations::identity::WorkspaceIdentity,
    cancellation: tokio_util::sync::CancellationToken,
    live: RwLock<HashMap<String, Arc<BufferSnapshot>>>,
}

impl DocumentService {
    pub fn new(workspace_root: PathBuf) -> Self {
        let workspace_root = std::fs::canonicalize(&workspace_root)
            .unwrap_or_else(|_| normalize_path(&workspace_root));
        Self {
            identity: crate::integrations::identity::WorkspaceIdentity::new(&workspace_root),
            cancellation: tokio_util::sync::CancellationToken::new(),
            workspace_root,
            live: RwLock::new(HashMap::new()),
        }
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    pub fn identity(&self) -> &crate::integrations::identity::WorkspaceIdentity {
        &self.identity
    }

    pub fn cancellation(&self) -> tokio_util::sync::CancellationToken {
        self.cancellation.clone()
    }

    /// Retire this generation even if in-flight work still holds its documents.
    pub fn retire(&self) {
        self.cancellation.cancel();
    }

    pub(crate) fn snapshot_key(&self, file_path: &str) -> String {
        let path = self.resolve_path(file_path);
        let normalized = std::fs::canonicalize(&path).unwrap_or_else(|_| normalize_path(&path));
        normalized
            .strip_prefix(&self.workspace_root)
            .unwrap_or(&normalized)
            .to_string_lossy()
            .replace('\\', "/")
    }

    fn resolve_path(&self, file_path: &str) -> PathBuf {
        self.workspace_root.join(file_path)
    }

    /// Keep the newest editor version. Repeated notifications reuse the snapshot.
    /// An unversioned open cannot overwrite an already-versioned document; close
    /// the old document before opening a new editing session for the same path.
    pub fn sync(
        &self,
        file_path: &str,
        version: Option<i32>,
        content: &str,
    ) -> io::Result<Arc<BufferSnapshot>> {
        let key = self.snapshot_key(file_path);
        let mut live = self.live.write().map_err(|_| lock_error())?;
        if let Some(existing) = live.get(&key) {
            if version < existing.version()
                || (version.is_some() && version == existing.version())
                || (version == existing.version() && content == existing.content())
            {
                return Ok(Arc::clone(existing));
            }
        }
        let snapshot = Arc::new(BufferSnapshot::new(
            key.clone(),
            version,
            content,
            BufferSnapshotSource::Live,
        ));
        live.insert(key, Arc::clone(&snapshot));
        Ok(snapshot)
    }

    pub fn live_snapshot(&self, file_path: &str) -> io::Result<Option<Arc<BufferSnapshot>>> {
        self.with_live_snapshot(file_path, |snapshot| snapshot.cloned())
    }

    /// Read live editor content first, otherwise read a fresh, uncached disk copy.
    pub fn read(&self, file_path: &str) -> io::Result<Arc<BufferSnapshot>> {
        if let Some(snapshot) = self.live_snapshot(file_path)? {
            return Ok(snapshot);
        }
        let content = std::fs::read_to_string(self.resolve_path(file_path))?;
        // The editor may have opened/changed this file while the disk read ran.
        self.with_live_snapshot(file_path, |snapshot| {
            snapshot.cloned().unwrap_or_else(|| {
                Arc::new(BufferSnapshot::new(
                    file_path,
                    None,
                    content,
                    BufferSnapshotSource::Disk,
                ))
            })
        })
    }

    /// Retire a saved snapshot only if it still describes the saved bytes.
    /// A newer unsaved editor edit must survive a delayed disk-save notification.
    pub fn saved(&self, file_path: &str, content: &str) -> io::Result<()> {
        let key = self.snapshot_key(file_path);
        let mut live = self.live.write().map_err(|_| lock_error())?;
        if live
            .get(&key)
            .is_some_and(|snapshot| snapshot.content() == content)
        {
            live.remove(&key);
        }
        Ok(())
    }

    pub fn close(&self, file_path: &str) -> io::Result<()> {
        self.live
            .write()
            .map_err(|_| lock_error())?
            .remove(&self.snapshot_key(file_path));
        Ok(())
    }

    pub(crate) fn live_paths(&self) -> io::Result<Vec<String>> {
        Ok(self
            .live
            .read()
            .map_err(|_| lock_error())?
            .keys()
            .cloned()
            .collect())
    }

    /// Publish derived state only while the observed snapshot is still current.
    /// The short callback must not re-enter this document service.
    pub(crate) fn with_live_snapshot<T>(
        &self,
        file_path: &str,
        action: impl FnOnce(Option<&Arc<BufferSnapshot>>) -> T,
    ) -> io::Result<T> {
        let key = self.snapshot_key(file_path);
        let live = self.live.read().map_err(|_| lock_error())?;
        Ok(action(live.get(&key)))
    }
}

fn lock_error() -> io::Error {
    io::Error::other("document snapshot lock is unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_sync_does_not_create_an_index_or_project_data() {
        let root = tempfile::tempdir().unwrap();
        let documents = DocumentService::new(root.path().to_path_buf());
        documents
            .sync("new.unsupported", Some(1), "unsaved text")
            .unwrap();
        assert_eq!(
            documents.read("new.unsupported").unwrap().content(),
            "unsaved text"
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn absolute_relative_and_normalized_paths_share_a_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let documents = DocumentService::new(root.path().to_path_buf());
        let original = documents.sync("new.ts", Some(2), "latest").unwrap();
        let absolute = root.path().join("new.ts");
        assert!(Arc::ptr_eq(
            &original,
            &documents.read(&absolute.to_string_lossy()).unwrap()
        ));
        assert!(Arc::ptr_eq(
            &original,
            &documents.read("nested/../new.ts").unwrap()
        ));
    }

    #[test]
    fn stale_and_duplicate_notifications_do_not_replace_newer_content() {
        let root = tempfile::tempdir().unwrap();
        let documents = DocumentService::new(root.path().to_path_buf());
        let current = documents.sync("file.ts", Some(3), "latest").unwrap();
        for version in [None, Some(1), Some(2)] {
            assert!(Arc::ptr_eq(
                &current,
                &documents.sync("file.ts", version, "stale").unwrap()
            ));
        }
        assert!(Arc::ptr_eq(
            &current,
            &documents.sync("file.ts", Some(3), "latest").unwrap()
        ));
        assert!(Arc::ptr_eq(
            &current,
            &documents
                .sync("file.ts", Some(3), "conflicting duplicate")
                .unwrap()
        ));
        assert_eq!(
            documents
                .sync("file.ts", Some(4), "latest")
                .unwrap()
                .version(),
            Some(4)
        );
    }

    #[test]
    fn save_preserves_a_newer_unsaved_edit_and_close_restores_disk_reads() {
        let root = tempfile::tempdir().unwrap();
        let documents = DocumentService::new(root.path().to_path_buf());
        std::fs::write(root.path().join("file.ts"), "saved").unwrap();
        documents.sync("file.ts", Some(2), "newer edit").unwrap();
        documents.saved("file.ts", "saved").unwrap();
        assert_eq!(documents.read("file.ts").unwrap().content(), "newer edit");
        documents.close("file.ts").unwrap();
        assert_eq!(documents.read("file.ts").unwrap().content(), "saved");
        std::fs::write(root.path().join("file.ts"), "changed on disk").unwrap();
        assert_eq!(
            documents.read("file.ts").unwrap().content(),
            "changed on disk"
        );
        assert!(documents.live_paths().unwrap().is_empty());
        documents.sync("file.ts", Some(1), "reopened").unwrap();
        documents.saved("file.ts", "reopened").unwrap();
        assert!(documents.live_snapshot("file.ts").unwrap().is_none());
    }

    #[test]
    fn separate_workspaces_and_retained_old_handles_cannot_share_content() {
        let old_root = tempfile::tempdir().unwrap();
        let new_root = tempfile::tempdir().unwrap();
        let old = Arc::new(DocumentService::new(old_root.path().to_path_buf()));
        let new = DocumentService::new(new_root.path().to_path_buf());
        old.sync("same.ts", Some(1), "old workspace").unwrap();
        new.sync("same.ts", Some(1), "new workspace").unwrap();
        old.sync("same.ts", Some(2), "late old event").unwrap();
        assert_eq!(new.read("same.ts").unwrap().content(), "new workspace");
    }
}
