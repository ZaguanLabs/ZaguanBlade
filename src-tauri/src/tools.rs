use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Deserialize;
use walkdir::WalkDir;
use lazy_static::lazy_static;

use crate::gitignore_filter::GitignoreFilter;
use crate::project_settings;

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub success: bool,
    pub content: String,
    pub error: Option<String>,
    pub skipped: bool,
}

fn get_bool_arg(args: &HashMap<String, serde_json::Value>, keys: &[&str], default: bool) -> bool {
    for k in keys {
        if let Some(value) = args.get(*k) {
            if let Some(b) = value.as_bool() {
                return b;
            }
        }
    }
    default
}

fn get_bounded_usize_arg(
    args: &HashMap<String, serde_json::Value>,
    keys: &[&str],
    default: usize,
    cap: usize,
) -> usize {
    for k in keys {
        if let Some(value) = args.get(*k) {
            if let Some(n) = value.as_u64() {
                return (n as usize).min(cap);
            }
        }
    }
    default
}

fn get_string_array_arg(
    args: &HashMap<String, serde_json::Value>,
    keys: &[&str],
) -> Option<Vec<String>> {
    for k in keys {
        if let Some(value) = args.get(*k) {
            if let Some(items) = value.as_array() {
                let strings = items
                    .iter()
                    .filter_map(|item| item.as_str().map(ToString::to_string))
                    .collect::<Vec<String>>();
                return Some(strings);
            }
        }
    }
    None
}

fn normalize_rel_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn compile_glob_patterns(patterns: &[String]) -> Result<Vec<glob::Pattern>, String> {
    let mut compiled = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        let p = glob::Pattern::new(pattern)
            .map_err(|e| format!("invalid glob pattern '{}': {}", pattern, e))?;
        compiled.push(p);
    }
    Ok(compiled)
}

fn collect_matching_files(
    workspace_root: &Path,
    include_globs: &[String],
    exclude_globs: &[String],
) -> Result<Vec<String>, String> {
    let ws = fs::canonicalize(workspace_root)
        .map_err(|e| format!("cannot canonicalize workspace: {}", e))?;
    let includes = compile_glob_patterns(include_globs)?;
    let excludes = compile_glob_patterns(exclude_globs)?;

    let gitignore_filter = create_gitignore_filter(&ws);
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for entry in WalkDir::new(&ws)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let abs_path = entry.path();
        if let Some(ref filter) = gitignore_filter {
            if filter.should_ignore(abs_path) {
                continue;
            }
        }

        let Ok(rel_path) = abs_path.strip_prefix(&ws) else {
            continue;
        };
        let rel = normalize_rel_path(rel_path);

        if !includes.iter().any(|p| p.matches(&rel)) {
            continue;
        }
        if excludes.iter().any(|p| p.matches(&rel)) {
            continue;
        }

        if seen.insert(rel.clone()) {
            out.push(rel);
        }
    }

    out.sort();
    Ok(out)
}

fn render_with_line_numbers(content: &str) -> String {
    if content.is_empty() {
        return String::new();
    }
    content
        .lines()
        .enumerate()
        .map(|(idx, line)| format!("{}: {}", idx + 1, line))
        .collect::<Vec<String>>()
        .join("\n")
}

fn is_batch_read_only_tool(tool_name: &str) -> bool {
    match tool_name {
        "get_editor_state"
        | "read_file"
        | "read_file_range"
        | "read_many_files"
        | "grep_search"
        | "rg"
        | "codebase_search"
        | "list_dir"
        | "list_directory"
        | "get_workspace_structure"
        | "find_files"
        | "find_files_glob"
        | "glob"
        | "get_file_info"
        | "get_project_index_overview"
        | "get_project_index_chunk"
        | "codebase_investigator" => true,
        _ => false,
    }
}

/// RFC: Large Tool Result Handling - Size limits
const MAX_TOOL_RESULT_BYTES: usize = 50 * 1024; // 50KB
const MAX_TOOL_RESULT_LINES: usize = 2000;
const HEAD_LINES: usize = 100;
const TAIL_LINES: usize = 50;
const PROJECT_INDEX_OVERVIEW_DEFAULT_MAX_CHARS: usize = 6000;
const PROJECT_INDEX_OVERVIEW_MAX_CHARS: usize = 12000;
const PROJECT_INDEX_CHUNK_DEFAULT_MAX_CHARS: usize = 4000;
const PROJECT_INDEX_CHUNK_MAX_CHARS: usize = 8000;
const READ_MANY_FILES_DEFAULT_MAX_FILES: usize = 100;
const READ_MANY_FILES_MAX_FILES_CAP: usize = 500;
const READ_MANY_FILES_DEFAULT_MAX_BYTES_PER_FILE: usize = 64 * 1024;
const READ_MANY_FILES_MAX_BYTES_PER_FILE_CAP: usize = 512 * 1024;
const BATCH_DEFAULT_MAX_PARALLEL: usize = 8;
const BATCH_MAX_PARALLEL_CAP: usize = 16;
const INVESTIGATOR_DEFAULT_MAX_TURNS: usize = 8;
const INVESTIGATOR_MAX_TURNS_CAP: usize = 16;
const INVESTIGATOR_DEFAULT_MAX_TOOL_CALLS: usize = 40;
const INVESTIGATOR_MAX_TOOL_CALLS_CAP: usize = 120;

const GREP_TIMEOUT_DEFAULT_MS: u64 = 8_000;
const GREP_TIMEOUT_MIN_MS: u64 = 500;
const GREP_TIMEOUT_MAX_MS: u64 = 30_000;
const DEPENDENCY_DIRS: &[&str] = &["node_modules", "vendor"];

const TOOL_METRICS_SAMPLE_CAP: usize = 512;

#[derive(Default, Clone)]
struct ToolMetricState {
    latencies_ms: Vec<u64>,
    calls: u64,
    failures: u64,
}

#[derive(Default, Clone)]
struct GrepSearchMetricState {
    total_calls: u64,
    timeout_count: u64,
    total_duration_ms: u64,
    total_results_returned: u64,
    durations_ms: Vec<u64>,
}

lazy_static! {
    static ref TOOL_METRICS: Mutex<HashMap<String, ToolMetricState>> = Mutex::new(HashMap::new());
    static ref GREP_SEARCH_METRICS: Mutex<GrepSearchMetricState> =
        Mutex::new(GrepSearchMetricState::default());
}

fn percentile(sorted: &[u64], p: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn record_tool_metric(tool_name: &str, elapsed_ms: u64, success: bool) {
    if let Ok(mut metrics) = TOOL_METRICS.lock() {
        let entry = metrics.entry(tool_name.to_string()).or_default();
        entry.calls += 1;
        if !success {
            entry.failures += 1;
        }
        entry.latencies_ms.push(elapsed_ms);
        if entry.latencies_ms.len() > TOOL_METRICS_SAMPLE_CAP {
            let drop_count = entry.latencies_ms.len() - TOOL_METRICS_SAMPLE_CAP;
            entry.latencies_ms.drain(0..drop_count);
        }
    }
}

fn metric_snapshot(tool_name: &str) -> serde_json::Value {
    let Ok(metrics) = TOOL_METRICS.lock() else {
        return serde_json::json!({
            "tool": tool_name,
            "available": false,
        });
    };
    let Some(state) = metrics.get(tool_name) else {
        return serde_json::json!({
            "tool": tool_name,
            "available": false,
        });
    };

    let mut lats = state.latencies_ms.clone();
    lats.sort_unstable();
    let calls = state.calls.max(1);
    let failure_rate = state.failures as f64 / calls as f64;

    serde_json::json!({
        "tool": tool_name,
        "available": true,
        "calls": state.calls,
        "failures": state.failures,
        "failure_rate": failure_rate,
        "latency_ms": {
            "p50": percentile(&lats, 0.50),
            "p95": percentile(&lats, 0.95)
        }
    })
}

fn record_grep_search_metric(elapsed_ms: u64, timed_out: bool, result_count: usize) {
    if let Ok(mut metrics) = GREP_SEARCH_METRICS.lock() {
        metrics.total_calls += 1;
        if timed_out {
            metrics.timeout_count += 1;
        }
        metrics.total_duration_ms += elapsed_ms;
        metrics.total_results_returned += result_count as u64;
        metrics.durations_ms.push(elapsed_ms);
        if metrics.durations_ms.len() > TOOL_METRICS_SAMPLE_CAP {
            let drop_count = metrics.durations_ms.len() - TOOL_METRICS_SAMPLE_CAP;
            metrics.durations_ms.drain(0..drop_count);
        }
    }
}

fn grep_search_metric_snapshot() -> serde_json::Value {
    let Ok(metrics) = GREP_SEARCH_METRICS.lock() else {
        return serde_json::json!({
            "available": false
        });
    };

    if metrics.total_calls == 0 {
        return serde_json::json!({
            "grep_search.total": 0,
            "grep_search.timeout_count": 0,
            "grep_search.timeout_rate": 0.0,
            "grep_search.avg_duration_ms": 0.0,
            "grep_search.p95_duration_ms": 0,
            "grep_search.avg_results_returned": 0.0
        });
    }

    let mut lats = metrics.durations_ms.clone();
    lats.sort_unstable();

    let total = metrics.total_calls;
    let timeout_rate = metrics.timeout_count as f64 / total as f64;
    let avg_duration_ms = metrics.total_duration_ms as f64 / total as f64;
    let avg_results_returned = metrics.total_results_returned as f64 / total as f64;

    serde_json::json!({
        "grep_search.total": total,
        "grep_search.timeout_count": metrics.timeout_count,
        "grep_search.timeout_rate": timeout_rate,
        "grep_search.avg_duration_ms": avg_duration_ms,
        "grep_search.p95_duration_ms": percentile(&lats, 0.95),
        "grep_search.avg_results_returned": avg_results_returned
    })
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            success: true,
            content: content.into(),
            error: None,
            skipped: false,
        }
    }

    pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            content: String::new(),
            error: Some(error.into()),
            skipped: false,
        }
    }

    pub fn skipped(message: impl Into<String>) -> Self {
        Self {
            success: false,
            content: String::new(),
            error: Some(message.into()),
            skipped: true,
        }
    }

    pub fn to_tool_content(&self) -> String {
        if self.success {
            self.content.clone()
        } else {
            format!(
                "tool_error: {}",
                self.error
                    .clone()
                    .unwrap_or_else(|| "unknown error".to_string())
            )
        }
    }

    /// RFC: Large Tool Result Handling - Truncate content for local storage mode
    /// In local mode, zblade pre-truncates large results since zcoderd won't save to DB.
    /// Returns truncated content with head/tail and guidance message.
    pub fn to_tool_content_truncated(&self) -> String {
        let content = self.to_tool_content();
        truncate_large_content(&content)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        apply_patch_to_string, execute_tool, grep_search, parse_grep_timeout_ms, GREP_TIMEOUT_DEFAULT_MS,
        GREP_TIMEOUT_MAX_MS, GREP_TIMEOUT_MIN_MS,
    };
    use std::fs;
    use std::collections::HashMap;
    use tempfile::tempdir;

    #[test]
    fn apply_patch_rejects_ambiguous_exact_matches() {
        let content = "A\nTARGET\nB\nTARGET\nC\n";
        let err = apply_patch_to_string(content, "TARGET", "REPLACED").unwrap_err();
        assert!(err.contains("Ambiguous match"), "unexpected error: {err}");
    }

    #[test]
    fn apply_patch_replaces_single_exact_match() {
        let content = "A\nTARGET\nB\n";
        let updated = apply_patch_to_string(content, "TARGET", "REPLACED").unwrap();
        assert_eq!(updated, "A\nREPLACED\nB\n");
    }

    #[test]
    fn batch_rejects_non_read_only_tool_calls() {
        let workspace = tempdir().expect("tempdir");
        let args = r#"{
            "calls": [
                {"tool": "run_command", "arguments": {"command": "echo hi"}}
            ]
        }"#;

        let result = execute_tool(workspace.path(), "batch", args);
        assert!(result.success, "batch should return structured all-settled output");

        let payload: serde_json::Value =
            serde_json::from_str(&result.content).expect("batch json output");
        let first = payload["results"][0].clone();
        assert_eq!(first["ok"].as_bool(), Some(false));
        assert!(first["error"]
            .as_str()
            .unwrap_or_default()
            .contains("read-only allowlist"));
    }

    #[test]
    fn read_many_files_includes_metrics_in_summary() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("example.txt");
        fs::write(&file_path, "hello\nworld\n").expect("write test file");

        let result = execute_tool(
            workspace.path(),
            "read_many_files",
            r#"{"paths":["*.txt"],"max_files":10}"#,
        );
        assert!(result.success, "read_many_files should succeed");

        let payload: serde_json::Value =
            serde_json::from_str(&result.content).expect("read_many_files json output");
        let metrics = &payload["summary"]["metrics"];
        assert_eq!(metrics["tool"].as_str(), Some("read_many_files"));
        assert!(metrics["calls"].as_u64().unwrap_or(0) >= 1);
        assert!(metrics["latency_ms"]["p50"].is_number());
        assert!(metrics["latency_ms"]["p95"].is_number());
    }

    #[test]
    fn grep_timeout_clamps_min_default_max() {
        let empty = HashMap::new();
        assert_eq!(parse_grep_timeout_ms(&empty), GREP_TIMEOUT_DEFAULT_MS);

        let below_min = serde_json::from_str::<HashMap<String, serde_json::Value>>(
            r#"{"timeout_ms":100}"#,
        )
        .expect("parse args");
        assert_eq!(parse_grep_timeout_ms(&below_min), GREP_TIMEOUT_MIN_MS);

        let above_max = serde_json::from_str::<HashMap<String, serde_json::Value>>(
            r#"{"timeout_ms":999999}"#,
        )
        .expect("parse args");
        assert_eq!(parse_grep_timeout_ms(&above_max), GREP_TIMEOUT_MAX_MS);
    }

    #[test]
    fn grep_timeout_returns_structured_partial_payload() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("sample.txt");
        fs::write(&file_path, "needle one\nneedle two\nneedle three\n").expect("write test file");

        let result = grep_search(
            workspace.path(),
            &serde_json::from_str::<HashMap<String, serde_json::Value>>(
                r#"{"pattern":"needle","path":".","timeout_ms":500,"include_dependencies":false,"__test_force_timeout":true}"#,
            )
            .expect("parse args"),
            true,
        );

        assert!(result.success, "timeout path should be graceful");
        let payload: serde_json::Value =
            serde_json::from_str(&result.content).expect("timeout payload json");
        assert_eq!(payload["timed_out"].as_bool(), Some(true));
        assert_eq!(payload["timeout_ms"].as_u64(), Some(GREP_TIMEOUT_MIN_MS));
        assert!(payload["partial_results"].is_array());
        assert!(payload["result_count"].is_u64());
        assert_eq!(payload["searched_path"].as_str(), Some("."));
        assert!(payload["next_step_hint"]
            .as_str()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("narrow"));
    }

    #[test]
    fn grep_non_timeout_behavior_is_unchanged_plain_output() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("alpha.txt");
        fs::write(&file_path, "first\nneedle\nlast\n").expect("write test file");

        let result = grep_search(
            workspace.path(),
            &serde_json::from_str::<HashMap<String, serde_json::Value>>(
                r#"{"pattern":"needle","path":".","timeout_ms":8000}"#,
            )
            .expect("parse args"),
            true,
        );

        assert!(result.success);
        assert!(result.content.contains("needle"));
        assert!(!result.content.trim_start().starts_with('{'));
    }

    #[test]
    fn grep_dependency_opt_in_respected() {
        let workspace = tempdir().expect("tempdir");
        let dep_dir = workspace.path().join("node_modules").join("pkg");
        fs::create_dir_all(&dep_dir).expect("create dependency dir");
        fs::write(dep_dir.join("index.js"), "const token = 'dep-hit';\n").expect("write dep file");

        let without_opt_in = grep_search(
            workspace.path(),
            &serde_json::from_str::<HashMap<String, serde_json::Value>>(
                r#"{"pattern":"dep-hit","path":".","timeout_ms":8000,"include_dependencies":false}"#,
            )
            .expect("parse args"),
            true,
        );
        assert!(without_opt_in.success);
        assert!(!without_opt_in.content.contains("dep-hit"));

        let with_opt_in = grep_search(
            workspace.path(),
            &serde_json::from_str::<HashMap<String, serde_json::Value>>(
                r#"{"pattern":"dep-hit","path":".","timeout_ms":8000,"include_dependencies":true}"#,
            )
            .expect("parse args"),
            true,
        );
        assert!(with_opt_in.success);
        assert!(with_opt_in.content.contains("dep-hit"));
    }
}

/// Truncate large content per RFC-LARGE-TOOL-RESULTS.md
/// Shows first 100 lines + last 50 lines with truncation message
pub fn truncate_large_content(content: &str) -> String {
    let bytes = content.len();
    let lines: Vec<&str> = content.lines().collect();
    let line_count = lines.len();

    // Check if truncation is needed (either limit exceeded triggers truncation)
    if bytes <= MAX_TOOL_RESULT_BYTES && line_count <= MAX_TOOL_RESULT_LINES {
        return content.to_string();
    }

    // Handle edge case: if content has fewer lines than HEAD + TAIL, just return as-is
    // (this shouldn't happen if we exceeded limits, but be defensive)
    if line_count <= HEAD_LINES + TAIL_LINES {
        return content.to_string();
    }

    // Build truncated output with head + tail
    let head: String = lines[..HEAD_LINES].join("\n");
    let tail: String = lines[line_count - TAIL_LINES..].join("\n");

    format!(
        "{}\n\n[TRUNCATED: {} bytes, {} lines - showing first {} and last {} lines]\nResult was too large. Use more specific tool parameters to get targeted results.\n\n{}",
        head,
        bytes,
        line_count,
        HEAD_LINES,
        TAIL_LINES,
        tail
    )
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Args {
    Map(HashMap<String, serde_json::Value>),
    Null,
}

fn get_str_arg(args: &HashMap<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = args.get(*k).and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

/// Load project settings and create a GitignoreFilter if needed
/// Returns None if gitignore filtering should not be applied
fn create_gitignore_filter(workspace_root: &Path) -> Option<GitignoreFilter> {
    let settings = project_settings::load_project_settings_or_default(workspace_root);
    
    // If allow_gitignored_files is true, don't create a filter (allow all files)
    if settings.allow_gitignored_files {
        return None;
    }
    
    // Create filter to respect .gitignore
    Some(GitignoreFilter::new(workspace_root))
}

// Editor state for IDE-specific tools
#[derive(Clone)]
pub struct EditorState {
    pub active_file: Option<String>,
    pub open_files: Vec<String>,
    pub active_tab_index: usize,
    pub cursor_line: Option<usize>,
    pub cursor_column: Option<usize>,
    pub selection_start_line: Option<usize>,
    pub selection_end_line: Option<usize>,
}

pub fn execute_tool(workspace_root: &Path, tool_name: &str, raw_args: &str) -> ToolResult {
    execute_tool_with_editor::<tauri::Wry>(workspace_root, tool_name, raw_args, None, None)
}

pub fn execute_tool_with_editor<R: tauri::Runtime>(
    workspace_root: &Path,
    tool_name: &str,
    raw_args: &str,
    editor_state: Option<&EditorState>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    // Claude models sometimes prefix arguments with {} - strip it
    // But don't strip if the entire string is just "{}"
    let sanitized_args = if raw_args.starts_with("{}") && raw_args.len() > 2 {
        &raw_args[2..]
    } else {
        raw_args
    };

    eprintln!(
        "[TOOL PARSE] tool={}, raw_args='{}', sanitized_args='{}'",
        tool_name, raw_args, sanitized_args
    );

    let args: HashMap<String, serde_json::Value> =
        match serde_json::from_str::<Args>(sanitized_args) {
            Ok(Args::Map(m)) => m,
            Ok(Args::Null) => HashMap::new(),
            Err(e) => {
                eprintln!("[TOOL PARSE ERROR] Failed to parse args: {}", e);
                return ToolResult::err(format!("invalid tool args json: {e}"));
            }
        };

    let grep_timeout_enforced = app_handle
        .map(|handle| {
            use tauri::Manager;
            handle
                .state::<crate::app_state::AppState>()
                .feature_flags
                .grep_timeout_enforced()
        })
        .unwrap_or(false);

    match tool_name {
        // Legacy tools (kept for compatibility)
        "read_file" => read_file(workspace_root, &args),
        "write_file" | "create_file" => write_file(workspace_root, &args),
        "edit_file" => edit_file(workspace_root, &args),
        "grep_search" | "rg" => grep_search(workspace_root, &args, grep_timeout_enforced),
        "codebase_search" => codebase_search(workspace_root, &args),
        "list_directory" | "list_dir" => list_directory(workspace_root, &args),

        // Phase 1 IDE-specific tools
        "get_editor_state" => get_editor_state(editor_state),
        "read_file_range" => read_file_range(workspace_root, &args),
        "apply_edit" | "apply_patch" => apply_edit_tool(workspace_root, &args),
        "get_workspace_structure" => get_workspace_structure(workspace_root, &args),
        "get_project_index_overview" => get_project_index_overview(workspace_root, &args),
        "get_project_index_chunk" => get_project_index_chunk(workspace_root, &args),
        "read_many_files" => read_many_files(workspace_root, &args),
        "batch" => batch(workspace_root, &args, editor_state),
        "codebase_investigator" => codebase_investigator(workspace_root, &args),


        // New file system tools
        "find_files" => find_files(workspace_root, &args),
        "find_files_glob" | "glob" => find_files_glob(workspace_root, &args),
        "create_directory" => create_directory(workspace_root, &args),
        "delete_file" => delete_file(workspace_root, &args),
        "move_file" => move_file(workspace_root, &args),
        "copy_file" => copy_file(workspace_root, &args),
        "get_file_info" => get_file_info(workspace_root, &args),

        // New editor interaction tools
        "open_file" => open_file(&args),
        "goto_line" => goto_line(&args),
        "get_selection" => get_selection(editor_state),
        "replace_selection" => replace_selection(&args),
        "insert_at_cursor" => insert_at_cursor(&args),

        // Server-side tools (handled by zcoderd, not zblade)
        "ask_followup_question" | "attempt_completion" | "new_task" | "generate_image" | "todo_write" => {
            ToolResult::err(format!(
                "Tool '{}' is a server-side tool that should be handled by zcoderd, not zblade. \
                This error indicates a protocol issue - zblade should not receive execution requests for server-side tools.",
                tool_name
            ))
        }

        _ => ToolResult::err(format!("unknown tool: {tool_name}")),
    }
}

/// Resolve a path (potentially relative) to an absolute path under the workspace.
/// This handles edge cases like ".", "./src", "src/utils" by prepending workspace root.
/// Does NOT require the path to exist (useful for write operations).
fn resolve_path_in_workspace(workspace_root: &Path, path: &Path) -> Result<PathBuf, String> {
    let ws = fs::canonicalize(workspace_root).map_err(|e| format!("cannot canonicalize workspace: {}", e))?;

    // Handle relative paths by joining with workspace root
    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        ws.join(path)
    };

    // Normalize the path by resolving . and .. components without requiring existence
    let normalized = normalize_path(&candidate);

    // Validate the normalized path is under workspace
    if !normalized.starts_with(&ws) {
        return Err(format!(
            "path is outside workspace (workspace: {}, resolved: {})",
            ws.display(),
            normalized.display()
        ));
    }

    Ok(normalized)
}

/// Normalize a path by resolving . and .. components without requiring the path to exist.
/// This is similar to canonicalize but works for non-existent paths.
fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;
    
    let mut normalized = PathBuf::new();
    
    for component in path.components() {
        match component {
            Component::Prefix(p) => normalized.push(p.as_os_str()),
            Component::RootDir => normalized.push("/"),
            Component::CurDir => {} // Skip "."
            Component::ParentDir => {
                // Go up one level if possible
                normalized.pop();
            }
            Component::Normal(c) => normalized.push(c),
        }
    }
    
    normalized
}

/// Validate and resolve a path under workspace. Requires the path to exist.
/// Use resolve_path_in_workspace for paths that may not exist yet.
fn validate_path_under_workspace(workspace_root: &Path, path: &Path) -> Result<PathBuf, String> {
    let ws = fs::canonicalize(workspace_root).map_err(|e| e.to_string())?;

    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        ws.join(path)
    };

    // First check if the path exists (without following symlinks)
    if !candidate.exists() {
        return Err(format!("path does not exist: {}", candidate.display()));
    }

    // For symlinks, we want to validate the link location, not the target
    // This allows symlinks inside the workspace even if they point outside
    let normalized = normalize_path(&candidate);
    
    // Validate the normalized path is under workspace
    if !normalized.starts_with(&ws) {
        return Err(format!(
            "path is outside workspace (workspace: {}, path: {})",
            ws.display(),
            normalized.display()
        ));
    }

    // Return the normalized path (not canonicalized) to preserve symlinks
    Ok(normalized)
}

fn read_file(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path", "file_path", "filepath", "filename"]) else {
        return ToolResult::err("missing required arg: path (or file_path)");
    };

    let abs = match validate_path_under_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    match fs::read_to_string(&abs) {
        Ok(s) => {
            let content = if s.is_empty() {
                format!(
                    "=== File: {} (empty) ===\n// This file exists but contains no content.",
                    abs.to_string_lossy()
                )
            } else {
                format!("=== File: {} ===\n{}", abs.to_string_lossy(), s)
            };
            ToolResult::ok(content)
        }
        Err(e) => ToolResult::err(e.to_string()),
    }
}

fn read_many_files(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let mut include_globs =
        get_string_array_arg(args, &["paths", "globs", "patterns"]).unwrap_or_default();
    if include_globs.is_empty() {
        if let Some(single) = get_str_arg(args, &["path", "pattern", "glob"]) {
            include_globs.push(single);
        }
    }
    if include_globs.is_empty() {
        return ToolResult::err("read_many_files requires 'paths' (array of glob patterns)");
    }

    let exclude_globs = get_string_array_arg(args, &["exclude", "excludes"]).unwrap_or_default();
    let max_files = get_bounded_usize_arg(
        args,
        &["max_files"],
        READ_MANY_FILES_DEFAULT_MAX_FILES,
        READ_MANY_FILES_MAX_FILES_CAP,
    );
    let max_bytes_per_file = get_bounded_usize_arg(
        args,
        &["max_bytes_per_file"],
        READ_MANY_FILES_DEFAULT_MAX_BYTES_PER_FILE,
        READ_MANY_FILES_MAX_BYTES_PER_FILE_CAP,
    );
    let include_line_numbers = get_bool_arg(args, &["include_line_numbers"], true);

    let started_at = Instant::now();
    let matched_files = match collect_matching_files(workspace_root, &include_globs, &exclude_globs) {
        Ok(files) => files,
        Err(e) => return ToolResult::err(e),
    };

    let matched_count = matched_files.len();
    let selected_files: Vec<String> = matched_files.into_iter().take(max_files).collect();
    let skipped = matched_count.saturating_sub(selected_files.len());

    if selected_files.is_empty() {
        return ToolResult::err("read_many_files matched zero files after filters");
    }

    let ws = match fs::canonicalize(workspace_root) {
        Ok(ws) => ws,
        Err(e) => return ToolResult::err(format!("cannot canonicalize workspace: {}", e)),
    };

    let queue = Arc::new(Mutex::new(
        selected_files
            .into_iter()
            .enumerate()
            .collect::<VecDeque<(usize, String)>>(),
    ));
    let results = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let worker_count = BATCH_DEFAULT_MAX_PARALLEL
        .min(BATCH_MAX_PARALLEL_CAP)
        .max(1)
        .min(queue.lock().ok().map(|q| q.len()).unwrap_or(1).max(1));

    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let results = Arc::clone(&results);
        let ws = ws.clone();

        handles.push(std::thread::spawn(move || {
            loop {
                let next = {
                    let mut guard = match queue.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    guard.pop_front()
                };

                let Some((index, rel_path)) = next else {
                    break;
                };

                let abs_path = ws.join(&rel_path);
                let file_json = match fs::read(&abs_path) {
                    Ok(bytes) => {
                        let original_byte_count = bytes.len();
                        let truncated = original_byte_count > max_bytes_per_file;
                        let selected_bytes = if truncated {
                            &bytes[..max_bytes_per_file]
                        } else {
                            &bytes[..]
                        };

                        let mut content = String::from_utf8_lossy(selected_bytes).to_string();
                        let line_count = if content.is_empty() {
                            0
                        } else {
                            content.lines().count()
                        };
                        if include_line_numbers {
                            content = render_with_line_numbers(&content);
                        }

                        serde_json::json!({
                            "index": index,
                            "path": rel_path,
                            "truncated": truncated,
                            "content": content,
                            "line_count": line_count,
                            "byte_count": selected_bytes.len(),
                            "original_byte_count": original_byte_count,
                        })
                    }
                    Err(e) => serde_json::json!({
                        "index": index,
                        "path": rel_path,
                        "truncated": false,
                        "error": e.to_string(),
                    }),
                };

                if let Ok(mut guard) = results.lock() {
                    guard.push(file_json);
                }
            }
        }));
    }

    for handle in handles {
        if let Err(e) = handle.join() {
            eprintln!("[read_many_files] worker thread panic: {:?}", e);
        }
    }

    let mut files = match results.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return ToolResult::err("read_many_files failed to aggregate results"),
    };
    files.sort_by_key(|v| v.get("index").and_then(|i| i.as_u64()).unwrap_or(u64::MAX));

    let successful_count = files.iter().filter(|f| f.get("error").is_none()).count();
    let failed_count = files.len().saturating_sub(successful_count);
    let truncated_files = files
        .iter()
        .filter(|f| f.get("truncated").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();

    if successful_count == 0 {
        return ToolResult::err(format!(
            "read_many_files could not return any readable files (matched={}, attempted={}, failed={})",
            matched_count,
            files.len(),
            failed_count
        ));
    }

    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let result = serde_json::json!({
        "files": files
            .into_iter()
            .map(|mut f| {
                if let Some(obj) = f.as_object_mut() {
                    obj.remove("index");
                }
                f
            })
            .collect::<Vec<serde_json::Value>>(),
        "summary": {
            "matched": matched_count,
            "returned": successful_count,
            "truncated_files": truncated_files,
            "skipped": skipped,
            "failed": failed_count,
            "elapsed_ms": elapsed_ms,
        }
    });

    eprintln!(
        "[TOOLS][read_many_files] matched={} returned={} failed={} truncated={} elapsed_ms={}",
        matched_count, successful_count, failed_count, truncated_files, elapsed_ms
    );

    record_tool_metric("read_many_files", elapsed_ms, successful_count > 0);

    let mut result_with_metrics = result;
    if let Some(summary) = result_with_metrics.get_mut("summary").and_then(|v| v.as_object_mut()) {
        summary.insert("metrics".to_string(), metric_snapshot("read_many_files"));
    }

    ToolResult::ok(serde_json::to_string_pretty(&result_with_metrics).unwrap_or_default())
}

fn batch(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    editor_state: Option<&EditorState>,
) -> ToolResult {
    let Some(calls_value) = args.get("calls") else {
        return ToolResult::err("batch requires 'calls' array");
    };
    let Some(calls_array) = calls_value.as_array() else {
        return ToolResult::err("batch 'calls' must be an array");
    };

    let max_parallel = get_bounded_usize_arg(
        args,
        &["max_parallel"],
        BATCH_DEFAULT_MAX_PARALLEL,
        BATCH_MAX_PARALLEL_CAP,
    )
    .max(1);
    let fail_fast = get_bool_arg(args, &["fail_fast"], false);
    let ordered = get_bool_arg(args, &["ordered"], true);
    let cancel_after_ms = args.get("cancel_after_ms").and_then(|v| v.as_u64());
    let started_at = Instant::now();

    let mut immediate_results = Vec::<serde_json::Value>::new();
    let mut queued = VecDeque::<(usize, String, String)>::new();

    for (index, call) in calls_array.iter().enumerate() {
        let Some(obj) = call.as_object() else {
            immediate_results.push(serde_json::json!({
                "index": index,
                "tool": "unknown",
                "ok": false,
                "error": "call must be an object",
                "elapsed_ms": 0
            }));
            continue;
        };

        let tool_name = obj
            .get("tool")
            .or_else(|| obj.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        if !is_batch_read_only_tool(&tool_name)
            || matches!(tool_name.as_str(), "batch" | "run_command")
        {
            immediate_results.push(serde_json::json!({
                "index": index,
                "tool": tool_name,
                "ok": false,
                "error": "tool is not allowed in batch (read-only allowlist enforced)",
                "elapsed_ms": 0
            }));
            continue;
        }

        let arguments = obj
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        if !arguments.is_object() && !arguments.is_null() {
            immediate_results.push(serde_json::json!({
                "index": index,
                "tool": tool_name,
                "ok": false,
                "error": "arguments must be a JSON object",
                "elapsed_ms": 0
            }));
            continue;
        }

        let args_json = match serde_json::to_string(&arguments) {
            Ok(s) => s,
            Err(e) => {
                immediate_results.push(serde_json::json!({
                    "index": index,
                    "tool": tool_name,
                    "ok": false,
                    "error": format!("failed to serialize arguments: {}", e),
                    "elapsed_ms": 0
                }));
                continue;
            }
        };

        queued.push_back((index, tool_name, args_json));
    }

    let ws = match fs::canonicalize(workspace_root) {
        Ok(ws) => ws,
        Err(e) => return ToolResult::err(format!("cannot canonicalize workspace: {}", e)),
    };

    let queue = Arc::new(Mutex::new(queued));
    let shared_results = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let editor_state_owned = editor_state.cloned();
    let worker_count = max_parallel
        .min(BATCH_MAX_PARALLEL_CAP)
        .min(queue.lock().ok().map(|q| q.len()).unwrap_or(0).max(1));

    let mut handles = Vec::new();
    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let results = Arc::clone(&shared_results);
        let stop = Arc::clone(&stop);
        let ws = ws.clone();
        let editor_state_owned = editor_state_owned.clone();
        let tool_started_at = started_at;

        handles.push(std::thread::spawn(move || {
            loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                if let Some(limit) = cancel_after_ms {
                    if tool_started_at.elapsed().as_millis() as u64 >= limit {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                }

                let next = {
                    let mut guard = match queue.lock() {
                        Ok(g) => g,
                        Err(_) => return,
                    };
                    guard.pop_front()
                };

                let Some((index, tool_name, args_json)) = next else {
                    break;
                };

                let tool_started = Instant::now();
                let result = execute_tool_with_editor::<tauri::Wry>(
                    &ws,
                    &tool_name,
                    &args_json,
                    editor_state_owned.as_ref(),
                    None,
                );
                let elapsed_ms = tool_started.elapsed().as_millis() as u64;

                let entry = if result.success {
                    serde_json::json!({
                        "index": index,
                        "tool": tool_name,
                        "ok": true,
                        "output": result.to_tool_content_truncated(),
                        "elapsed_ms": elapsed_ms,
                    })
                } else {
                    serde_json::json!({
                        "index": index,
                        "tool": tool_name,
                        "ok": false,
                        "error": result.error.unwrap_or_else(|| "unknown error".to_string()),
                        "skipped": result.skipped,
                        "elapsed_ms": elapsed_ms,
                    })
                };

                if !entry.get("ok").and_then(|v| v.as_bool()).unwrap_or(false) && fail_fast {
                    stop.store(true, Ordering::Relaxed);
                }

                if let Ok(mut guard) = results.lock() {
                    guard.push(entry);
                }
            }
        }));
    }

    for handle in handles {
        if let Err(e) = handle.join() {
            eprintln!("[batch] worker thread panic: {:?}", e);
        }
    }

    let mut results = immediate_results;
    if let Ok(guard) = shared_results.lock() {
        results.extend(guard.iter().cloned());
    }

    if ordered {
        results.sort_by_key(|value| value.get("index").and_then(|v| v.as_u64()).unwrap_or(u64::MAX));
    }

    let succeeded = results
        .iter()
        .filter(|value| value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();
    let total = calls_array.len();
    let failed = total.saturating_sub(succeeded);
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let cancelled = stop.load(Ordering::Relaxed)
        && cancel_after_ms.map(|limit| elapsed_ms >= limit).unwrap_or(false);

    let output = serde_json::json!({
        "results": results,
        "meta": {
            "total": total,
            "succeeded": succeeded,
            "failed": failed,
            "elapsed_ms": elapsed_ms,
            "cancelled": cancelled,
            "max_parallel": max_parallel,
            "fail_fast": fail_fast,
            "ordered": ordered,
        }
    });

    eprintln!(
        "[TOOLS][batch] total={} succeeded={} failed={} elapsed_ms={} cancelled={}",
        total, succeeded, failed, elapsed_ms, cancelled
    );

    record_tool_metric("batch", elapsed_ms, failed == 0);

    let mut output_with_metrics = output;
    if let Some(meta) = output_with_metrics.get_mut("meta").and_then(|v| v.as_object_mut()) {
        meta.insert("metrics".to_string(), metric_snapshot("batch"));
    }

    ToolResult::ok(serde_json::to_string_pretty(&output_with_metrics).unwrap_or_default())
}

fn extract_objective_keywords(objective: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "where", "what", "that", "from", "into", "when",
        "how", "find", "show", "this", "these", "those", "code", "repo", "project",
    ];

    let mut keywords = objective
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter_map(|part| {
            let token = part.trim().to_lowercase();
            if token.len() < 4 || STOPWORDS.contains(&token.as_str()) {
                None
            } else {
                Some(token)
            }
        })
        .collect::<Vec<String>>();

    keywords.sort();
    keywords.dedup();
    keywords
}

fn codebase_investigator(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(objective) = get_str_arg(args, &["objective"]) else {
        return ToolResult::err("codebase_investigator requires 'objective'");
    };

    let scope =
        get_string_array_arg(args, &["scope"]).unwrap_or_else(|| vec!["**/*".to_string()]);
    let max_turns = get_bounded_usize_arg(
        args,
        &["max_turns"],
        INVESTIGATOR_DEFAULT_MAX_TURNS,
        INVESTIGATOR_MAX_TURNS_CAP,
    );
    let max_tool_calls = get_bounded_usize_arg(
        args,
        &["max_tool_calls"],
        INVESTIGATOR_DEFAULT_MAX_TOOL_CALLS,
        INVESTIGATOR_MAX_TOOL_CALLS_CAP,
    );
    let output_format =
        get_str_arg(args, &["output_format"]).unwrap_or_else(|| "json".to_string());
    let cancel_after_ms = args.get("cancel_after_ms").and_then(|v| v.as_u64());

    let started_at = Instant::now();
    let keywords = extract_objective_keywords(&objective);
    let files = match collect_matching_files(workspace_root, &scope, &Vec::new()) {
        Ok(files) => files,
        Err(e) => return ToolResult::err(e),
    };

    let ws = match fs::canonicalize(workspace_root) {
        Ok(ws) => ws,
        Err(e) => return ToolResult::err(format!("cannot canonicalize workspace: {}", e)),
    };

    let mut findings = Vec::<serde_json::Value>::new();
    let mut tool_calls_used = 0usize;
    let mut turns_used = 0usize;

    for rel_path in files.iter() {
        if tool_calls_used >= max_tool_calls || turns_used >= max_turns {
            break;
        }
        if let Some(limit) = cancel_after_ms {
            if started_at.elapsed().as_millis() as u64 >= limit {
                break;
            }
        }

        let abs = ws.join(rel_path);
        let Ok(content) = fs::read_to_string(&abs) else {
            tool_calls_used += 1;
            continue;
        };
        tool_calls_used += 1;
        turns_used = tool_calls_used.div_ceil(5);

        let lower = content.to_lowercase();
        let matched_keywords = keywords
            .iter()
            .filter(|kw| lower.contains(kw.as_str()))
            .cloned()
            .collect::<Vec<String>>();

        if matched_keywords.is_empty() {
            continue;
        }

        let mut evidence = Vec::<String>::new();
        for (line_idx, line) in content.lines().enumerate() {
            let line_lower = line.to_lowercase();
            if matched_keywords
                .iter()
                .any(|keyword| line_lower.contains(keyword.as_str()))
            {
                evidence.push(format!("{}:{}", rel_path, line_idx + 1));
            }
            if evidence.len() >= 4 {
                break;
            }
        }

        findings.push(serde_json::json!({
            "claim": format!(
                "{} appears relevant to objective based on keyword overlap ({})",
                rel_path,
                matched_keywords.join(", ")
            ),
            "evidence": evidence,
        }));

        if findings.len() >= 12 {
            break;
        }
    }

    let mut gaps = Vec::<String>::new();
    if findings.is_empty() {
        gaps.push("No direct matches found within the bounded scope/budget".to_string());
    }
    if tool_calls_used >= max_tool_calls {
        gaps.push("Investigation stopped due to max_tool_calls budget".to_string());
    }
    if turns_used >= max_turns {
        gaps.push("Investigation stopped due to max_turns budget".to_string());
    }

    let recommended_changes = vec![
        "Promote repeated policy checks to a centralized policy map/table".to_string(),
        "Expose budget and tool policy as explicit config surfaced to clients".to_string(),
    ];
    let confidence = if findings.len() >= 5 {
        "high"
    } else if findings.len() >= 2 {
        "medium"
    } else {
        "low"
    };

    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let report = serde_json::json!({
        "findings": findings,
        "gaps": gaps,
        "recommended_changes": recommended_changes,
        "confidence": confidence,
        "meta": {
            "objective": objective,
            "scope": scope,
            "max_turns": max_turns,
            "max_tool_calls": max_tool_calls,
            "turns_used": turns_used,
            "tool_calls_used": tool_calls_used,
            "elapsed_ms": elapsed_ms,
            "keywords": keywords,
            "read_only_tools": ["read_file", "read_file_range", "grep_search", "list_dir", "glob"],
        }
    });

    eprintln!(
        "[TOOLS][codebase_investigator] findings={} tool_calls_used={} turns_used={} elapsed_ms={}",
        report["findings"].as_array().map(|a| a.len()).unwrap_or(0),
        tool_calls_used,
        turns_used,
        elapsed_ms
    );

    record_tool_metric("codebase_investigator", elapsed_ms, !findings.is_empty());

    let mut report_with_metrics = report;
    if let Some(meta) = report_with_metrics.get_mut("meta").and_then(|v| v.as_object_mut()) {
        meta.insert(
            "metrics".to_string(),
            metric_snapshot("codebase_investigator"),
        );
    }

    if output_format.eq_ignore_ascii_case("markdown") {
        let mut markdown = String::new();
        markdown.push_str("# Codebase Investigator Report\n\n");
        markdown.push_str(&format!("Objective: {}\n\n", objective));
        markdown.push_str("## Findings\n");
        if let Some(findings) = report_with_metrics["findings"].as_array() {
            for finding in findings {
                let claim = finding["claim"].as_str().unwrap_or("Unknown claim");
                markdown.push_str(&format!("- {}\n", claim));
                if let Some(evidence) = finding["evidence"].as_array() {
                    for ev in evidence {
                        if let Some(ev) = ev.as_str() {
                            markdown.push_str(&format!("  - `{}`\n", ev));
                        }
                    }
                }
            }
        }
        markdown.push_str(&format!("\nConfidence: {}\n", confidence));
        return ToolResult::ok(markdown);
    }

    ToolResult::ok(serde_json::to_string_pretty(&report_with_metrics).unwrap_or_default())
}

fn get_project_index_overview(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
) -> ToolResult {
    let root = match resolve_index_root(workspace_root, args) {
        Ok(root) => root,
        Err(e) => return ToolResult::err(e),
    };

    let max_chars = parse_bounded_usize_arg(
        args,
        "max_chars",
        PROJECT_INDEX_OVERVIEW_DEFAULT_MAX_CHARS,
        PROJECT_INDEX_OVERVIEW_MAX_CHARS,
    );
    let offset = parse_bounded_usize_arg(args, "offset", 0, usize::MAX);

    build_project_index_window_result("get_project_index_overview", &root, offset, max_chars)
}

fn get_project_index_chunk(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
) -> ToolResult {
    let root = match resolve_index_root(workspace_root, args) {
        Ok(root) => root,
        Err(e) => return ToolResult::err(e),
    };

    let max_chars = parse_bounded_usize_arg(
        args,
        "max_chars",
        PROJECT_INDEX_CHUNK_DEFAULT_MAX_CHARS,
        PROJECT_INDEX_CHUNK_MAX_CHARS,
    );
    let offset = parse_bounded_usize_arg(args, "offset", 0, usize::MAX);

    build_project_index_window_result("get_project_index_chunk", &root, offset, max_chars)
}

fn build_project_index_window_result(
    tool_name: &str,
    root: &Path,
    offset: usize,
    max_chars: usize,
) -> ToolResult {
    let index_path = root.join(".zblade/context/project_index.md");
    if !index_path.exists() {
        return ToolResult::err(format!(
            "project index missing: {}. Run workspace indexing to generate .zblade/context/project_index.md",
            index_path.display()
        ));
    }

    let content = match fs::read_to_string(&index_path) {
        Ok(content) => content,
        Err(e) => {
            return ToolResult::err(format!(
                "failed to read project index {}: {}",
                index_path.display(),
                e
            ));
        }
    };

    let (window, end, total_chars, has_more) = slice_by_char_window(&content, offset, max_chars);
    let returned_chars = window.chars().count();

    let result = serde_json::json!({
        "tool": tool_name,
        "workspace_root": root.display().to_string(),
        "index_path": index_path.display().to_string(),
        "total_chars": total_chars,
        "offset": offset,
        "end": end,
        "returned_chars": returned_chars,
        "max_chars": max_chars,
        "has_more": has_more,
        "next_offset": if has_more { Some(end) } else { None },
        "content": window,
    });

    ToolResult::ok(serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn resolve_index_root(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
) -> Result<PathBuf, String> {
    let path = get_str_arg(args, &["path"]).unwrap_or_else(|| ".".to_string());
    let root = resolve_path_in_workspace(workspace_root, Path::new(&path))?;

    if !root.exists() {
        return Err(format!(
            "project root does not exist: {}",
            root.display()
        ));
    }
    if !root.is_dir() {
        return Err(format!(
            "project root must be a directory: {}",
            root.display()
        ));
    }

    Ok(root)
}

fn parse_bounded_usize_arg(
    args: &HashMap<String, serde_json::Value>,
    key: &str,
    default: usize,
    max_allowed: usize,
) -> usize {
    args.get(key)
        .and_then(|v| v.as_u64())
        .map(|v| (v as usize).min(max_allowed))
        .unwrap_or(default)
}

fn slice_by_char_window(content: &str, offset: usize, max_chars: usize) -> (String, usize, usize, bool) {
    let total_chars = content.chars().count();
    if offset >= total_chars {
        return (String::new(), total_chars, total_chars, false);
    }

    let window: String = content.chars().skip(offset).take(max_chars).collect();
    let end = offset + window.chars().count();
    let has_more = end < total_chars;

    (window, end, total_chars, has_more)
}

fn write_file(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path", "file_path", "filepath", "filename"]) else {
        return ToolResult::err("missing required arg: path (or file_path)");
    };
    let Some(content) = get_str_arg(args, &["content", "contents", "text", "data"]) else {
        return ToolResult::err("missing required arg: content (or contents/text)");
    };

    // Use resolve_path_in_workspace which handles relative paths and doesn't require existence
    let abs = match resolve_path_in_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    // Create parent directories if needed
    if let Some(parent) = abs.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return ToolResult::err(format!("cannot create parent directory: {}", e));
        }
    }

    match fs::write(&abs, content.as_bytes()) {
        Ok(()) => ToolResult::ok(format!("wrote {} bytes to {}", content.len(), abs.display())),
        Err(e) => ToolResult::err(format!("write failed: {}", e)),
    }
}

fn edit_file(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path", "file_path", "filepath", "filename"]) else {
        return ToolResult::err("missing required arg: path (or file_path)");
    };
    let Some(old_content) = get_str_arg(args, &["old_content", "old", "from"]) else {
        return ToolResult::err("missing required arg: old_content (or old/from)");
    };
    let Some(new_content) = get_str_arg(args, &["new_content", "new", "to"]) else {
        return ToolResult::err("missing required arg: new_content (or new/to)");
    };

    let abs = match validate_path_under_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    let content = match fs::read_to_string(&abs) {
        Ok(s) => s,
        Err(e) => return ToolResult::err(e.to_string()),
    };

    let Some(pos) = content.find(&old_content) else {
        return ToolResult::err("old_content not found".to_string());
    };

    let mut out = String::with_capacity(content.len() - old_content.len() + new_content.len());
    out.push_str(&content[..pos]);
    out.push_str(&new_content);
    out.push_str(&content[pos + old_content.len()..]);

    match fs::write(&abs, out.as_bytes()) {
        Ok(()) => ToolResult::ok("edit applied".to_string()),
        Err(e) => ToolResult::err(e.to_string()),
    }
}

fn list_directory(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    // Forward to get_workspace_structure with default depth 1 to provide a consistant tree view
    // which coding models (like Mercury/Qwen) prefer over flat lists.
    let mut new_args = args.clone();

    // Default to current dir if no path provided (handles empty args case)
    if !new_args.contains_key("path")
        && !new_args.contains_key("dir")
        && !new_args.contains_key("directory")
    {
        new_args.insert(
            "path".to_string(),
            serde_json::Value::String(".".to_string()),
        );
    }

    if !new_args.contains_key("max_depth") {
        new_args.insert("max_depth".to_string(), serde_json::Value::Number(1.into()));
    }
    get_workspace_structure(workspace_root, &new_args)
}

fn parse_grep_timeout_ms(args: &HashMap<String, serde_json::Value>) -> u64 {
    let timeout_ms = args
        .get("timeout_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(GREP_TIMEOUT_DEFAULT_MS);
    timeout_ms.clamp(GREP_TIMEOUT_MIN_MS, GREP_TIMEOUT_MAX_MS)
}

fn is_dependency_path(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        DEPENDENCY_DIRS.iter().any(|dir| value == *dir)
    })
}

fn build_grep_next_step_hint(include_dependencies: bool) -> String {
    if include_dependencies {
        "Search timed out. Narrow your path to a focused subtree (for example src/internal or node_modules/<pkg>) and retry with a targeted pattern.".to_string()
    } else {
        "Search timed out. Narrow your path to src/internal or, for dependency code, retry with include_dependencies=true and a targeted dependency path like node_modules/<pkg>.".to_string()
    }
}

fn grep_search(workspace_root: &Path, args: &HashMap<String, serde_json::Value>, enforce_timeout: bool) -> ToolResult {
    let Some(pattern) = get_str_arg(args, &["pattern", "query", "regex"]) else {
        return ToolResult::err(
            "grep_search requires a 'pattern' argument. Example: {\"pattern\": \"Priority\"}",
        );
    };
    let path = get_str_arg(args, &["path", "dir", "directory"]).unwrap_or_else(|| ".".to_string());
    let include_dependencies = get_bool_arg(args, &["include_dependencies"], false);
    let timeout_ms = if enforce_timeout {
        Some(parse_grep_timeout_ms(args))
    } else {
        None
    };

    let abs = match validate_path_under_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    let re = match Regex::new(&pattern) {
        Ok(r) => r,
        Err(e) => return ToolResult::err(format!("invalid regex: {e}")),
    };

    // Load gitignore filter
    let gitignore_filter = create_gitignore_filter(workspace_root);

    let started_at = Instant::now();
    #[cfg(test)]
    let force_timeout = get_bool_arg(args, &["__test_force_timeout"], false);
    #[cfg(not(test))]
    let force_timeout = false;

    let mut timed_out = false;
    let mut out = String::new();
    let mut partial_results: Vec<String> = Vec::new();
    let deadline = if force_timeout {
        Some(started_at)
    } else {
        timeout_ms.map(|ms| started_at + Duration::from_millis(ms))
    };

    let mut result_count = 0usize;

    'file_loop: for entry in WalkDir::new(abs)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if let Some(limit) = deadline {
            if Instant::now() >= limit {
                timed_out = true;
                break 'file_loop;
            }
        }

        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();

        if !include_dependencies && is_dependency_path(path) {
            continue;
        }

        // Check gitignore filter
        if let Some(ref filter) = gitignore_filter {
            if filter.should_ignore(path) {
                continue;
            }
        }

        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };

        for (idx, line) in text.lines().enumerate() {
            if let Some(limit) = deadline {
                if Instant::now() >= limit {
                    timed_out = true;
                    break 'file_loop;
                }
            }

            if re.is_match(line) {
                let hit = format!(
                    "{}:{}:{}",
                    path.to_string_lossy(),
                    idx + 1,
                    line
                );
                out.push_str(&hit);
                out.push('\n');
                partial_results.push(hit);
                result_count += 1;
            }
        }
    }

    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    record_grep_search_metric(elapsed_ms, timed_out, result_count);

    if timed_out {
        let effective_timeout_ms = timeout_ms.unwrap_or(GREP_TIMEOUT_DEFAULT_MS);
        let payload = serde_json::json!({
            "timed_out": true,
            "timeout_ms": effective_timeout_ms,
            "partial_results": partial_results,
            "result_count": result_count,
            "searched_path": path,
            "next_step_hint": build_grep_next_step_hint(include_dependencies),
            "metrics": grep_search_metric_snapshot(),
        });

        eprintln!(
            "[TOOLS][grep_search] timed_out=true timeout_ms={} include_dependencies={} result_count={} elapsed_ms={} telemetry={}",
            effective_timeout_ms,
            include_dependencies,
            result_count,
            elapsed_ms,
            grep_search_metric_snapshot(),
        );

        return ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default());
    }

    eprintln!(
        "[TOOLS][grep_search] timed_out=false include_dependencies={} result_count={} elapsed_ms={} telemetry={}",
        include_dependencies,
        result_count,
        elapsed_ms,
        grep_search_metric_snapshot(),
    );

    ToolResult::ok(out)
}

fn codebase_search(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(query) = get_str_arg(args, &["query"]) else {
        return ToolResult::err(
            "codebase_search requires a 'query' argument. Example: {\"query\": \"struct User\"}",
        );
    };

    let file_pattern = get_str_arg(args, &["file_pattern"]);
    let max_results = args
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;

    let abs = match fs::canonicalize(workspace_root) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(format!("cannot canonicalize workspace: {}", e)),
    };

    // Compile regex pattern
    let re = match Regex::new(&query) {
        Ok(r) => r,
        Err(e) => return ToolResult::err(format!("invalid regex pattern: {}", e)),
    };

    // Load gitignore filter
    let gitignore_filter = create_gitignore_filter(workspace_root);

    let mut results = Vec::new();
    let mut count = 0;

    for entry in WalkDir::new(&abs)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.path();
        
        // Check gitignore filter
        if let Some(ref filter) = gitignore_filter {
            if filter.should_ignore(path) {
                continue;
            }
        }

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Apply file pattern filter if specified
        if let Some(ref pattern) = file_pattern {
            let patterns: Vec<&str> = pattern.split(',').collect();
            let matches_pattern = patterns.iter().any(|p| {
                let p = p.trim();
                if p.starts_with("*.") {
                    file_name.ends_with(&p[1..])
                } else if p.starts_with("*") {
                    file_name.ends_with(&p[1..])
                } else {
                    file_name == p
                }
            });

            if !matches_pattern {
                continue;
            }
        }

        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };

        let lines: Vec<&str> = text.lines().collect();

        for (idx, line) in lines.iter().enumerate() {
            if re.is_match(line) {
                if count >= max_results {
                    break;
                }

                // Get context lines (2 before, 2 after)
                let start = idx.saturating_sub(2);
                let end = (idx + 3).min(lines.len());

                let context_lines: Vec<String> = lines[start..end]
                    .iter()
                    .enumerate()
                    .map(|(i, l)| {
                        let line_num = start + i + 1;
                        let marker = if start + i == idx { ">>>" } else { "   " };
                        format!("{} {}: {}", marker, line_num, l)
                    })
                    .collect();

                results.push(format!(
                    "\n{}:{}:\n{}\n",
                    path.strip_prefix(&abs).unwrap_or(path).to_string_lossy(),
                    idx + 1,
                    context_lines.join("\n")
                ));

                count += 1;
            }
        }

        if count >= max_results {
            break;
        }
    }

    if results.is_empty() {
        return ToolResult::ok(format!("No matches found for query: '{}'", query));
    }

    let output = format!(
        "Found {} matches for '{}' (showing up to {}):\n{}",
        count,
        query,
        max_results,
        results.join("\n")
    );

    ToolResult::ok(output)
}

// ===== Phase 1 IDE-Specific Tools =====

fn get_editor_state(editor_state: Option<&EditorState>) -> ToolResult {
    let Some(state) = editor_state else {
        return ToolResult::err("editor state not available");
    };

    let json = serde_json::json!({
        "active_file": state.active_file,
        "open_files": state.open_files,
        "active_tab_index": state.active_tab_index,
        "cursor_line": state.cursor_line,
        "cursor_column": state.cursor_column,
        "selection_start_line": state.selection_start_line,
        "selection_end_line": state.selection_end_line,
    });

    let mut result = serde_json::to_string_pretty(&json).unwrap_or_default();

    // Add helpful context for Claude
    if let Some(ref active_file) = state.active_file {
        result.push_str(&format!("\n\n// The active file is: {}", active_file));

        if let Some(line) = state.cursor_line {
            result.push_str(&format!("\n// Cursor is at line {}", line));
            if let Some(col) = state.cursor_column {
                result.push_str(&format!(", column {}", col));
            }
        }

        if let (Some(start), Some(end)) = (state.selection_start_line, state.selection_end_line) {
            if start != end {
                result.push_str(&format!(
                    "\n// Text is selected from line {} to line {}",
                    start, end
                ));
            }
        }

        result.push_str(&format!(
            "\n// Use read_file with path '{}' to get the file contents.",
            active_file
        ));

        if let Some(line) = state.cursor_line {
            result.push_str(&format!(
                "\n// To get context around the cursor, use read_file_range with:"
            ));
            result.push_str(&format!("\n//   path: '{}'", active_file));
            result.push_str(&format!(
                "\n//   start_line: {}",
                line.saturating_sub(5).max(1)
            ));
            result.push_str(&format!("\n//   end_line: {}", line + 5));
            result.push_str(&format!("\n//   context_lines: 3 (optional)"));
        }
    }

    ToolResult::ok(result)
}

fn read_file_range(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path", "file_path", "filepath", "filename"]) else {
        return ToolResult::err("missing required arg: path (or file_path)");
    };

    let abs = match validate_path_under_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    let content = match fs::read_to_string(&abs) {
        Ok(s) => s,
        Err(e) => return ToolResult::err(e.to_string()),
    };

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Parse line range (1-indexed)
    let start_line = args.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let end_line = args
        .get("end_line")
        .and_then(|v| v.as_u64())
        .unwrap_or(total_lines as u64) as usize;
    let context_lines = args
        .get("context_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    // Adjust for 1-indexed and apply context
    // Clamp final bounds to avoid panics when requested range exceeds file length.
    let requested_start = start_line.saturating_sub(1).saturating_sub(context_lines);
    let requested_end = end_line
        .saturating_add(context_lines)
        .min(total_lines);

    let start = requested_start.min(total_lines);
    let end = requested_end.max(start).min(total_lines);

    let selected_lines: Vec<String> = lines[start..end]
        .iter()
        .enumerate()
        .map(|(idx, line)| format!("{}: {}", start + idx + 1, line))
        .collect();

    let result = format!(
        "File: {}\nLines {}-{} (of {}):\n{}\n",
        path,
        start + 1,
        end,
        total_lines,
        selected_lines.join("\n")
    );

    ToolResult::ok(result)
}

// Helper for applying patches with robust matching
pub fn apply_patch_to_string(
    content: &str,
    old_text: &str,
    new_text: &str,
) -> Result<String, String> {
    // Strategy 1: Exact Match
    // Guard against repeated boilerplate blocks (e.g. changelog sections). If old_text
    // appears multiple times, replacing the first match silently can corrupt unrelated
    // sections and produce misleading diffs.
    if !old_text.is_empty() {
        let mut exact_matches = content.match_indices(old_text);
        if let Some((pos, _)) = exact_matches.next() {
            if exact_matches.next().is_some() {
                return Err(
                    "Ambiguous match: old_text appears multiple times (exact match). Please provide more unique context."
                        .to_string(),
                );
            }

            let mut out = String::with_capacity(content.len() - old_text.len() + new_text.len());
            out.push_str(&content[..pos]);
            out.push_str(new_text);
            out.push_str(&content[pos + old_text.len()..]);
            return Ok(out);
        }
    } else if let Some(pos) = content.find(old_text) {
        // Preserve legacy behavior for explicit empty old_text usage.
        let mut out = String::with_capacity(content.len() - old_text.len() + new_text.len());
        out.push_str(&content[..pos]);
        out.push_str(new_text);
        out.push_str(&content[pos + old_text.len()..]);
        return Ok(out);
    }

    // Strategy 2: Line-by-Line Fuzzy Match (ignoring whitespace differences)
    let content_lines: Vec<&str> = content.lines().collect();
    let old_lines: Vec<&str> = old_text.lines().collect();

    // Normalize lines for comparison (trim whitespace)
    let norm_content_lines: Vec<String> =
        content_lines.iter().map(|l| l.trim().to_string()).collect();
    let norm_old_lines: Vec<String> = old_lines.iter().map(|l| l.trim().to_string()).collect();

    // If old_text is empty or just whitespace, we can't fuzzy match safely
    if norm_old_lines.is_empty() || (norm_old_lines.len() == 1 && norm_old_lines[0].is_empty()) {
        return Err("old_text not found (exact match failed, fuzzy match skipped for empty/whitespace input)".to_string());
    }

    // Find all potential matches
    let mut matches = Vec::new();
    if content_lines.len() >= old_lines.len() {
        for i in 0..=(content_lines.len() - old_lines.len()) {
            if norm_content_lines[i..i + old_lines.len()] == norm_old_lines[..] {
                matches.push(i);
            }
        }
    }

    if matches.len() == 1 {
        let start_line_idx = matches[0];
        let end_line_idx = start_line_idx + old_lines.len();

        // Detect indentation from the first matched line in the original file
        let original_indent = content_lines[start_line_idx]
            .chars()
            .take_while(|c| c.is_whitespace())
            .collect::<String>();

        // Check if the first line of new_text needs indentation
        // If new_text has less indentation than original, we might need to fix it
        let new_lines: Vec<&str> = new_text.lines().collect();
        let new_text_indent = if !new_lines.is_empty() {
            new_lines[0]
                .chars()
                .take_while(|c| c.is_whitespace())
                .collect::<String>()
        } else {
            String::new()
        };

        let should_fix_indent = !original_indent.is_empty()
            && new_text_indent.len() < original_indent.len()
            && !new_text.trim().is_empty();

        // Reconstruct the file content
        // 1. Everything before the match
        let mut out = String::new();
        for i in 0..start_line_idx {
            out.push_str(content_lines[i]);
            out.push('\n');
        }

        // 2. The NEW text (replacing the matched block) with optional indentation fix
        if should_fix_indent {
            for (i, line) in new_lines.iter().enumerate() {
                if !line.trim().is_empty() {
                    out.push_str(&original_indent);
                }
                out.push_str(line);
                if i < new_lines.len() - 1 {
                    out.push('\n');
                }
            }
            if new_text.ends_with('\n') {
                out.push('\n');
            }
        } else {
            out.push_str(new_text);
        }

        // 3. Everything after the match
        if end_line_idx < content_lines.len() {
            // Ensure newline before appending rest if new_text didn't end with one
            if !out.ends_with('\n') && !new_text.is_empty() {
                out.push('\n');
            }

            for i in end_line_idx..content_lines.len() {
                out.push_str(content_lines[i]);
                if i < content_lines.len() - 1 {
                    out.push('\n');
                }
            }

            // Preserve trailing newline from original if it existed
            if content.ends_with('\n') && !out.ends_with('\n') {
                out.push('\n');
            }
        } else if content.ends_with('\n') && !out.ends_with('\n') {
            out.push('\n');
        }

        Ok(out)
    } else if matches.len() > 1 {
        Err(format!(
            "Ambiguous match: found {} occurrences of old_text (ignoring whitespace). Please provide more unique context.",
            matches.len()
        ))
    } else {
        Err(format!(
            "old_text not found in file (searched {} chars). Exact match failed. Fuzzy match failed.",
            old_text.len()
        ))
    }
}

/// Represents a single patch hunk for multi-patch operations
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PatchHunk {
    old_text: String,
    new_text: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

/// Result of applying multiple patches atomically
#[derive(Debug)]
#[allow(dead_code)]
struct MultiPatchResult {
    success: bool,
    applied_count: usize,
    total_count: usize,
    error: Option<String>,
    failed_index: Option<usize>,
}

/// Apply multiple patches atomically to a file content string.
/// All patches are validated before any are applied.
/// If any patch fails validation, the operation is aborted and no changes are made.
fn apply_multi_patch_to_string(content: &str, patches: &[PatchHunk]) -> Result<String, String> {
    if patches.is_empty() {
        return Err("No patches provided".to_string());
    }

    // Phase 1: Validate ALL patches before applying any
    // This ensures atomicity - we either apply all or none
    let mut validation_errors = Vec::new();

    for (idx, patch) in patches.iter().enumerate() {
        // Count occurrences of old_text
        let count = content.matches(&patch.old_text).count();

        if count == 0 {
            // Try fuzzy match to give better error message
            let norm_old: Vec<String> = patch
                .old_text
                .lines()
                .map(|l| l.trim().to_string())
                .collect();
            let content_lines: Vec<&str> = content.lines().collect();
            let norm_content: Vec<String> =
                content_lines.iter().map(|l| l.trim().to_string()).collect();

            let mut fuzzy_count = 0;
            if !norm_old.is_empty() && content_lines.len() >= norm_old.len() {
                for i in 0..=(content_lines.len() - norm_old.len()) {
                    if norm_content[i..i + norm_old.len()] == norm_old[..] {
                        fuzzy_count += 1;
                    }
                }
            }

            if fuzzy_count == 1 {
                // Will succeed with fuzzy matching - continue
            } else if fuzzy_count > 1 {
                validation_errors.push(format!(
                    "Patch {}: old_text matches {} times (fuzzy). Add start_line hint or more context.",
                    idx + 1, fuzzy_count
                ));
            } else {
                validation_errors.push(format!("Patch {}: old_text not found in file", idx + 1));
            }
        } else if count > 1 {
            // TODO: Use start_line/end_line hints to disambiguate
            validation_errors.push(format!(
                "Patch {}: old_text matches {} times. Add start_line hint or more context.",
                idx + 1,
                count
            ));
        }
        // count == 1 is perfect, no error
    }

    if !validation_errors.is_empty() {
        return Err(format!(
            "Multi-patch validation failed (no changes made):\n{}",
            validation_errors.join("\n")
        ));
    }

    // Phase 2: Apply patches sequentially
    // Since we validated all patches, we apply them in order
    let mut working = content.to_string();

    for (idx, patch) in patches.iter().enumerate() {
        match apply_patch_to_string(&working, &patch.old_text, &patch.new_text) {
            Ok(new_content) => {
                working = new_content;
            }
            Err(e) => {
                // This shouldn't happen since we validated, but handle gracefully
                return Err(format!(
                    "Patch {} failed unexpectedly after validation: {}",
                    idx + 1,
                    e
                ));
            }
        }
    }

    Ok(working)
}

fn apply_edit_tool(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path", "file_path", "filepath", "filename"]) else {
        return ToolResult::err("missing required arg: path (or file_path)");
    };

    let abs = match validate_path_under_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    let content = match fs::read_to_string(&abs) {
        Ok(s) => s,
        Err(e) => return ToolResult::err(e.to_string()),
    };

    // Check for new multi-patch format first
    if let Some(patches_value) = args.get("patches") {
        if let Some(patches_array) = patches_value.as_array() {
            // Parse patches array
            let mut patches = Vec::new();

            for (idx, patch_value) in patches_array.iter().enumerate() {
                let Some(patch_obj) = patch_value.as_object() else {
                    return ToolResult::err(format!("Patch {} is not an object", idx + 1));
                };

                let Some(old_text) = patch_obj.get("old_text").and_then(|v| v.as_str()) else {
                    return ToolResult::err(format!("Patch {} missing old_text", idx + 1));
                };

                let Some(new_text) = patch_obj.get("new_text").and_then(|v| v.as_str()) else {
                    return ToolResult::err(format!("Patch {} missing new_text", idx + 1));
                };

                let start_line = patch_obj
                    .get("start_line")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);
                let end_line = patch_obj
                    .get("end_line")
                    .and_then(|v| v.as_u64())
                    .map(|n| n as usize);

                patches.push(PatchHunk {
                    old_text: old_text.to_string(),
                    new_text: new_text.to_string(),
                    start_line,
                    end_line,
                });
            }

            if patches.is_empty() {
                return ToolResult::err("patches array is empty");
            }

            // Apply multi-patch atomically
            match apply_multi_patch_to_string(&content, &patches) {
                Ok(new_content) => match fs::write(&abs, new_content.as_bytes()) {
                    Ok(()) => {
                        let count = patches.len();
                        ToolResult::ok(format!(
                            "Applied {} patch{} atomically to {}",
                            count,
                            if count == 1 { "" } else { "es" },
                            path
                        ))
                    }
                    Err(e) => ToolResult::err(format!("Failed to write file: {}", e)),
                },
                Err(e) => ToolResult::err(e),
            }
        } else {
            ToolResult::err("patches must be an array")
        }
    } else {
        // Legacy single-patch format
        let Some(old_text) = get_str_arg(args, &["old_text", "old_content", "old", "from"]) else {
            return ToolResult::err(
                "missing required arg: old_text (or old_content/old/from) or patches array",
            );
        };
        let Some(new_text) = get_str_arg(args, &["new_text", "new_content", "new", "to"]) else {
            return ToolResult::err("missing required arg: new_text (or new_content/new/to)");
        };

        match apply_patch_to_string(&content, &old_text, &new_text) {
            Ok(new_content) => match fs::write(&abs, new_content.as_bytes()) {
                Ok(()) => ToolResult::ok(format!("Applied edit to {}", path)),
                Err(e) => ToolResult::err(e.to_string()),
            },
            Err(e) => {
                // Provide helpful debugging info
                let _preview_len = 200.min(content.len());
                let _old_preview = if old_text.len() > 100 {
                    format!("{}... ({} chars)", &old_text[..100], old_text.len())
                } else {
                    old_text.clone()
                };

                ToolResult::err(e)
            }
        }
    }
}

/// Default limit for directory entries (inspired by Codex's 25, but slightly higher)
const DEFAULT_LIST_LIMIT: usize = 50;
/// Maximum limit to prevent abuse
const MAX_LIST_LIMIT: usize = 200;
/// Default depth for directory traversal
const DEFAULT_LIST_DEPTH: usize = 2;
/// Indentation spaces per depth level (like Codex)
const INDENT_SPACES: usize = 2;

/// Directories to always ignore regardless of gitignore settings
/// (inspired by opencode, cline, roo-code)
const DIRS_TO_ALWAYS_IGNORE: &[&str] = &[
    "node_modules",
    "__pycache__",
    ".git",
    "target",
    "dist",
    "build",
    ".next",
    ".nuxt",
    "vendor",
    ".venv",
    "venv",
    "env",
    ".cargo",
    ".rustup",
    "tmp",
    "temp",
    ".cache",
    "cache",
    "coverage",
    ".coverage",
    "logs",
    "Pods",
    ".idea",
    ".vscode",
    "obj",
    "bin",
    ".zig-cache",
    "zig-out",
];

fn get_workspace_structure(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
) -> ToolResult {
    let path = get_str_arg(args, &["path", "dir", "directory"]).unwrap_or_else(|| ".".to_string());
    let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_LIST_DEPTH as u64) as usize;
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(DEFAULT_LIST_LIMIT as u64) as usize;
    let limit = limit.min(MAX_LIST_LIMIT); // Cap at maximum

    let abs = match validate_path_under_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    // Load gitignore filter if enabled in project settings
    let gitignore_filter = create_gitignore_filter(workspace_root);

    // Collect entries with BFS traversal (like Codex)
    let mut entries: Vec<ListEntry> = Vec::new();
    collect_dir_entries(
        &abs,
        &abs,
        depth,
        gitignore_filter.as_ref(),
        &mut entries,
    );

    // Sort entries by path for consistent output
    entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    // Apply limit
    let truncated = entries.len() > limit;
    let entries: Vec<_> = entries.into_iter().take(limit).collect();

    // Format output (clean indented style like Codex)
    let mut output = format!("Directory: {}\n", abs.to_string_lossy());
    for entry in &entries {
        let indent = " ".repeat(entry.depth * INDENT_SPACES);
        let suffix = if entry.is_dir { "/" } else { "" };
        output.push_str(&format!("{}{}{}\n", indent, entry.name, suffix));
    }

    if truncated {
        output.push_str(&format!("\n(showing {} of more entries, use a more specific path or increase limit)\n", limit));
    }

    ToolResult::ok(output)
}

#[derive(Debug)]
struct ListEntry {
    name: String,
    rel_path: String,
    depth: usize,
    is_dir: bool,
}

fn collect_dir_entries(
    base_path: &Path,
    current_path: &Path,
    max_depth: usize,
    gitignore_filter: Option<&GitignoreFilter>,
    entries: &mut Vec<ListEntry>,
) {
    let rel_to_base = current_path.strip_prefix(base_path).unwrap_or(Path::new(""));
    let current_depth = rel_to_base.components().count();

    if current_depth >= max_depth {
        return;
    }

    let Ok(read_dir) = fs::read_dir(current_path) else {
        return;
    };

    let mut items: Vec<_> = read_dir.filter_map(Result::ok).collect();
    items.sort_by_key(|e| e.file_name());

    for entry in items {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files/dirs
        if name.starts_with('.') {
            continue;
        }

        // Always skip certain directories regardless of gitignore
        if DIRS_TO_ALWAYS_IGNORE.contains(&name.as_str()) {
            continue;
        }

        let entry_path = entry.path();

        // Check gitignore filter
        if let Some(filter) = gitignore_filter {
            if filter.should_ignore(&entry_path) {
                continue;
            }
        }

        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let rel_path = entry_path
            .strip_prefix(base_path)
            .unwrap_or(&entry_path)
            .to_string_lossy()
            .to_string();

        entries.push(ListEntry {
            name: name.clone(),
            rel_path: rel_path.clone(),
            depth: current_depth,
            is_dir,
        });

        // Recurse into directories
        if is_dir {
            collect_dir_entries(base_path, &entry_path, max_depth, gitignore_filter, entries);
        }
    }
}


fn find_files(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(pattern) = get_str_arg(args, &["pattern"]) else {
        return ToolResult::err("missing required arg: pattern");
    };

    let search_path = get_str_arg(args, &["path"])
        .map(|p| workspace_root.join(p))
        .unwrap_or_else(|| workspace_root.to_path_buf());

    let max_depth = args
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .map(|d| d as usize);

    let mut results = Vec::new();
    let walker = if let Some(depth) = max_depth {
        WalkDir::new(&search_path).max_depth(depth)
    } else {
        WalkDir::new(&search_path)
    };

    for entry in walker.into_iter().filter_map(|e| e.ok()) {
        if let Some(name) = entry.file_name().to_str() {
            if name.contains(pattern.as_str()) {
                if let Ok(rel_path) = entry.path().strip_prefix(workspace_root) {
                    results.push(rel_path.display().to_string());
                }
            }
        }
    }

    ToolResult::ok(results.join("\n"))
}

fn find_files_glob(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(pattern) = get_str_arg(args, &["pattern", "glob"]) else {
        return ToolResult::err("missing required arg: pattern");
    };

    // Optional base path within workspace
    let search_base = get_str_arg(args, &["path"])
        .map(|p| workspace_root.join(p))
        .unwrap_or_else(|| workspace_root.to_path_buf());

    // Resolve base path
    let abs_base = match fs::canonicalize(&search_base) {
        Ok(p) => p,
        Err(_) => search_base,
    };

    // Safest way:
    // If pattern starts with /, assume it's relative to workspace root (ignore leading /)
    let clean_pattern = pattern.trim_start_matches('/');

    // Combine base and pattern
    let full_pattern = abs_base.join(clean_pattern);
    let pattern_str = full_pattern.to_string_lossy();

    let case_sensitive = args
        .get("case_sensitive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let mut matches = Vec::new();
    let mut count = 0;
    const MAX_RESULTS: usize = 200;

    let options = glob::MatchOptions {
        case_sensitive: case_sensitive,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };

    match glob::glob_with(&pattern_str, options) {
        Ok(paths) => {
            for entry in paths {
                match entry {
                    Ok(path) => {
                        if path.is_file() {
                            let rel = path
                                .strip_prefix(workspace_root)
                                .unwrap_or(&path)
                                .to_string_lossy()
                                .to_string();
                            matches.push(rel);
                            count += 1;
                        }
                    }
                    Err(e) => eprintln!("Glob error: {:?}", e),
                }
                if count >= MAX_RESULTS {
                    break;
                }
            }
        }
        Err(e) => return ToolResult::err(format!("Invalid glob pattern: {}", e)),
    }

    if matches.is_empty() {
        return ToolResult::ok("No matching files found.");
    }

    let mut output = matches.join("\n");
    if count >= MAX_RESULTS {
        output.push_str(&format!("\n... (truncated after {} results)", MAX_RESULTS));
    }

    ToolResult::ok(output)
}

fn create_directory(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
) -> ToolResult {
    let Some(path_str) = get_str_arg(args, &["path"]) else {
        return ToolResult::err("missing required arg: path");
    };

    let path = workspace_root.join(path_str);
    match fs::create_dir_all(&path) {
        Ok(_) => ToolResult::ok(format!("Created directory: {}", path.display())),
        Err(e) => ToolResult::err(format!("Failed to create directory: {}", e)),
    }
}

fn delete_file(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path_str) = get_str_arg(args, &["path"]) else {
        return ToolResult::err("missing required arg: path");
    };

    let path = workspace_root.join(path_str);
    let recursive = args
        .get("recursive")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if path.is_dir() {
        if !recursive {
            return ToolResult::err("recursive flag required to delete directories");
        }
        match fs::remove_dir_all(&path) {
            Ok(_) => ToolResult::ok(format!("Deleted directory: {}", path.display())),
            Err(e) => ToolResult::err(format!("Failed to delete directory: {}", e)),
        }
    } else {
        match fs::remove_file(&path) {
            Ok(_) => ToolResult::ok(format!("Deleted file: {}", path.display())),
            Err(e) => ToolResult::err(format!("Failed to delete file: {}", e)),
        }
    }
}

fn move_file(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(source_str) = get_str_arg(args, &["source"]) else {
        return ToolResult::err("missing required arg: source");
    };
    let Some(dest_str) = get_str_arg(args, &["destination"]) else {
        return ToolResult::err("missing required arg: destination");
    };

    let source = workspace_root.join(source_str);
    let dest = workspace_root.join(dest_str);

    match fs::rename(&source, &dest) {
        Ok(_) => ToolResult::ok(format!("Moved {} to {}", source.display(), dest.display())),
        Err(e) => ToolResult::err(format!("Failed to move file: {}", e)),
    }
}

fn copy_file(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(source_str) = get_str_arg(args, &["source"]) else {
        return ToolResult::err("missing required arg: source");
    };
    let Some(dest_str) = get_str_arg(args, &["destination"]) else {
        return ToolResult::err("missing required arg: destination");
    };

    let source = workspace_root.join(source_str);
    let dest = workspace_root.join(dest_str);

    if source.is_dir() {
        // Recursive directory copy
        match copy_dir_recursive(&source, &dest) {
            Ok(_) => ToolResult::ok(format!(
                "Copied directory {} to {}",
                source.display(),
                dest.display()
            )),
            Err(e) => ToolResult::err(format!("Failed to copy directory: {}", e)),
        }
    } else {
        match fs::copy(&source, &dest) {
            Ok(_) => ToolResult::ok(format!("Copied {} to {}", source.display(), dest.display())),
            Err(e) => ToolResult::err(format!("Failed to copy file: {}", e)),
        }
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if ty.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn get_file_info(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path_str) = get_str_arg(args, &["path"]) else {
        return ToolResult::err("missing required arg: path");
    };

    let path = workspace_root.join(path_str);
    match fs::metadata(&path) {
        Ok(metadata) => {
            let info = serde_json::json!({
                "path": path.display().to_string(),
                "size": metadata.len(),
                "is_directory": metadata.is_dir(),
                "is_file": metadata.is_file(),
                "modified": metadata.modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()),
                "readonly": metadata.permissions().readonly(),
            });
            ToolResult::ok(serde_json::to_string_pretty(&info).unwrap_or_default())
        }
        Err(e) => ToolResult::err(format!("Failed to get file info: {}", e)),
    }
}

fn open_file(args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path"]) else {
        return ToolResult::err("missing required arg: path");
    };

    let line = args
        .get("line")
        .and_then(|v| v.as_u64())
        .map(|n| n as usize);

    // This tool returns a special format that the frontend will intercept
    // and use to open the file in the editor
    let mut result = serde_json::json!({
        "action": "open_file",
        "path": path,
    });

    if let Some(line_num) = line {
        result["line"] = serde_json::json!(line_num);
    }

    ToolResult::ok(serde_json::to_string(&result).unwrap_or_default())
}

fn goto_line(args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(line) = args.get("line").and_then(|v| v.as_u64()) else {
        return ToolResult::err("missing required arg: line");
    };

    let column = args.get("column").and_then(|v| v.as_u64());

    let mut result = serde_json::json!({
        "action": "goto_line",
        "line": line,
    });

    if let Some(col) = column {
        result["column"] = serde_json::json!(col);
    }

    ToolResult::ok(serde_json::to_string(&result).unwrap_or_default())
}

fn get_selection(editor_state: Option<&EditorState>) -> ToolResult {
    let Some(state) = editor_state else {
        return ToolResult::err("editor state not available");
    };

    // For now, return a placeholder - this needs to be implemented in the frontend
    // to actually track selection state
    let result = serde_json::json!({
        "action": "get_selection",
        "selection": state.active_file.as_ref().map(|_| "<selection not yet implemented>"),
    });

    ToolResult::ok(serde_json::to_string(&result).unwrap_or_default())
}

fn replace_selection(args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(content) = get_str_arg(args, &["content"]) else {
        return ToolResult::err("missing required arg: content");
    };

    let result = serde_json::json!({
        "action": "replace_selection",
        "content": content,
    });

    ToolResult::ok(serde_json::to_string(&result).unwrap_or_default())
}

fn insert_at_cursor(args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(content) = get_str_arg(args, &["content"]) else {
        return ToolResult::err("missing required arg: content");
    };

    let result = serde_json::json!({
        "action": "insert_at_cursor",
        "content": content,
    });

    ToolResult::ok(serde_json::to_string(&result).unwrap_or_default())
}
