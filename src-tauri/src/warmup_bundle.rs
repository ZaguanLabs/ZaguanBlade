//! Warmup context bundle — deterministic editor/workspace context attached to
//! `POST /v1/blade/warmup` so zcoderd can prewarm provider prompt caches
//! (e.g. an Anthropic `max_tokens: 0` request) before the first real message.
//!
//! Contract (zcoderd feature request 2026-07-05; plan in
//! `docs/internal/2026-07-05-warmup-context-prefetch-plan.md`):
//! - identical editor/workspace state must serialize to identical bytes:
//!   stable field order, sorted lists, no timestamps or request ids
//! - every content-bearing item carries `sha256:<hex>` of the EXACT sent bytes
//! - editor-buffer content wins over disk when the active file is dirty
//! - secret/gitignored/binary/oversized content is omitted or truncated with
//!   explicit metadata (`content_omitted_reason` / `truncated`), never silently
//!
//! Blade only gathers; provider cache strategy (breakpoints, `cache_control`)
//! stays in zcoderd.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const MAX_ACTIVE_FILE_BYTES: usize = 120_000;
pub const MAX_REPO_GUIDANCE_BYTES: usize = 64_000;
pub const MAX_OPEN_FILES: usize = 20;
/// Open-file identity hashes are skipped above this size so a huge tab can't
/// stall warmup with a multi-hundred-MB read.
const MAX_OPEN_FILE_HASH_BYTES: u64 = 10_000_000;

/// `line`/`column` spell the same in camelCase and snake_case, so these two
/// shapes are shared between the frontend snapshot and the zcoderd payload.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Position {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

// ---------------------------------------------------------------------------
// Frontend input (camelCase: crosses the Tauri invoke boundary)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorWarmupSnapshot {
    pub active_file: Option<String>,
    pub cursor: Option<Position>,
    pub selection: Option<Range>,
    pub visible_range: Option<Range>,
    #[serde(default)]
    pub open_files: Vec<SnapshotOpenFile>,
    /// Editor-buffer content of the active file, present only when the buffer
    /// has unsaved changes (buffer wins over disk per the contract).
    pub active_buffer_content: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotOpenFile {
    pub path: String,
    #[serde(default)]
    pub is_modified: bool,
    pub tab_order: u32,
}

// ---------------------------------------------------------------------------
// Outgoing bundle (snake_case: the zcoderd warmup contract)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct WarmupContextBundle {
    pub workspace: WorkspaceInfo,
    pub editor_state: WarmupEditorState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_file_snapshot: Option<ActiveFileSnapshot>,
    pub repo_guidance: Vec<RepoGuidanceItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_state: Option<GitState>,
    pub limits: WarmupLimits,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceInfo {
    pub root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub allow_gitignored_files: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WarmupEditorState {
    /// `null` when no file is active — the contract wants the field present.
    pub active_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_position: Option<Position>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection: Option<Range>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_range: Option<Range>,
    pub open_files: Vec<OpenFileEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenFileEntry {
    pub path: String,
    /// Identity hash: buffer hash for the active dirty file, disk hash for
    /// clean files. Absent for dirty non-active files (their buffers are not
    /// shipped) and unreadable/oversized files.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    pub is_active: bool,
    pub is_modified: bool,
    pub tab_order: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveFileSnapshot {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// SHA-256 of exactly the bytes in `content` (the excerpt when truncated).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_id: Option<String>,
    pub is_modified: bool,
    pub source: SnapshotSource,
    pub truncated: bool,
    /// Bytes of the sent `content`; disk size of the file when content is
    /// omitted (0 when even that is unknown).
    pub byte_length: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_omitted_reason: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotSource {
    EditorBuffer,
    Disk,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoGuidanceItem {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
    pub truncated: bool,
    pub byte_length: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_omitted_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub modified_files: Vec<String>,
    pub staged_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WarmupLimits {
    pub max_active_file_bytes: u64,
    pub max_repo_guidance_bytes: u64,
    pub max_open_files: u32,
}

// ---------------------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------------------

/// Build the deterministic context bundle for one warmup send, or `None` when
/// the user disabled warmup context prefetch for this project (the warmup then
/// degrades to the legacy context-free ping). Does blocking filesystem and git
/// work — call from `spawn_blocking`.
pub fn build_bundle(
    workspace_root: &Path,
    snapshot: EditorWarmupSnapshot,
) -> Option<WarmupContextBundle> {
    let root = fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    let settings = crate::project_settings::load_project_settings_or_default(&root);
    if !settings.warmup_context_prefetch {
        return None;
    }
    let gitignore = crate::gitignore_filter::GitignoreFilter::new(&root);

    let active_path = snapshot
        .active_file
        .as_deref()
        .and_then(|path| resolve_in_workspace(&root, path));

    let active_entry_modified = active_path.as_ref().is_some_and(|resolved| {
        snapshot.open_files.iter().any(|file| {
            file.is_modified && resolve_in_workspace(&root, &file.path).as_ref() == Some(resolved)
        })
    }) || snapshot.active_buffer_content.is_some();

    let active_file_snapshot = active_path.as_ref().map(|path| {
        build_active_file_snapshot(
            path,
            active_entry_modified,
            snapshot.active_buffer_content.as_deref(),
            &gitignore,
            settings.allow_gitignored_files,
        )
    });

    // Non-null in editor_state even when the file falls outside the workspace:
    // the path itself is metadata, only content is policy-gated.
    let active_file_display = active_path
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| snapshot.active_file.clone());

    let open_files = build_open_files(
        &root,
        &snapshot,
        active_path.as_deref(),
        active_file_snapshot.as_ref(),
    );

    let active_relative = active_path
        .as_ref()
        .and_then(|path| path.strip_prefix(&root).ok())
        .map(|rel| rel.to_string_lossy().to_string());

    Some(WarmupContextBundle {
        workspace: WorkspaceInfo {
            root: root.to_string_lossy().to_string(),
            project_id: crate::project::get_existing_project_id(&root),
            allow_gitignored_files: settings.allow_gitignored_files,
        },
        editor_state: WarmupEditorState {
            active_file: active_file_display,
            cursor_position: snapshot.cursor,
            selection: snapshot.selection,
            visible_range: snapshot.visible_range,
            open_files,
        },
        active_file_snapshot,
        repo_guidance: build_repo_guidance(&root, active_relative.as_deref()),
        git_state: build_git_state(&root),
        limits: WarmupLimits {
            max_active_file_bytes: MAX_ACTIVE_FILE_BYTES as u64,
            max_repo_guidance_bytes: MAX_REPO_GUIDANCE_BYTES as u64,
            max_open_files: MAX_OPEN_FILES as u32,
        },
    })
}

fn build_active_file_snapshot(
    path: &Path,
    is_modified: bool,
    buffer: Option<&str>,
    gitignore: &crate::gitignore_filter::GitignoreFilter,
    allow_gitignored: bool,
) -> ActiveFileSnapshot {
    let path_str = path.to_string_lossy().to_string();
    let language_id = Some(crate::file_lang::detect_language(&path.to_path_buf()));
    let disk_len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);

    let omitted = |reason: &str, source: SnapshotSource| ActiveFileSnapshot {
        path: path_str.clone(),
        content: None,
        hash: None,
        language_id: language_id.clone(),
        is_modified,
        source,
        truncated: false,
        byte_length: disk_len,
        content_omitted_reason: Some(reason.to_string()),
    };

    if is_sensitive_for_warmup(&path_str) {
        return omitted("sensitive_file", SnapshotSource::Disk);
    }
    if !allow_gitignored && gitignore.should_ignore(path) {
        return omitted("gitignored", SnapshotSource::Disk);
    }

    let (owned, source) = match buffer {
        Some(buffer) => (buffer.to_string(), SnapshotSource::EditorBuffer),
        None => {
            // A dirty buffer we were not given must not be impersonated by
            // stale disk content.
            if is_modified {
                return omitted("modified_buffer_unavailable", SnapshotSource::EditorBuffer);
            }
            match fs::read(path) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) if !text.contains('\0') => (text, SnapshotSource::Disk),
                    _ => return omitted("binary", SnapshotSource::Disk),
                },
                Err(_) => return omitted("unreadable", SnapshotSource::Disk),
            }
        }
    };

    let (sent, truncated) = truncate_at_char_boundary(&owned, MAX_ACTIVE_FILE_BYTES);
    ActiveFileSnapshot {
        path: path_str,
        hash: Some(sha256_hex(sent.as_bytes())),
        byte_length: sent.len() as u64,
        content: Some(sent.to_string()),
        language_id,
        is_modified,
        source,
        truncated,
        content_omitted_reason: None,
    }
}

fn build_open_files(
    root: &Path,
    snapshot: &EditorWarmupSnapshot,
    active_path: Option<&Path>,
    active_snapshot: Option<&ActiveFileSnapshot>,
) -> Vec<OpenFileEntry> {
    let mut entries: Vec<OpenFileEntry> = snapshot
        .open_files
        .iter()
        .map(|file| {
            let resolved = resolve_in_workspace(root, &file.path);
            let is_active = resolved.as_deref().is_some_and(|r| Some(r) == active_path);
            let path = resolved
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| file.path.clone());

            let hash = if is_active {
                // Same identity the snapshot carries (buffer hash when dirty) —
                // but only when the snapshot content survived the policy gates,
                // so a full-file hash never rides along with truncated content.
                active_snapshot.and_then(|s| (!s.truncated).then(|| s.hash.clone()).flatten())
            } else if !file.is_modified {
                resolved.as_deref().and_then(disk_identity_hash)
            } else {
                None // dirty non-active buffer: content not available here
            };

            OpenFileEntry {
                path,
                hash,
                is_active,
                is_modified: file.is_modified,
                tab_order: file.tab_order,
            }
        })
        .collect();

    entries.sort_by(|a, b| a.tab_order.cmp(&b.tab_order).then(a.path.cmp(&b.path)));
    entries.truncate(MAX_OPEN_FILES);
    entries
}

fn build_repo_guidance(root: &Path, active_relative: Option<&str>) -> Vec<RepoGuidanceItem> {
    let candidates: Vec<String> = active_relative.map(str::to_string).into_iter().collect();
    let mut paths = crate::agent_instructions::discover_agent_file_paths(root, &candidates);
    paths.sort_by_key(|path| (path.components().count(), path.clone()));

    let mut budget = MAX_REPO_GUIDANCE_BYTES;
    paths
        .into_iter()
        .map(|path| {
            let path_str = path.to_string_lossy().to_string();
            let disk_len = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let omitted = |reason: &str| RepoGuidanceItem {
                path: path_str.clone(),
                content: None,
                hash: None,
                truncated: false,
                byte_length: disk_len,
                content_omitted_reason: Some(reason.to_string()),
            };

            if budget == 0 {
                return omitted("guidance_budget_exhausted");
            }
            let text = match fs::read(&path) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(text) if !text.contains('\0') => text,
                    _ => return omitted("binary"),
                },
                Err(_) => return omitted("unreadable"),
            };

            let (sent, truncated) = truncate_at_char_boundary(&text, budget);
            budget -= sent.len();
            RepoGuidanceItem {
                path: path_str,
                hash: Some(sha256_hex(sent.as_bytes())),
                byte_length: sent.len() as u64,
                content: Some(sent.to_string()),
                truncated,
                content_omitted_reason: None,
            }
        })
        .collect()
}

fn build_git_state(root: &Path) -> Option<GitState> {
    let snapshot = crate::git::collect_git_status_snapshot_cli(root.to_str()?).ok()?;
    if !snapshot.summary.is_repo {
        return None;
    }
    let collect = |predicate: fn(&crate::git::GitFileStatus) -> bool| -> Vec<String> {
        let mut files: Vec<String> = snapshot
            .files
            .iter()
            .filter(|file| predicate(file))
            .map(|file| file.path.clone())
            .collect();
        files.sort();
        files.dedup();
        files
    };
    Some(GitState {
        branch: snapshot.summary.branch.clone(),
        modified_files: collect(|file| file.unstaged || file.untracked),
        staged_files: collect(|file| file.staged),
    })
}

// ---------------------------------------------------------------------------
// Live-chat active-file identity
// ---------------------------------------------------------------------------

/// Identity of the active file for the live-chat workspace payload, derived
/// with the SAME policy and byte-exact hashing as the warmup snapshot so
/// zcoderd can compare the warmed snapshot against the live turn without
/// forcing a re-read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveFileIdentity {
    /// `sha256:<hex>` of the buffer (when dirty) or disk content. `None` when
    /// the content was omitted (secret/gitignored/binary/unreadable/dirty
    /// buffer we were not handed) or truncated — never a hash that disagrees
    /// with what warmup shipped.
    pub hash: Option<String>,
    /// The active buffer is dirty (a buffer was handed to us).
    pub is_modified: bool,
}

/// Compute the active file's identity for a live chat turn, or `None` when
/// warmup context prefetch is disabled for this project (the caller then keeps
/// the legacy hashless workspace entry) or the file resolves outside the
/// workspace. Does blocking filesystem work — call from `spawn_blocking`.
///
/// The hash is produced by the very same [`build_active_file_snapshot`] the
/// warmup bundle uses, so an unchanged file yields byte-identical hashes on
/// both paths.
pub fn active_file_identity(
    workspace_root: &Path,
    active_file: &str,
    dirty_buffer: Option<&str>,
) -> Option<ActiveFileIdentity> {
    let root = fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    let settings = crate::project_settings::load_project_settings_or_default(&root);
    if !settings.warmup_context_prefetch {
        return None;
    }
    let active_path = resolve_in_workspace(&root, active_file)?;
    let gitignore = crate::gitignore_filter::GitignoreFilter::new(&root);
    let is_modified = dirty_buffer.is_some();

    let snapshot = build_active_file_snapshot(
        &active_path,
        is_modified,
        dirty_buffer,
        &gitignore,
        settings.allow_gitignored_files,
    );

    // Mirror the warmup open-file rule: only surface a hash that agrees with
    // the untruncated snapshot content. A truncated excerpt hashes to
    // something the full-file warmup snapshot never matched, so drop it.
    let hash = if snapshot.truncated {
        None
    } else {
        snapshot.hash
    };

    Some(ActiveFileIdentity { hash, is_modified })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher
        .finalize()
        .iter()
        .fold(String::from("sha256:"), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Absolute canonical form of `path` when it stays inside `root` (which must
/// already be canonical); `None` otherwise. Falls back to lexical
/// normalization for paths that do not exist on disk (unsaved buffers).
fn resolve_in_workspace(root: &Path, path: &str) -> Option<PathBuf> {
    let joined = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };
    let resolved = fs::canonicalize(&joined).unwrap_or(lexical_normalize(&joined)?);
    resolved.starts_with(root).then_some(resolved)
}

fn lexical_normalize(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    Some(normalized)
}

/// The warmup deny-list is broader than the Symbols-Index one: `.env` variable
/// *names* are safe to index, but warmup would ship the whole file — values
/// included — so env files are suppressed here.
fn is_sensitive_for_warmup(path: &str) -> bool {
    if crate::secrets::is_secret_file(path) {
        return true;
    }
    let name = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .to_ascii_lowercase();
    name == ".env" || name.starts_with(".env.")
}

fn disk_identity_hash(path: &Path) -> Option<String> {
    let len = fs::metadata(path).ok()?.len();
    if len > MAX_OPEN_FILE_HASH_BYTES || is_sensitive_for_warmup(&path.to_string_lossy()) {
        return None;
    }
    fs::read(path).ok().map(|bytes| sha256_hex(&bytes))
}

/// Longest prefix of `s` that fits in `max_bytes` without splitting a UTF-8
/// character. Returns `(prefix, was_truncated)`.
fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> (&str, bool) {
    if s.len() <= max_bytes {
        return (s, false);
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    (&s[..end], true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    /// `build_bundle` with the default-enabled prefetch setting unwrapped.
    fn build(root: &Path, snapshot: EditorWarmupSnapshot) -> WarmupContextBundle {
        build_bundle(root, snapshot).expect("prefetch is enabled by default")
    }

    fn snapshot_for(root: &Path, file: &str) -> EditorWarmupSnapshot {
        EditorWarmupSnapshot {
            active_file: Some(root.join(file).to_string_lossy().to_string()),
            cursor: Some(Position { line: 3, column: 1 }),
            selection: None,
            visible_range: None,
            open_files: vec![SnapshotOpenFile {
                path: root.join(file).to_string_lossy().to_string(),
                is_modified: false,
                tab_order: 0,
            }],
            active_buffer_content: None,
        }
    }

    #[test]
    fn disabled_prefetch_setting_yields_no_bundle() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.rs"), "content");
        write(
            &dir.path().join(".zblade/config/settings.json"),
            r#"{"warmup_context_prefetch": false}"#,
        );

        assert!(build_bundle(dir.path(), snapshot_for(dir.path(), "a.rs")).is_none());
    }

    #[test]
    fn identical_state_serializes_identically() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("src/main.rs"), "fn main() {}\n");
        write(&dir.path().join("AGENTS.md"), "rules\n");

        let a = build(dir.path(), snapshot_for(dir.path(), "src/main.rs"));
        let b = build(dir.path(), snapshot_for(dir.path(), "src/main.rs"));

        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }

    #[test]
    fn sha256_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"hello"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn active_file_content_comes_from_disk_with_hash() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.rs"), "disk content");

        let bundle = build(dir.path(), snapshot_for(dir.path(), "a.rs"));
        let snap = bundle.active_file_snapshot.unwrap();

        assert_eq!(snap.content.as_deref(), Some("disk content"));
        assert_eq!(snap.source, SnapshotSource::Disk);
        assert_eq!(
            snap.hash.as_deref(),
            Some(sha256_hex(b"disk content").as_str())
        );
        assert_eq!(snap.byte_length, 12);
        assert!(!snap.is_modified);
        assert!(!snap.truncated);
    }

    #[test]
    fn dirty_buffer_wins_over_disk() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.rs"), "old disk content");

        let mut snapshot = snapshot_for(dir.path(), "a.rs");
        snapshot.open_files[0].is_modified = true;
        snapshot.active_buffer_content = Some("new buffer content".to_string());

        let bundle = build(dir.path(), snapshot);
        let snap = bundle.active_file_snapshot.unwrap();

        assert_eq!(snap.content.as_deref(), Some("new buffer content"));
        assert_eq!(snap.source, SnapshotSource::EditorBuffer);
        assert!(snap.is_modified);
        assert_eq!(
            snap.hash.as_deref(),
            Some(sha256_hex(b"new buffer content").as_str())
        );
        // The active open-file entry carries the same identity.
        assert_eq!(bundle.editor_state.open_files[0].hash, snap.hash);
    }

    #[test]
    fn dirty_file_without_buffer_content_is_not_impersonated_by_disk() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.rs"), "stale disk");

        let mut snapshot = snapshot_for(dir.path(), "a.rs");
        snapshot.open_files[0].is_modified = true; // dirty, but no buffer sent

        let snap = build(dir.path(), snapshot).active_file_snapshot.unwrap();

        assert!(snap.content.is_none());
        assert_eq!(
            snap.content_omitted_reason.as_deref(),
            Some("modified_buffer_unavailable")
        );
    }

    #[test]
    fn oversized_active_file_is_truncated_at_char_boundary() {
        let dir = tempfile::tempdir().unwrap();
        // Multibyte chars (é = 2 bytes) so the cap lands mid-char.
        let big = "é".repeat(MAX_ACTIVE_FILE_BYTES / 2 + 500);
        write(&dir.path().join("big.txt"), &big);

        let snap = build(dir.path(), snapshot_for(dir.path(), "big.txt"))
            .active_file_snapshot
            .unwrap();

        assert!(snap.truncated);
        let content = snap.content.unwrap();
        assert!(content.len() <= MAX_ACTIVE_FILE_BYTES);
        assert!(content.is_char_boundary(content.len()));
        // Hash covers the excerpt actually sent, not the full file.
        assert_eq!(snap.hash.unwrap(), sha256_hex(content.as_bytes()));
        assert_eq!(snap.byte_length, content.len() as u64);
    }

    #[test]
    fn secret_and_env_files_are_omitted() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["id_rsa", ".env", ".env.local", "server.pem"] {
            write(&dir.path().join(name), "SECRET=value");
            let snap = build(dir.path(), snapshot_for(dir.path(), name))
                .active_file_snapshot
                .unwrap();
            assert!(snap.content.is_none(), "content leaked for {name}");
            assert!(snap.hash.is_none(), "hash leaked for {name}");
            assert_eq!(
                snap.content_omitted_reason.as_deref(),
                Some("sensitive_file")
            );
        }
    }

    #[test]
    fn gitignored_file_is_omitted_by_default() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join(".gitignore"), "generated.rs\n");
        write(&dir.path().join("generated.rs"), "content");

        let snap = build(dir.path(), snapshot_for(dir.path(), "generated.rs"))
            .active_file_snapshot
            .unwrap();

        assert!(snap.content.is_none());
        assert_eq!(snap.content_omitted_reason.as_deref(), Some("gitignored"));
    }

    #[test]
    fn binary_file_is_omitted() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("blob.bin"), b"ab\x00cd").unwrap();

        let snap = build(dir.path(), snapshot_for(dir.path(), "blob.bin"))
            .active_file_snapshot
            .unwrap();

        assert!(snap.content.is_none());
        assert_eq!(snap.content_omitted_reason.as_deref(), Some("binary"));
    }

    #[test]
    fn file_outside_workspace_gets_no_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("out.rs"), "outside");

        let mut snapshot = snapshot_for(dir.path(), "x.rs");
        snapshot.active_file = Some(outside.path().join("out.rs").to_string_lossy().to_string());
        snapshot.open_files.clear();

        let bundle = build(dir.path(), snapshot);
        assert!(bundle.active_file_snapshot.is_none());
    }

    #[test]
    fn repo_guidance_is_per_file_hashed_and_depth_sorted() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("AGENTS.md"), "root rules");
        write(&dir.path().join("src/AGENTS.md"), "src rules");
        write(&dir.path().join("src/main.rs"), "fn main() {}");

        let bundle = build(dir.path(), snapshot_for(dir.path(), "src/main.rs"));

        assert_eq!(bundle.repo_guidance.len(), 2);
        assert!(bundle.repo_guidance[0].path.ends_with("/AGENTS.md"));
        assert!(bundle.repo_guidance[1].path.ends_with("/src/AGENTS.md"));
        assert_eq!(
            bundle.repo_guidance[0].content.as_deref(),
            Some("root rules")
        );
        assert_eq!(
            bundle.repo_guidance[0].hash.as_deref(),
            Some(sha256_hex(b"root rules").as_str())
        );
        assert!(!bundle.repo_guidance[0].truncated);
    }

    #[test]
    fn repo_guidance_respects_total_budget() {
        let dir = tempfile::tempdir().unwrap();
        write(
            &dir.path().join("AGENTS.md"),
            &"a".repeat(MAX_REPO_GUIDANCE_BYTES + 100),
        );
        write(&dir.path().join("src/AGENTS.md"), "src rules");
        write(&dir.path().join("src/main.rs"), "fn main() {}");

        let bundle = build(dir.path(), snapshot_for(dir.path(), "src/main.rs"));

        let first = &bundle.repo_guidance[0];
        assert!(first.truncated);
        assert_eq!(first.byte_length as usize, MAX_REPO_GUIDANCE_BYTES);
        let second = &bundle.repo_guidance[1];
        assert!(second.content.is_none());
        assert_eq!(
            second.content_omitted_reason.as_deref(),
            Some("guidance_budget_exhausted")
        );
    }

    #[test]
    fn open_files_sorted_by_tab_order_then_path_and_capped() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..(MAX_OPEN_FILES + 5) {
            write(&dir.path().join(format!("f{i:02}.rs")), "x");
        }

        let open_files: Vec<SnapshotOpenFile> = (0..(MAX_OPEN_FILES + 5))
            .rev() // deliberately unsorted input
            .map(|i| SnapshotOpenFile {
                path: dir
                    .path()
                    .join(format!("f{i:02}.rs"))
                    .to_string_lossy()
                    .to_string(),
                is_modified: false,
                tab_order: i as u32,
            })
            .collect();
        let snapshot = EditorWarmupSnapshot {
            active_file: None,
            cursor: None,
            selection: None,
            visible_range: None,
            open_files,
            active_buffer_content: None,
        };

        let bundle = build(dir.path(), snapshot);
        let files = &bundle.editor_state.open_files;

        assert_eq!(files.len(), MAX_OPEN_FILES);
        assert!(files.windows(2).all(|w| w[0].tab_order < w[1].tab_order));
        // Clean files carry a disk identity hash.
        assert!(files.iter().all(|f| f.hash.is_some()));
        assert!(bundle.editor_state.active_file.is_none());
    }

    #[test]
    fn dirty_non_active_open_file_has_no_hash() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.rs"), "a");
        write(&dir.path().join("b.rs"), "b");

        let mut snapshot = snapshot_for(dir.path(), "a.rs");
        snapshot.open_files.push(SnapshotOpenFile {
            path: dir.path().join("b.rs").to_string_lossy().to_string(),
            is_modified: true,
            tab_order: 1,
        });

        let bundle = build(dir.path(), snapshot);
        let files = &bundle.editor_state.open_files;

        assert!(files[0].is_active && files[0].hash.is_some());
        assert!(files[1].is_modified && files[1].hash.is_none());
    }

    #[test]
    fn active_file_identity_matches_warmup_snapshot_hash_for_clean_file() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.rs"), "disk content");

        // The identity used by the live chat path...
        let identity =
            active_file_identity(dir.path(), &dir.path().join("a.rs").to_string_lossy(), None)
                .unwrap();
        // ...must equal the hash the warmup snapshot shipped for the same file.
        let warm = build(dir.path(), snapshot_for(dir.path(), "a.rs"))
            .active_file_snapshot
            .unwrap();

        assert_eq!(identity.hash, warm.hash);
        assert_eq!(
            identity.hash.as_deref(),
            Some(sha256_hex(b"disk content").as_str())
        );
        assert!(!identity.is_modified);
    }

    #[test]
    fn active_file_identity_hashes_dirty_buffer() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.rs"), "old disk content");

        let identity = active_file_identity(
            dir.path(),
            &dir.path().join("a.rs").to_string_lossy(),
            Some("new buffer content"),
        )
        .unwrap();

        assert!(identity.is_modified);
        assert_eq!(
            identity.hash.as_deref(),
            Some(sha256_hex(b"new buffer content").as_str())
        );

        // Same source (buffer) the warmup snapshot would hash.
        let mut warm_snapshot = snapshot_for(dir.path(), "a.rs");
        warm_snapshot.open_files[0].is_modified = true;
        warm_snapshot.active_buffer_content = Some("new buffer content".to_string());
        let warm = build(dir.path(), warm_snapshot)
            .active_file_snapshot
            .unwrap();
        assert_eq!(identity.hash, warm.hash);
    }

    #[test]
    fn active_file_identity_omits_hash_for_sensitive_and_binary() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join(".env"), "SECRET=value");
        fs::write(dir.path().join("blob.bin"), b"ab\x00cd").unwrap();

        for name in [".env", "blob.bin"] {
            let identity =
                active_file_identity(dir.path(), &dir.path().join(name).to_string_lossy(), None)
                    .unwrap();
            assert!(identity.hash.is_none(), "hash leaked for {name}");
        }
    }

    #[test]
    fn active_file_identity_omits_hash_when_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let big = "a".repeat(MAX_ACTIVE_FILE_BYTES + 500);
        write(&dir.path().join("big.txt"), &big);

        let identity = active_file_identity(
            dir.path(),
            &dir.path().join("big.txt").to_string_lossy(),
            None,
        )
        .unwrap();

        // Truncated content would hash to something warmup's full-file snapshot
        // never carried (its open-file entry also drops the hash), so no hash.
        assert!(identity.hash.is_none());
    }

    #[test]
    fn active_file_identity_none_when_dirty_but_no_buffer() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.rs"), "stale disk");

        // is_modified=false (no buffer handed): clean disk hash. The dirty case
        // without a buffer only arises if the frontend fails to ship it; then
        // the buffer is None and we must not impersonate with disk content —
        // callers pass the buffer whenever dirty, so None here means clean.
        let identity =
            active_file_identity(dir.path(), &dir.path().join("a.rs").to_string_lossy(), None)
                .unwrap();
        assert_eq!(
            identity.hash.as_deref(),
            Some(sha256_hex(b"stale disk").as_str())
        );
        assert!(!identity.is_modified);
    }

    #[test]
    fn active_file_identity_none_for_file_outside_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("out.rs"), "outside");

        let identity = active_file_identity(
            dir.path(),
            &outside.path().join("out.rs").to_string_lossy(),
            None,
        );
        assert!(identity.is_none());
    }

    #[test]
    fn active_file_identity_none_when_prefetch_disabled() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.rs"), "content");
        write(
            &dir.path().join(".zblade/config/settings.json"),
            r#"{"warmup_context_prefetch": false}"#,
        );

        let identity =
            active_file_identity(dir.path(), &dir.path().join("a.rs").to_string_lossy(), None);
        assert!(identity.is_none());
    }

    #[test]
    fn truncate_at_char_boundary_never_splits_chars() {
        let s = "aé漢🎉";
        for max in 0..=s.len() {
            let (prefix, truncated) = truncate_at_char_boundary(s, max);
            assert!(prefix.len() <= max);
            assert_eq!(truncated, prefix.len() < s.len());
            assert!(s.starts_with(prefix));
        }
    }
}
