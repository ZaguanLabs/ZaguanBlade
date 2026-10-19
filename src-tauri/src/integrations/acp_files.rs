//! Workspace files an ACP agent reads and writes through Blade
//! (`fs/read_text_file`, `fs/write_text_file`). Reads see the editor's unsaved
//! text. A write never replaces unsaved editor changes, and every write is
//! recorded so the user can review and reject it like a native AI edit.
//! Paths must be absolute and stay inside the workspace once symlinks are
//! resolved.
use crate::document_service::DocumentService;
use std::{
    io,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

/// Larger files are refused rather than read into memory.
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
/// What one read may return; the agent can ask for a line range instead.
const MAX_READ_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FileError {
    NotAbsolute,
    OutsideWorkspace,
    /// `.git` or Blade's own `.zblade` settings.
    Protected,
    NotFound,
    NotText,
    TooLarge,
    /// The editor holds unsaved changes that differ from the file on disk.
    UnsavedChanges,
    WorkspaceChanged,
    Failed,
}

impl FileError {
    fn from_io(error: &io::Error) -> Self {
        match error.kind() {
            io::ErrorKind::NotFound => Self::NotFound,
            io::ErrorKind::InvalidData => Self::NotText,
            _ => Self::Failed,
        }
    }

    /// The JSON-RPC error the agent receives. Messages are for the agent's model.
    pub fn to_protocol(self, path: &Path) -> agent_client_protocol::Error {
        use agent_client_protocol::Error;
        let shown = path.display();
        match self {
            Self::NotFound => Error::resource_not_found(Some(format!("file://{shown}"))),
            Self::NotAbsolute => Error::new(-32602, "The path must be absolute."),
            Self::OutsideWorkspace => {
                Error::new(-32602, format!("{shown} is outside the workspace."))
            }
            Self::Protected => {
                Error::new(-32602, format!("{shown} is protected; change it yourself."))
            }
            Self::NotText => Error::new(-32602, format!("{shown} is not a UTF-8 text file.")),
            Self::TooLarge => Error::new(
                -32602,
                format!("{shown} is too large; read a range with line and limit."),
            ),
            Self::UnsavedChanges => Error::new(
                -32603,
                format!(
                    "{shown} has unsaved changes in the user's editor, so it was not written. \
                     Ask the user to save or discard them."
                ),
            ),
            Self::WorkspaceChanged => Error::new(-32603, "The workspace was closed."),
            Self::Failed => Error::internal_error(),
        }
    }
}

/// Blade's bookkeeping around one agent write: a history snapshot before it,
/// the reviewable change record and the editor refresh after it.
pub trait WriteJournal: Send + Sync {
    /// `before` is `None` for a new file. `write` changes the file.
    fn record(
        &self,
        path: &Path,
        before: Option<&str>,
        write: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<()>;
}

/// Writes without any bookkeeping; for tests.
#[cfg(test)]
pub struct NoJournal;
#[cfg(test)]
impl WriteJournal for NoJournal {
    fn record(
        &self,
        _path: &Path,
        _before: Option<&str>,
        write: &mut dyn FnMut() -> io::Result<()>,
    ) -> io::Result<()> {
        write()
    }
}

pub struct WorkspaceFiles {
    documents: Arc<DocumentService>,
    journal: Arc<dyn WriteJournal>,
}

fn has_parent_or_current(path: &Path) -> bool {
    path.components()
        .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
}

fn protected(relative: &Path) -> bool {
    let mut parts = relative.components();
    matches!(parts.next(), Some(Component::Normal(first)) if first == ".zblade")
        || relative
            .components()
            .any(|part| matches!(part, Component::Normal(name) if name == ".git"))
}

impl WorkspaceFiles {
    pub fn new(documents: Arc<DocumentService>, journal: Arc<dyn WriteJournal>) -> Self {
        Self { documents, journal }
    }

    fn root(&self) -> &Path {
        self.documents.workspace_root()
    }

    fn check_open(&self) -> Result<(), FileError> {
        if self.documents.cancellation().is_cancelled() {
            return Err(FileError::WorkspaceChanged);
        }
        Ok(())
    }

    /// The real location of `path`, which must exist inside the workspace.
    fn existing(&self, path: &Path) -> Result<PathBuf, FileError> {
        if !path.is_absolute() {
            return Err(FileError::NotAbsolute);
        }
        let real = std::fs::canonicalize(path).map_err(|error| FileError::from_io(&error))?;
        if !real.starts_with(self.root()) {
            return Err(FileError::OutsideWorkspace);
        }
        if !real.is_file() {
            return Err(FileError::NotFound);
        }
        Ok(real)
    }

    /// Where a write lands: the real location of an existing file, or of the
    /// nearest existing folder plus the rest of the path for a new one.
    fn target(&self, path: &Path) -> Result<PathBuf, FileError> {
        if !path.is_absolute() {
            return Err(FileError::NotAbsolute);
        }
        if has_parent_or_current(path) {
            return Err(FileError::OutsideWorkspace);
        }
        let mut existing = path;
        let mut rest = Vec::new();
        let base = loop {
            match std::fs::canonicalize(existing) {
                Ok(real) => break real,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    rest.push(existing.file_name().ok_or(FileError::OutsideWorkspace)?);
                    existing = existing.parent().ok_or(FileError::OutsideWorkspace)?;
                }
                Err(error) => return Err(FileError::from_io(&error)),
            }
        };
        let target = rest.iter().rev().fold(base, |path, part| path.join(part));
        let relative = target
            .strip_prefix(self.root())
            .map_err(|_| FileError::OutsideWorkspace)?;
        if relative.as_os_str().is_empty() || target.is_dir() {
            return Err(FileError::NotFound);
        }
        if protected(relative) {
            return Err(FileError::Protected);
        }
        Ok(target)
    }

    /// The text of `path` as the editor has it, from one-based `line`, at most
    /// `limit` lines.
    pub fn read(
        &self,
        path: &Path,
        line: Option<u32>,
        limit: Option<u32>,
    ) -> Result<String, FileError> {
        self.check_open()?;
        let real = self.existing(path)?;
        let key = real.to_string_lossy();
        let live = self
            .documents
            .live_snapshot(&key)
            .map_err(|_| FileError::Failed)?;
        if live.is_none()
            && std::fs::metadata(&real).map_or(0, |metadata| metadata.len()) > MAX_FILE_BYTES
        {
            return Err(FileError::TooLarge);
        }
        let snapshot = match live {
            Some(snapshot) => snapshot,
            None => self
                .documents
                .read(&key)
                .map_err(|error| FileError::from_io(&error))?,
        };
        let text = lines(snapshot.content(), line, limit);
        if text.len() > MAX_READ_BYTES {
            return Err(FileError::TooLarge);
        }
        Ok(text.to_owned())
    }

    /// Writes `content` to `path`, creating it and its folders when needed.
    /// Returns the real path written, or `None` when nothing changed.
    pub fn write(&self, path: &Path, content: &str) -> Result<Option<PathBuf>, FileError> {
        self.check_open()?;
        if content.len() > MAX_READ_BYTES {
            return Err(FileError::TooLarge);
        }
        let target = self.target(path)?;
        let before = match std::fs::metadata(&target) {
            Ok(metadata) if metadata.len() > MAX_FILE_BYTES => return Err(FileError::TooLarge),
            Ok(_) => {
                Some(std::fs::read_to_string(&target).map_err(|error| FileError::from_io(&error))?)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(FileError::from_io(&error)),
        };
        let key = target.to_string_lossy().into_owned();
        let live = self
            .documents
            .live_snapshot(&key)
            .map_err(|_| FileError::Failed)?;
        if live
            .as_ref()
            .is_some_and(|snapshot| Some(snapshot.content()) != before.as_deref())
        {
            return Err(FileError::UnsavedChanges);
        }
        if before.as_deref() == Some(content) {
            return Ok(None);
        }
        self.journal
            .record(&target, before.as_deref(), &mut || {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&target, content)
            })
            .map_err(|error| FileError::from_io(&error))?;
        // The editor's copy matched the old text; it reloads the new one.
        if let Some(before) = &before {
            let _ = self.documents.saved(&key, before);
        }
        Ok(Some(target))
    }
}

/// From one-based `line` (1 when absent), at most `limit` lines, keeping
/// line endings.
fn lines(text: &str, line: Option<u32>, limit: Option<u32>) -> &str {
    let skip = line.unwrap_or(1).saturating_sub(1) as usize;
    if skip == 0 && limit.is_none() {
        return text;
    }
    let mut starts = text
        .split_inclusive('\n')
        .scan(0, |offset, line| {
            let start = *offset;
            *offset += line.len();
            Some(start)
        })
        .chain(std::iter::once(text.len()));
    let Some(start) = starts.nth(skip) else {
        return "";
    };
    let end = match limit {
        Some(0) => start,
        Some(limit) => starts.nth(limit as usize - 1).unwrap_or(text.len()),
        None => text.len(),
    };
    &text[start..end.max(start)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Journal(Mutex<Vec<(PathBuf, Option<String>)>>);
    impl WriteJournal for Journal {
        fn record(
            &self,
            path: &Path,
            before: Option<&str>,
            write: &mut dyn FnMut() -> io::Result<()>,
        ) -> io::Result<()> {
            self.0
                .lock()
                .unwrap()
                .push((path.to_path_buf(), before.map(str::to_owned)));
            write()
        }
    }

    fn workspace() -> (
        tempfile::TempDir,
        Arc<DocumentService>,
        Arc<Journal>,
        WorkspaceFiles,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let documents = Arc::new(DocumentService::new(dir.path().to_path_buf()));
        let journal = Arc::new(Journal::default());
        let files = WorkspaceFiles::new(documents.clone(), journal.clone());
        (dir, documents, journal, files)
    }

    #[test]
    fn line_ranges_are_one_based_and_keep_line_endings() {
        let text = "one\ntwo\r\nthree\nfour";
        assert_eq!(lines(text, None, None), text);
        assert_eq!(lines(text, Some(2), None), "two\r\nthree\nfour");
        assert_eq!(lines(text, Some(2), Some(2)), "two\r\nthree\n");
        assert_eq!(lines(text, Some(0), Some(1)), "one\n");
        assert_eq!(lines(text, Some(4), Some(9)), "four");
        assert_eq!(lines(text, Some(5), None), "");
        assert_eq!(lines(text, Some(9), Some(1)), "");
        assert_eq!(lines(text, Some(1), Some(0)), "");
        assert_eq!(lines("a\n", Some(2), None), "");
    }

    #[test]
    fn reads_see_unsaved_editor_text_and_stay_in_the_workspace() {
        let (dir, documents, _, files) = workspace();
        let root = documents.workspace_root().to_path_buf();
        std::fs::write(root.join("main.rs"), "disk\nline two\n").unwrap();
        assert_eq!(
            files.read(&root.join("main.rs"), Some(2), None),
            Ok("line two\n".into())
        );
        documents
            .sync("main.rs", Some(1), "unsaved\nedit\n")
            .unwrap();
        assert_eq!(
            files.read(&root.join("main.rs"), Some(1), Some(1)),
            Ok("unsaved\n".into())
        );
        assert_eq!(
            files.read(Path::new("main.rs"), None, None),
            Err(FileError::NotAbsolute)
        );
        assert_eq!(
            files.read(&root.join("missing.rs"), None, None),
            Err(FileError::NotFound)
        );
        assert_eq!(files.read(&root, None, None), Err(FileError::NotFound));
        std::fs::write(root.join("binary.bin"), [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(
            files.read(&root.join("binary.bin"), None, None),
            Err(FileError::NotText)
        );
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "s").unwrap();
        assert_eq!(
            files.read(&outside.path().join("secret"), None, None),
            Err(FileError::OutsideWorkspace)
        );
        assert_eq!(
            files.read(&root.join("..").join("secret"), None, None),
            Err(FileError::NotFound)
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path().join("secret"), root.join("link")).unwrap();
            assert_eq!(
                files.read(&root.join("link"), None, None),
                Err(FileError::OutsideWorkspace)
            );
        }
        documents.retire();
        assert_eq!(
            files.read(&root.join("main.rs"), None, None),
            Err(FileError::WorkspaceChanged)
        );
        drop(dir);
    }

    #[test]
    fn writes_are_recorded_and_never_replace_unsaved_changes() {
        let (_dir, documents, journal, files) = workspace();
        let root = documents.workspace_root().to_path_buf();
        let main = root.join("main.rs");
        std::fs::write(&main, "old\n").unwrap();

        // An open, unchanged editor copy: written, and the stale copy is dropped.
        documents.sync("main.rs", Some(1), "old\n").unwrap();
        assert_eq!(files.write(&main, "new\n"), Ok(Some(main.clone())));
        assert_eq!(std::fs::read_to_string(&main).unwrap(), "new\n");
        assert!(documents.live_snapshot("main.rs").unwrap().is_none());
        assert_eq!(files.write(&main, "new\n"), Ok(None), "no change");

        // Unsaved edits that differ from disk are never overwritten.
        documents.sync("main.rs", Some(2), "mine\n").unwrap();
        assert_eq!(
            files.write(&main, "theirs\n"),
            Err(FileError::UnsavedChanges)
        );
        assert_eq!(std::fs::read_to_string(&main).unwrap(), "new\n");

        let created = root.join("src/deep/lib.rs");
        assert_eq!(
            files.write(&created, "fn x() {}\n"),
            Ok(Some(created.clone()))
        );
        assert_eq!(std::fs::read_to_string(&created).unwrap(), "fn x() {}\n");
        assert_eq!(
            *journal.0.lock().unwrap(),
            [(main.clone(), Some("old\n".into())), (created, None)]
        );

        for (path, error) in [
            (root.join(".git/config"), FileError::Protected),
            (root.join("sub/.git/HEAD"), FileError::Protected),
            (root.join(".zblade/settings.json"), FileError::Protected),
            (root.join("src/../../escape"), FileError::OutsideWorkspace),
            (PathBuf::from("relative.rs"), FileError::NotAbsolute),
            (root.join("src"), FileError::NotFound),
        ] {
            assert_eq!(files.write(&path, "x"), Err(error), "{}", path.display());
        }
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(
            files.write(&outside.path().join("new.rs"), "x"),
            Err(FileError::OutsideWorkspace)
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.path(), root.join("out")).unwrap();
            assert_eq!(
                files.write(&root.join("out/new.rs"), "x"),
                Err(FileError::OutsideWorkspace)
            );
            assert!(!outside.path().join("new.rs").exists());
        }
        assert_eq!(journal.0.lock().unwrap().len(), 2);
    }
}
