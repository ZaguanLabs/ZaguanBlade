use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use lazy_static::lazy_static;
use regex::Regex;
use serde::Deserialize;
use walkdir::WalkDir;

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

fn get_optional_bounded_usize_arg(
    args: &HashMap<String, serde_json::Value>,
    keys: &[&str],
    cap: usize,
) -> Option<usize> {
    for k in keys {
        if let Some(value) = args.get(*k) {
            if let Some(n) = value.as_u64() {
                return Some((n as usize).min(cap));
            }
        }
    }
    None
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

fn get_string_arg(args: &HashMap<String, serde_json::Value>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = args.get(*key).and_then(|value| value.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
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
        | "fast_context"
        | "symbol_search"
        | "semantic_anchor_search"
        | "symbol_resolve"
        | "symbol_related"
        | "symbol_references"
        | "edit_impact"
        | "symbol_graph"
        | "symbol_trace"
        | "symbol_schema"
        | "symbol_outline"
        | "read_file"
        | "read_file_range"
        | "load_skill"
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
const READ_MANY_FILES_MAX_BYTES_PER_FILE_CAP: usize = 512 * 1024;

const GREP_TIMEOUT_DEFAULT_MS: u64 = 8_000;
const GREP_TIMEOUT_MIN_MS: u64 = 500;
const GREP_TIMEOUT_MAX_MS: u64 = 30_000;
const GREP_SEARCH_DEFAULT_MAX_RESULTS: usize = 20;
const GREP_SEARCH_MAX_RESULTS_CAP: usize = 20;
const DEPENDENCY_DIRS: &[&str] = &["node_modules", "vendor"];

const TOOL_METRICS_SAMPLE_CAP: usize = 512;
const SYMBOL_OUTLINE_DEFAULT_MAX_SYMBOLS: usize = 120;
const SYMBOL_OUTLINE_MAX_SYMBOLS_CAP: usize = 300;
const SYMBOL_OUTLINE_DEFAULT_MAX_NODES: usize = 120;
const SYMBOL_OUTLINE_MAX_NODES_CAP: usize = 500;
const SYMBOL_OUTLINE_DEFAULT_MAX_DEPTH: usize = 4;
const SYMBOL_OUTLINE_MAX_DEPTH_CAP: usize = 12;
const SYMBOL_SEARCH_MAX_OFFSET: usize = 1000;
const SYMBOL_TEXT_PREVIEW_CHARS: usize = 240;

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

    pub fn to_tool_content_for_tool(&self, tool_name: &str) -> String {
        if self.success {
            return self.content.clone();
        }
        if self.skipped {
            return self.to_tool_content();
        }

        let raw = self.to_tool_content();
        let Some(error) = self.error.as_deref() else {
            return raw;
        };
        let feedback = build_tool_failure_feedback(tool_name, error);
        if feedback.is_empty() {
            raw
        } else {
            format!("{raw}\n\n{feedback}")
        }
    }

    pub fn to_tool_content_truncated(&self) -> String {
        let content = self.to_tool_content();
        truncate_large_content(&content)
    }

    pub fn to_tool_content_truncated_for_tool(&self, tool_name: &str) -> String {
        let content = self.to_tool_content_for_tool(tool_name);
        truncate_large_content(&content)
    }
}

fn build_tool_failure_feedback(tool_name: &str, error: &str) -> String {
    let lower = error.to_lowercase();
    let mut lines = vec!["ZaguanBlade feedback:".to_string()];

    match tool_name {
        "run_command" => {
            if lower.contains("exit code") {
                lines.push(
                    "- What happened: the command executed, but the process returned a non-zero exit code.".to_string(),
                );
                lines.push(
                    "- Suggested next step: read stdout/stderr above as the authoritative diagnostic, fix the underlying issue, and avoid retrying the exact same command unless something changed.".to_string(),
                );
            } else if lower.contains("cwd") || lower.contains("directory") {
                lines.push(
                    "- What happened: the command could not be started in the requested working directory.".to_string(),
                );
                lines.push(
                    "- Suggested next step: verify the workspace-relative cwd and use an existing directory inside the workspace.".to_string(),
                );
            } else {
                lines.push(
                    "- What happened: the command failed before or during execution.".to_string(),
                );
                lines.push(
                    "- Suggested next step: inspect the error above, check the command/cwd/environment, and explain the likely cause before choosing a safer next action.".to_string(),
                );
            }
        }
        "apply_patch"
        | "apply_patch_validated"
        | "apply_edit"
        | "replace_file_content"
        | "multi_replace_file_content"
        | "edit_file" => {
            if lower.contains("old_text not found") {
                lines.push(
                    "- What happened: the requested old_text did not exactly match the current file contents.".to_string(),
                );
                lines.push(
                    "- Suggested next step: read the current file or relevant line range again, then retry with an exact snippet including whitespace and surrounding context.".to_string(),
                );
            } else if lower.contains("ambiguous match") {
                lines.push(
                    "- What happened: the patch matched more than one location, so applying it would be unsafe.".to_string(),
                );
                lines.push(
                    "- Suggested next step: retry with a more unique old_text block or include accurate start_line/end_line hints.".to_string(),
                );
            } else if lower.contains("missing required arg") || lower.contains("must be") {
                lines.push(
                    "- What happened: the edit request did not match the expected tool schema."
                        .to_string(),
                );
                lines.push(
                    "- Suggested next step: correct the tool arguments before retrying; do not change strategy until the payload shape is valid.".to_string(),
                );
            } else {
                lines.push(
                    "- What happened: the edit tool could not safely apply the requested file change.".to_string(),
                );
                lines.push(
                    "- Suggested next step: use the raw error above as the source of truth, re-read the target content if needed, and retry with a smaller, more precise change.".to_string(),
                );
            }
        }
        "read_file" | "read_file_range" | "read_many_files" => {
            lines.push(
                "- What happened: the file-read request could not return the requested content."
                    .to_string(),
            );
            lines.push(
                "- Suggested next step: verify the path, line range, and workspace scope before relying on assumptions about the file.".to_string(),
            );
        }
        "grep_search" | "codebase_search" | "symbol_search" | "codebase_investigator" => {
            lines.push(
                "- What happened: the search/investigation tool did not complete successfully."
                    .to_string(),
            );
            lines.push(
                "- Suggested next step: narrow the query, reduce scope, or switch to a more targeted file/path search.".to_string(),
            );
        }
        _ => {
            lines.push("- What happened: the tool reported a failure.".to_string());
            lines.push(
                "- Suggested next step: treat the raw error above as authoritative, explain it briefly to the user, and choose the smallest safe corrective action.".to_string(),
            );
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::{
        apply_multi_patch_to_string, apply_patch_to_string, apply_patch_to_string_with_line_hint,
        apply_semantic_patch_with_service, apply_semantic_patch_writes_with_service,
        compact_outline_nodes_for_parent, execute_tool, fast_context_tool, grep_search,
        impact_confidence, impact_risk_level, is_batch_read_only_tool,
        language_support_for_path_json, language_support_meta_json, paginate_tool_results,
        parse_grep_timeout_ms, parse_relationship_types_arg, related_test_files_for_paths,
        stage_semantic_patch_writes, symbol_inventory_entries, symbol_inventory_summary,
        symbol_language_diagnostics, symbol_outline_diagnostics, symbol_reference_resolution_json,
        symbol_search_connection_json, symbol_to_json, PatchHunk, SemanticPatchWrite, ToolResult,
        GREP_TIMEOUT_DEFAULT_MS, GREP_TIMEOUT_MAX_MS, GREP_TIMEOUT_MIN_MS,
    };
    use crate::semantic_patch::{InsertPosition, PatchOperation, PatchTarget, SemanticPatch};
    use crate::symbol_index::SymbolStore;
    use crate::tree_sitter::{Position, Range, Symbol, SymbolType};
    use std::collections::HashMap;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn test_symbol(
        id: &str,
        name: &str,
        symbol_type: SymbolType,
        start_line: u32,
        parent_id: Option<&str>,
    ) -> Symbol {
        let mut symbol = Symbol::new(
            name.to_string(),
            symbol_type,
            "src/example.ts".to_string(),
            Range::new(Position::new(start_line, 0), Position::new(start_line, 10)),
        );
        symbol.id = id.to_string();
        symbol.qualified_name = format!("src/example.ts::{}", name);
        symbol.parent_id = parent_id.map(str::to_string);
        symbol.signature = Some(format!("{}()", name));
        symbol
    }

    #[test]
    fn language_support_metadata_marks_known_language() {
        let metadata = language_support_for_path_json("src/main.ts");
        assert_eq!(metadata["supported"], true);
        assert_eq!(metadata["display_name"], "TypeScript");
        assert_eq!(metadata["support_level"], "full");
        assert_eq!(metadata["parser"], "tree_sitter");
        assert_eq!(metadata["extracts"]["definitions"], true);
    }

    #[test]
    fn language_support_metadata_marks_css_partial_support() {
        let metadata = language_support_for_path_json("src/styles/app.css");
        assert_eq!(metadata["supported"], true);
        assert_eq!(metadata["display_name"], "CSS");
        assert_eq!(metadata["support_level"], "partial");
        assert_eq!(metadata["parser"], "scanner");

        let diagnostics = symbol_language_diagnostics(Some("src/styles/app.css"), 0);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("partial symbol support")),
            "expected partial-support diagnostic, got {diagnostics:?}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("No indexed symbols matched")),
            "expected fallback guidance, got {diagnostics:?}"
        );
    }

    #[test]
    fn language_support_metadata_marks_stylesheet_variants_partial_support() {
        for (path, display_name) in [
            ("src/styles/app.scss", "SCSS"),
            ("src/styles/app.sass", "Sass"),
            ("src/styles/app.less", "Less"),
            ("src/styles/Button.module.scss", "SCSS"),
        ] {
            let metadata = language_support_for_path_json(path);
            assert_eq!(metadata["supported"], true, "{path}");
            assert_eq!(metadata["display_name"], display_name, "{path}");
            assert_eq!(metadata["support_level"], "partial", "{path}");
            assert_eq!(metadata["parser"], "scanner", "{path}");
        }
    }

    #[test]
    fn language_support_metadata_marks_markup_variants_partial_support() {
        for (path, display_name) in [
            ("public/index.html", "HTML"),
            ("public/index.htm", "HTML"),
            ("src/App.vue", "Vue"),
            ("src/App.svelte", "Svelte"),
        ] {
            let metadata = language_support_for_path_json(path);
            assert_eq!(metadata["supported"], true, "{path}");
            assert_eq!(metadata["display_name"], display_name, "{path}");
            assert_eq!(metadata["support_level"], "partial", "{path}");
            assert_eq!(metadata["parser"], "scanner", "{path}");
            assert_eq!(metadata["extracts"]["definitions"], true, "{path}");
            assert_eq!(metadata["extracts"]["relationships"], false, "{path}");
        }
    }

    #[test]
    fn language_support_metadata_marks_config_variants_partial_support() {
        for (path, display_name) in [
            ("package.json", "JSON"),
            ("config/app.yaml", "YAML"),
            ("config/app.yml", "YAML"),
            ("Cargo.toml", "TOML"),
            ("pyproject.toml", "TOML"),
        ] {
            let metadata = language_support_for_path_json(path);
            assert_eq!(metadata["supported"], true, "{path}");
            assert_eq!(metadata["display_name"], display_name, "{path}");
            assert_eq!(metadata["support_level"], "partial", "{path}");
            assert_eq!(metadata["parser"], "scanner", "{path}");
            assert_eq!(metadata["extracts"]["definitions"], true, "{path}");
            assert_eq!(metadata["extracts"]["imports"], false, "{path}");
        }
    }

    #[test]
    fn language_support_meta_includes_file_and_supported_languages() {
        let metadata = language_support_meta_json(Some("src/styles/app.css"));
        assert_eq!(metadata["file"]["display_name"], "CSS");
        assert_eq!(metadata["file"]["support_level"], "partial");
        let supported_languages = metadata["supported_languages"]
            .as_array()
            .expect("expected supported language list");
        assert!(!supported_languages.is_empty());
        for display_name in ["CSS", "HTML", "Vue", "Svelte", "JSON", "YAML", "TOML"] {
            assert!(
                supported_languages
                    .iter()
                    .any(|language| language["display_name"] == display_name),
                "expected supported language list to include {display_name}"
            );
        }

        let metadata = language_support_meta_json(None);
        assert!(metadata["file"].is_null());
        assert!(metadata["supported_languages"]
            .as_array()
            .is_some_and(|languages| !languages.is_empty()));
    }

    #[test]
    fn language_support_metadata_marks_unsupported_file_type() {
        let metadata = language_support_for_path_json("src/native/widget.swift");
        assert_eq!(metadata["supported"], false);
        assert_eq!(metadata["support_level"], "unsupported");
        assert_eq!(metadata["parser"], serde_json::Value::Null);
        assert_eq!(metadata["extracts"]["definitions"], false);

        let diagnostics = symbol_language_diagnostics(Some("src/native/widget.swift"), 0);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("not supported by the Symbols Index")),
            "expected unsupported diagnostic, got {diagnostics:?}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("semantic_anchor_search")),
            "expected fallback tool guidance, got {diagnostics:?}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("No indexed symbols matched")),
            "expected empty-result diagnostic, got {diagnostics:?}"
        );
    }

    #[test]
    fn language_support_diagnostics_do_not_emit_empty_warning_when_results_exist() {
        let diagnostics = symbol_language_diagnostics(Some("src/App.vue"), 3);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("partial symbol support")),
            "expected partial-support diagnostic, got {diagnostics:?}"
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| !diagnostic.contains("No indexed symbols matched")),
            "did not expect empty-result diagnostic, got {diagnostics:?}"
        );
    }

    #[test]
    fn symbol_outline_diagnostics_report_partial_support() {
        let diagnostics = symbol_outline_diagnostics("README.md", 0);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("partial symbol support")),
            "expected partial-support diagnostic, got {diagnostics:?}"
        );
    }

    #[test]
    fn symbol_outline_diagnostics_report_unsupported_file_type() {
        let diagnostics = symbol_outline_diagnostics("src/native/widget.swift", 0);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("not supported by the Symbols Index")),
            "expected unsupported diagnostic, got {diagnostics:?}"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.contains("grep_search or codebase_search")),
            "expected fallback search guidance, got {diagnostics:?}"
        );
    }

    #[test]
    fn paginate_tool_results_reports_has_more() {
        let (page, has_more, total_available) = paginate_tool_results(vec![1, 2, 3, 4], 1, 2);
        assert_eq!(page, vec![2, 3]);
        assert!(has_more);
        assert_eq!(total_available, 4);

        let (page, has_more, total_available) = paginate_tool_results(vec![1, 2, 3], 3, 2);
        assert!(page.is_empty());
        assert!(!has_more);
        assert_eq!(total_available, 3);
    }

    #[test]
    fn apply_patch_rejects_ambiguous_exact_matches() {
        let content = "A\nTARGET\nB\nTARGET\nC\n";
        let err = apply_patch_to_string(content, "TARGET", "REPLACED").unwrap_err();
        assert!(err.contains("Ambiguous match"), "unexpected error: {err}");
    }

    #[test]
    fn apply_patch_uses_line_hint_to_disambiguate_exact_matches() {
        let content = "A\nTARGET\nB\nTARGET\nC\n";
        let updated =
            apply_patch_to_string_with_line_hint(content, "TARGET", "REPLACED", Some(4), Some(4))
                .unwrap();
        assert_eq!(updated, "A\nTARGET\nB\nREPLACED\nC\n");
    }

    #[test]
    fn fast_context_is_batch_read_only() {
        assert!(is_batch_read_only_tool("fast_context"));
    }

    #[test]
    fn fast_context_tool_returns_context_pack_payload() {
        let temp_dir = tempdir().unwrap();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::write(
            temp_dir.path().join("src/service.ts"),
            "export function buildContextPack() { return true; }",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("src/service.css"),
            ".contextCard { color: var(--accent); }",
        )
        .unwrap();
        let mut args = HashMap::new();
        args.insert("query".to_string(), serde_json::json!("build context pack"));
        args.insert(
            "active_file".to_string(),
            serde_json::json!("src/service.css"),
        );
        args.insert("include_memory".to_string(), serde_json::json!(false));
        args.insert(
            "include_project_index_min".to_string(),
            serde_json::json!(false),
        );

        let result = fast_context_tool(temp_dir.path(), &args, None);
        let payload: serde_json::Value = serde_json::from_str(&result.content).unwrap();

        assert!(result.success);
        assert_eq!(payload["queries_used"][0], "build context pack");
        assert!(payload.get("confidence").is_some());

        let language_support = &payload["_meta"]["language_support"];
        assert_eq!(language_support["file"]["display_name"], "CSS");
        assert_eq!(language_support["file"]["support_level"], "partial");
        assert!(language_support["supported_languages"]
            .as_array()
            .is_some_and(|languages| !languages.is_empty()));
        let metadata_len = serde_json::to_string(language_support).unwrap().len();
        assert!(
            metadata_len < 8_000,
            "fast_context language-support metadata should stay compact, got {metadata_len} bytes"
        );

        let schema_summary = &payload["_meta"]["index_schema_summary"];
        assert!(schema_summary["totals"]["indexed_files"].is_number());
        let schema_summary_len = serde_json::to_string(schema_summary).unwrap().len();
        assert!(
            schema_summary_len < 4_000,
            "fast_context schema summary should stay compact, got {schema_summary_len} bytes"
        );
    }

    #[test]
    fn edit_impact_risk_and_confidence_are_bounded_and_explainable() {
        assert_eq!(impact_risk_level(9, 2, 1), "high");
        assert_eq!(impact_risk_level(2, 5, 1), "medium");
        assert_eq!(impact_risk_level(1, 1, 1), "low");
        assert_eq!(impact_risk_level(1, 1, 0), "medium");

        assert_eq!(impact_confidence(true, 1, 1), "high");
        assert_eq!(impact_confidence(false, 1, 1), "medium");
        assert_eq!(impact_confidence(true, 0, 0), "low");
    }

    #[test]
    fn edit_impact_discovers_likely_tests_by_impacted_stem() {
        let temp_dir = tempdir().unwrap();
        fs::create_dir_all(temp_dir.path().join("src")).unwrap();
        fs::create_dir_all(temp_dir.path().join("tests")).unwrap();
        fs::write(
            temp_dir.path().join("src/service.ts"),
            "export function service() {}",
        )
        .unwrap();
        fs::write(
            temp_dir.path().join("tests/service.test.ts"),
            "test('service', () => {})",
        )
        .unwrap();

        let tests =
            related_test_files_for_paths(temp_dir.path(), &["src/service.ts".to_string()], 5);

        assert!(tests
            .iter()
            .any(|test| test["path"] == "tests/service.test.ts"));
    }

    #[test]
    fn symbol_reference_resolution_marks_resolved_and_fallback_confidence() {
        let source = test_symbol("caller", "caller", SymbolType::Function, 1, None);
        let target = test_symbol("helper", "helper", SymbolType::Function, 3, None);
        let resolved = crate::symbol_index::SymbolReference {
            source_symbol: source.clone(),
            relationship_type: crate::tree_sitter::SymbolRelationshipType::Call,
            target_name: "helper".to_string(),
            target_symbol_id: Some(target.id.clone()),
            target_symbol: Some(target),
            line: 2,
        };
        let fallback = crate::symbol_index::SymbolReference {
            source_symbol: source,
            relationship_type: crate::tree_sitter::SymbolRelationshipType::Call,
            target_name: "helper".to_string(),
            target_symbol_id: None,
            target_symbol: None,
            line: 2,
        };

        assert_eq!(
            symbol_reference_resolution_json(&resolved)["strategy"],
            "resolved_symbol_id"
        );
        assert_eq!(
            symbol_reference_resolution_json(&resolved)["confidence"],
            "high"
        );
        assert_eq!(
            symbol_reference_resolution_json(&fallback)["strategy"],
            "name_fallback"
        );
        assert_eq!(
            symbol_reference_resolution_json(&fallback)["confidence"],
            "low"
        );
    }

    #[test]
    fn symbol_search_connection_includes_resolution_metadata() {
        let source = test_symbol("caller", "caller", SymbolType::Function, 1, None);
        let target = test_symbol("helper", "helper", SymbolType::Function, 3, None);
        let reference = crate::symbol_index::SymbolReference {
            source_symbol: source,
            relationship_type: crate::tree_sitter::SymbolRelationshipType::Call,
            target_name: "helper".to_string(),
            target_symbol_id: Some(target.id.clone()),
            target_symbol: Some(target),
            line: 2,
        };

        let connection = symbol_search_connection_json(&reference, "outgoing");

        assert_eq!(connection["resolution"]["strategy"], "resolved_symbol_id");
        assert_eq!(connection["resolution"]["confidence"], "high");
        assert_eq!(connection["resolution"]["resolved"], true);
    }

    #[test]
    fn symbol_json_serialization_escapes_control_characters() {
        let mut symbol = test_symbol(
            "control",
            "quote\"slash\\control",
            SymbolType::Function,
            1,
            None,
        );
        let docstring = "line\n tab\t carriage\r control\u{0001}";
        symbol.docstring = Some(docstring.to_string());

        let payload = symbol_to_json(&symbol);
        let serialized = serde_json::to_string(&payload).unwrap();

        assert!(serialized.contains("\\\""));
        assert!(serialized.contains("\\\\"));
        assert!(serialized.contains("\\n"));
        assert!(serialized.contains("\\t"));
        assert!(serialized.contains("\\r"));
        assert!(serialized.contains("\\u0001"));

        let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
        assert_eq!(parsed["name"], symbol.name);
        assert_eq!(parsed["docstring"], docstring);
    }

    #[test]
    fn parse_relationship_types_supports_single_and_multiple_filters() {
        let mut single = HashMap::new();
        single.insert(
            "relationship".to_string(),
            serde_json::Value::String("import".to_string()),
        );
        assert_eq!(
            parse_relationship_types_arg(&single).unwrap(),
            vec![crate::tree_sitter::SymbolRelationshipType::Import]
        );

        let mut multiple = HashMap::new();
        multiple.insert(
            "relationships".to_string(),
            serde_json::json!(["call", "export"]),
        );
        assert_eq!(
            parse_relationship_types_arg(&multiple).unwrap(),
            vec![
                crate::tree_sitter::SymbolRelationshipType::Call,
                crate::tree_sitter::SymbolRelationshipType::Export,
            ]
        );
    }

    #[test]
    fn symbol_inventory_summary_counts_types_and_hierarchy() {
        let symbols = vec![
            test_symbol("module", "example", SymbolType::Module, 0, None),
            test_symbol("class", "UserService", SymbolType::Class, 1, None),
            test_symbol("method", "getUser", SymbolType::Method, 2, Some("class")),
        ];

        let summary = symbol_inventory_summary(&symbols);

        assert_eq!(summary["total_symbols"], 3);
        assert_eq!(summary["top_level_symbols"], 2);
        assert_eq!(summary["symbols_with_children"], 1);
        assert_eq!(summary["by_type"]["class"], 1);
        assert_eq!(summary["by_type"]["method"], 1);
        assert_eq!(summary["by_type"]["module"], 1);
    }

    #[test]
    fn symbol_inventory_entries_are_ordered_bounded_and_include_child_counts() {
        let symbols = vec![
            test_symbol("method", "getUser", SymbolType::Method, 20, Some("class")),
            test_symbol("class", "UserService", SymbolType::Class, 10, None),
            test_symbol("helper", "helper", SymbolType::Function, 30, None),
        ];

        let entries = symbol_inventory_entries(&symbols, 2, false);

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["name"], "UserService");
        assert_eq!(entries[0]["child_count"], 1);
        assert_eq!(entries[0]["line_range"]["start_line"], 10);
        assert_eq!(entries[1]["name"], "getUser");
        assert!(entries[0]["docstring"].is_null());
    }

    #[test]
    fn compact_outline_nodes_are_bounded_and_do_not_emit_full_symbol_payloads() {
        let symbols = vec![
            test_symbol("class", "UserService", SymbolType::Class, 10, None),
            test_symbol("method", "getUser", SymbolType::Method, 20, Some("class")),
            test_symbol("helper", "helper", SymbolType::Function, 30, None),
        ];
        let mut by_parent = HashMap::new();
        for symbol in symbols {
            by_parent
                .entry(symbol.parent_id.clone())
                .or_insert_with(Vec::new)
                .push(symbol);
        }

        let mut emitted = 0usize;
        let nodes = compact_outline_nodes_for_parent(&by_parent, None, 2, 4, 0, &mut emitted);

        assert_eq!(emitted, 2);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0]["name"], "UserService");
        assert_eq!(nodes[0]["child_count"], 1);
        assert_eq!(nodes[0]["children"][0]["name"], "getUser");
        assert!(nodes[0]["byte_offset"].is_null());
    }

    #[test]
    fn apply_patch_replaces_single_exact_match() {
        let content = "A\nTARGET\nB\n";
        let updated = apply_patch_to_string(content, "TARGET", "REPLACED").unwrap();
        assert_eq!(updated, "A\nREPLACED\nB\n");
    }

    #[test]
    fn execute_tool_supports_apply_patch_validated() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("example.txt");
        fs::write(&file_path, "hello\nworld\n").expect("write test file");

        let result = execute_tool(
            workspace.path(),
            "apply_patch_validated",
            r#"{"path":"example.txt","old_text":"world","new_text":"blade"}"#,
        );

        assert!(result.success, "apply_patch_validated should succeed");
        let updated = fs::read_to_string(&file_path).expect("read updated file");
        assert_eq!(updated, "hello\nblade\n");
    }

    #[test]
    fn failed_tool_content_includes_recovery_feedback_for_patch_failures() {
        let result = ToolResult::err(
            "old_text not found in file (searched 12 chars). Exact match required.",
        );

        let content = result.to_tool_content_for_tool("apply_patch");

        assert!(content.contains("tool_error: old_text not found"));
        assert!(content.contains("ZaguanBlade feedback:"));
        assert!(content.contains("read the current file or relevant line range again"));
    }

    #[test]
    fn skipped_tool_content_does_not_add_recovery_feedback() {
        let result = ToolResult::skipped("User skipped this action.");

        let content = result.to_tool_content_for_tool("apply_patch");

        assert!(content.contains("tool_error: User skipped this action."));
        assert!(!content.contains("ZaguanBlade feedback:"));
    }

    #[test]
    fn apply_patch_rejects_whitespace_only_fuzzy_match() {
        let content = "    TARGET   \nB\n";
        let err = apply_patch_to_string(content, "TARGET\n", "TARGET\nEXTRA\n").unwrap_err();
        assert!(
            err.contains("Exact match required"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn apply_multi_patch_rejects_whitespace_only_fuzzy_validation() {
        let content = "    TARGET   \nB\n";
        let patches = vec![PatchHunk {
            old_text: "TARGET\n".to_string(),
            new_text: "TARGET\nEXTRA\n".to_string(),
            start_line: None,
            end_line: None,
        }];
        let err = apply_multi_patch_to_string(content, &patches).unwrap_err();
        assert!(
            err.contains("old_text not found in file"),
            "unexpected error: {err}"
        );
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
        assert!(
            result.success,
            "batch should return structured all-settled output"
        );

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
    fn batch_preserves_full_nested_tool_output() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("large.txt");
        let content = (0..220)
            .map(|i| format!("line {i:03} {}", "x".repeat(300)))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&file_path, &content).expect("write test file");

        let args = r#"{
            "calls": [
                {"tool": "read_file", "arguments": {"path": "large.txt"}}
            ]
        }"#;

        let result = execute_tool(workspace.path(), "batch", args);
        assert!(result.success, "batch should succeed");

        let payload: serde_json::Value =
            serde_json::from_str(&result.content).expect("batch json output");
        let output = payload["results"][0]["output"]
            .as_str()
            .expect("nested output should be string");

        assert!(output.contains("line 000"));
        assert!(output.contains("line 219"));
        assert!(
            !output.contains("[TRUNCATED:"),
            "nested batch output should not be pre-truncated"
        );
    }

    #[test]
    fn read_file_returns_disk_content() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("current.txt");
        fs::write(&file_path, "disk content\n").expect("write test file");

        let result = execute_tool(workspace.path(), "read_file", r#"{"path":"current.txt"}"#);

        assert!(result.success, "read_file should succeed");
        assert!(result.content.contains("disk content"));
    }

    #[test]
    fn load_skill_returns_workspace_skill_body() {
        let workspace = tempdir().expect("tempdir");
        let skill_path = workspace.path().join(".agents/skills/example/SKILL.md");
        fs::create_dir_all(skill_path.parent().expect("skill path parent")).unwrap();
        fs::write(
            &skill_path,
            "---\nid: example\ndescription: Example workflow\n---\nRead references/guide.md.",
        )
        .expect("write skill");
        fs::create_dir_all(workspace.path().join(".agents/skills/example/references")).unwrap();
        fs::write(
            workspace
                .path()
                .join(".agents/skills/example/references/guide.md"),
            "reference details",
        )
        .expect("write referenced file");

        let result = execute_tool(workspace.path(), "load_skill", r#"{"skill_id":"example"}"#);

        assert!(result.success, "load_skill should succeed");
        let payload: serde_json::Value =
            serde_json::from_str(&result.content).expect("load_skill json output");
        assert_eq!(payload["skill_id"].as_str(), Some("example"));
        assert_eq!(payload["base_dir"].as_str(), Some(".agents/skills/example"));
        assert!(payload["content"]
            .as_str()
            .unwrap_or_default()
            .contains("Read references/guide.md."));
        assert!(!result.content.contains("reference details"));
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
    fn read_many_files_returns_full_content_by_default() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("long.txt");
        let content = "x".repeat(70 * 1024);
        fs::write(&file_path, &content).expect("write test file");

        let result = execute_tool(
            workspace.path(),
            "read_many_files",
            r#"{"paths":["long.txt"],"max_files":10,"include_line_numbers":false}"#,
        );
        assert!(result.success, "read_many_files should succeed");

        let payload: serde_json::Value =
            serde_json::from_str(&result.content).expect("read_many_files json output");
        let file = &payload["files"][0];
        assert_eq!(file["truncated"].as_bool(), Some(false));
        assert_eq!(file["content"].as_str(), Some(content.as_str()));
        assert_eq!(
            payload["summary"]["truncated_files"].as_u64(),
            Some(0),
            "default read_many_files should not truncate per-file content"
        );
    }

    #[test]
    fn read_many_files_honors_explicit_max_bytes_per_file() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("long.txt");
        fs::write(&file_path, "abcdefghij").expect("write test file");

        let result = execute_tool(
            workspace.path(),
            "read_many_files",
            r#"{"paths":["long.txt"],"max_files":10,"max_bytes_per_file":4,"include_line_numbers":false}"#,
        );
        assert!(result.success, "read_many_files should succeed");

        let payload: serde_json::Value =
            serde_json::from_str(&result.content).expect("read_many_files json output");
        let file = &payload["files"][0];
        assert_eq!(file["truncated"].as_bool(), Some(true));
        assert_eq!(file["content"].as_str(), Some("abcd"));
        assert_eq!(file["byte_count"].as_u64(), Some(4));
        assert_eq!(file["original_byte_count"].as_u64(), Some(10));
        assert_eq!(payload["summary"]["truncated_files"].as_u64(), Some(1));
    }

    #[test]
    fn grep_timeout_clamps_min_default_max() {
        let empty = HashMap::new();
        assert_eq!(parse_grep_timeout_ms(&empty), GREP_TIMEOUT_DEFAULT_MS);

        let below_min =
            serde_json::from_str::<HashMap<String, serde_json::Value>>(r#"{"timeout_ms":100}"#)
                .expect("parse args");
        assert_eq!(parse_grep_timeout_ms(&below_min), GREP_TIMEOUT_MIN_MS);

        let above_max =
            serde_json::from_str::<HashMap<String, serde_json::Value>>(r#"{"timeout_ms":999999}"#)
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
    fn grep_search_honors_max_results() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("many.txt");
        fs::write(
            &file_path,
            "needle one\nneedle two\nneedle three\nneedle four\nneedle five\n",
        )
        .expect("write test file");

        let result = grep_search(
            workspace.path(),
            &serde_json::from_str::<HashMap<String, serde_json::Value>>(
                r#"{"pattern":"needle","path":".","timeout_ms":8000,"max_results":2}"#,
            )
            .expect("parse args"),
            true,
        );

        assert!(result.success);
        assert_eq!(result.content.lines().count(), 2);
        assert!(result.content.contains("needle one"));
        assert!(result.content.contains("needle two"));
        assert!(!result.content.contains("needle three"));
    }

    #[test]
    fn grep_search_defaults_to_twenty_results() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("many.txt");
        let text = (1..=25)
            .map(|idx| format!("needle {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&file_path, format!("{text}\n")).expect("write test file");

        let result = grep_search(
            workspace.path(),
            &serde_json::from_str::<HashMap<String, serde_json::Value>>(
                r#"{"pattern":"needle","path":".","timeout_ms":8000}"#,
            )
            .expect("parse args"),
            true,
        );

        assert!(result.success);
        assert_eq!(result.content.lines().count(), 20);
        assert!(result.content.contains("needle 20"));
        assert!(!result.content.contains("needle 21"));
    }

    #[test]
    fn grep_search_clamps_max_results_to_twenty() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("many.txt");
        let text = (1..=25)
            .map(|idx| format!("needle {idx}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&file_path, format!("{text}\n")).expect("write test file");

        let result = grep_search(
            workspace.path(),
            &serde_json::from_str::<HashMap<String, serde_json::Value>>(
                r#"{"pattern":"needle","path":".","timeout_ms":8000,"max_results":999}"#,
            )
            .expect("parse args"),
            true,
        );

        assert!(result.success);
        assert_eq!(result.content.lines().count(), 20);
        assert!(result.content.contains("needle 20"));
        assert!(!result.content.contains("needle 21"));
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

    #[test]
    fn semantic_patch_persists_cross_file_move_and_reindexes() {
        let workspace = tempdir().expect("tempdir");
        let db_path = workspace.path().join("symbols.db");
        let store = Arc::new(SymbolStore::new(&db_path).expect("symbol store"));
        let service = Arc::new(
            crate::language_service::LanguageService::new(workspace.path().to_path_buf(), store)
                .expect("language service"),
        );

        let source_path = workspace.path().join("move_source.ts");
        let target_path = workspace.path().join("move_target.ts");
        fs::write(&source_path, "function first() { return 1; }\n").expect("write source");
        fs::write(&target_path, "const before = 0;\nconst after = 1;\n").expect("write target");
        service.index_file("move_source.ts").expect("index source");
        service.index_file("move_target.ts").expect("index target");

        let patch = SemanticPatch {
            id: "persist-cross-file-move".to_string(),
            description: "Move symbol across files".to_string(),
            file_path: "move_source.ts".to_string(),
            operation: PatchOperation::Move {
                target_file: "move_target.ts".to_string(),
                target_position: InsertPosition::AtLine(2),
            },
            target: PatchTarget::Symbol {
                name: "first".to_string(),
                symbol_type: Some(crate::tree_sitter::SymbolType::Function),
            },
            content: None,
            confidence: 1.0,
        };

        let affected_paths = apply_semantic_patch_with_service(workspace.path(), &service, &patch)
            .expect("apply semantic patch");

        assert_eq!(
            affected_paths,
            vec!["move_source.ts".to_string(), "move_target.ts".to_string()]
        );
        assert_eq!(fs::read_to_string(&source_path).expect("read source"), "");
        assert_eq!(
            fs::read_to_string(&target_path).expect("read target"),
            "const before = 0;\nfunction first() { return 1; }\nconst after = 1;\n"
        );
        assert_eq!(
            service
                .get_file_content("move_target.ts")
                .expect("get indexed target"),
            "const before = 0;\nfunction first() { return 1; }\nconst after = 1;\n"
        );
    }

    #[test]
    fn semantic_patch_pre_commit_hook_runs_before_disk_mutation() {
        let workspace = tempdir().expect("tempdir");
        let db_path = workspace.path().join("symbols.db");
        let store = Arc::new(SymbolStore::new(&db_path).expect("symbol store"));
        let service = Arc::new(
            crate::language_service::LanguageService::new(workspace.path().to_path_buf(), store)
                .expect("language service"),
        );

        let file_path = workspace.path().join("replace_target.ts");
        fs::write(&file_path, "function oldName() { return 1; }\n").expect("write target");
        service
            .index_file("replace_target.ts")
            .expect("index target");

        let patch = SemanticPatch {
            id: "pre-commit-before-mutation".to_string(),
            description: "Replace symbol".to_string(),
            file_path: "replace_target.ts".to_string(),
            operation: PatchOperation::Replace,
            target: PatchTarget::Symbol {
                name: "oldName".to_string(),
                symbol_type: Some(crate::tree_sitter::SymbolType::Function),
            },
            content: Some("function newName() { return 2; }\n".to_string()),
            confidence: 1.0,
        };

        let mut observed_disk_content = String::new();
        let writes = apply_semantic_patch_writes_with_service(
            workspace.path(),
            &service,
            &patch,
            |pending_writes| {
                assert_eq!(pending_writes.len(), 1);
                observed_disk_content =
                    fs::read_to_string(&pending_writes[0].abs_path).expect("read pre-commit file");
            },
        )
        .expect("apply semantic patch");

        assert_eq!(writes.len(), 1);
        assert_eq!(observed_disk_content, "function oldName() { return 1; }\n");
        assert_eq!(
            fs::read_to_string(&file_path).expect("read committed file"),
            "function newName() { return 2; }\n\n"
        );
    }

    #[test]
    fn stage_semantic_patch_writes_preserves_originals_until_commit() {
        let workspace = tempdir().expect("tempdir");
        let file_path = workspace.path().join("atomic.ts");
        fs::write(&file_path, "const before = 1;\n").expect("write original");

        let staged = stage_semantic_patch_writes(vec![SemanticPatchWrite {
            file_path: "atomic.ts".to_string(),
            abs_path: file_path.clone(),
            original_content: "const before = 1;\n".to_string(),
            new_content: "const after = 2;\n".to_string(),
        }])
        .expect("stage writes");

        assert_eq!(
            fs::read_to_string(&file_path).expect("read original after staging"),
            "const before = 1;\n"
        );
        assert_eq!(
            fs::read_to_string(&staged[0].temp_path).expect("read staged temp"),
            "const after = 2;\n"
        );
    }
}

/// Truncate large content per RFC-LARGE-TOOL-RESULTS.md
/// Shows first 100 lines + last 50 lines with truncation message.
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
        "read_file" => read_file_with_app(workspace_root, &args, app_handle),
        "write_file" | "write_file_validated" | "create_file" | "write_to_file" => {
            write_file(workspace_root, &args, app_handle)
        }
        "edit_file" => edit_file(workspace_root, &args, app_handle),
        "grep_search" | "rg" => grep_search(workspace_root, &args, grep_timeout_enforced),
        "codebase_search" => codebase_search(workspace_root, &args),
        "list_directory" | "list_dir" => list_directory(workspace_root, &args),

        // Phase 1 IDE-specific tools
        "get_editor_state" => get_editor_state(editor_state),
        "fast_context" => fast_context_tool(workspace_root, &args, editor_state),
        "symbol_search" => symbol_search_tool(workspace_root, &args, app_handle),
        "semantic_anchor_search" => semantic_anchor_search_tool(workspace_root, &args, app_handle),
        "symbol_resolve" => symbol_resolve_tool(workspace_root, &args, app_handle),
        "symbol_related" => symbol_related_tool(workspace_root, &args, app_handle),
        "symbol_references" => symbol_references_tool(workspace_root, &args, app_handle),
        "edit_impact" => edit_impact_tool(workspace_root, &args, app_handle),
        "symbol_graph" => symbol_graph_tool(workspace_root, &args, app_handle),
        "symbol_trace" => symbol_trace_tool(workspace_root, &args, app_handle),
        "symbol_schema" => symbol_schema_tool(&args, app_handle),
        "symbol_outline" => symbol_outline_tool(workspace_root, &args, app_handle),
        "read_file_range" => read_file_range(workspace_root, &args),
        "load_skill" => load_skill_tool(workspace_root, &args),
        "apply_edit"
        | "apply_patch"
        | "apply_patch_validated"
        | "replace_file_content"
        | "multi_replace_file_content" => {
            apply_edit_tool(workspace_root, &args, app_handle)
        }
        "get_workspace_structure" => get_workspace_structure(workspace_root, &args),
        "get_project_index_overview" => get_project_index_overview(workspace_root, &args, app_handle),
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
    let ws = fs::canonicalize(workspace_root)
        .map_err(|e| format!("cannot canonicalize workspace: {}", e))?;

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
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(c) => normalized.push(c),
        }
    }

    normalized
}

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

fn validate_read_path(workspace_root: &Path, path: &Path) -> Result<PathBuf, String> {
    match validate_path_under_workspace(workspace_root, path) {
        Ok(path) => Ok(path),
        Err(workspace_error) => validate_path_under_global_skills(path).map_err(|global_error| {
            format!("{workspace_error}; not a readable global skill resource: {global_error}")
        }),
    }
}

fn validate_path_under_global_skills(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("path is not absolute".to_string());
    }

    let global_skills_dir = crate::config::global_skills_dir();
    let candidate = normalize_path(path);
    let root = normalize_path(&global_skills_dir);

    if !candidate.exists() {
        return Err(format!("path does not exist: {}", candidate.display()));
    }
    if !candidate.starts_with(&root) {
        return Err(format!(
            "path is outside global skills directory (global skills: {}, path: {})",
            root.display(),
            candidate.display()
        ));
    }

    Ok(candidate)
}

fn read_file(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path", "file_path", "filepath", "filename"]) else {
        return ToolResult::err("missing required arg: path (or file_path)");
    };

    let abs = match validate_read_path(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    match fs::read_to_string(&abs) {
        Ok(s) => ToolResult::ok(format_read_file_content(&abs, &s)),
        Err(e) => ToolResult::err(e.to_string()),
    }
}

fn load_skill_tool(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(skill_id) = get_string_arg(args, &["skill_id", "id", "name"]) else {
        return ToolResult::err("missing required arg: skill_id");
    };

    match crate::agent_skills::load_skill(workspace_root, &skill_id) {
        Ok(skill) => ToolResult::ok(serde_json::to_string_pretty(&skill).unwrap_or_default()),
        Err(error) => ToolResult::err(error),
    }
}

fn format_read_file_content(abs: &Path, content: &str) -> String {
    if content.is_empty() {
        format!(
            "=== File: {} (empty) ===\n// This file exists but contains no content.",
            abs.to_string_lossy()
        )
    } else {
        format!("=== File: {} ===\n{}", abs.to_string_lossy(), content)
    }
}

fn read_file_with_app<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    _app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    read_file(workspace_root, args)
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
    let max_bytes_per_file = get_optional_bounded_usize_arg(
        args,
        &["max_bytes_per_file"],
        READ_MANY_FILES_MAX_BYTES_PER_FILE_CAP,
    );
    let include_line_numbers = get_bool_arg(args, &["include_line_numbers"], true);

    let started_at = Instant::now();
    let matched_files = match collect_matching_files(workspace_root, &include_globs, &exclude_globs)
    {
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

    let mut files = Vec::with_capacity(selected_files.len());
    for rel_path in selected_files {
        let abs_path = ws.join(&rel_path);
        match fs::read(&abs_path) {
            Ok(bytes) => {
                let original_byte_count = bytes.len();
                let truncated = max_bytes_per_file
                    .map(|limit| original_byte_count > limit)
                    .unwrap_or(false);
                let selected_bytes = match max_bytes_per_file {
                    Some(limit) if truncated => &bytes[..limit],
                    _ => &bytes[..],
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

                files.push(serde_json::json!({
                    "path": rel_path,
                    "truncated": truncated,
                    "content": content,
                    "line_count": line_count,
                    "byte_count": selected_bytes.len(),
                    "original_byte_count": original_byte_count,
                }));
            }
            Err(e) => {
                files.push(serde_json::json!({
                    "path": rel_path,
                    "truncated": false,
                    "error": e.to_string(),
                }));
            }
        }
    }

    let successful_count = files.iter().filter(|f| f.get("error").is_none()).count();
    let failed_count = files.len().saturating_sub(successful_count);
    let truncated_files = files
        .iter()
        .filter(|f| {
            f.get("truncated")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        })
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
    eprintln!(
        "[TOOLS][read_many_files] matched={} returned={} failed={} truncated={} elapsed_ms={}",
        matched_count, successful_count, failed_count, truncated_files, elapsed_ms
    );
    record_tool_metric("read_many_files", elapsed_ms, successful_count > 0);

    let mut result = serde_json::json!({
        "files": files,
        "summary": {
            "matched": matched_count,
            "returned": successful_count,
            "truncated_files": truncated_files,
            "skipped": skipped,
            "failed": failed_count,
            "elapsed_ms": elapsed_ms,
        }
    });
    if let Some(summary) = result.get_mut("summary").and_then(|v| v.as_object_mut()) {
        summary.insert("metrics".to_string(), metric_snapshot("read_many_files"));
    }

    ToolResult::ok(serde_json::to_string_pretty(&result).unwrap_or_default())
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

    let fail_fast = get_bool_arg(args, &["fail_fast"], false);
    let ordered = get_bool_arg(args, &["ordered"], true);
    let started_at = Instant::now();
    let mut results = Vec::with_capacity(calls_array.len());

    for (index, call) in calls_array.iter().enumerate() {
        let Some(obj) = call.as_object() else {
            results.push(serde_json::json!({
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
            results.push(serde_json::json!({
                "index": index,
                "tool": tool_name,
                "ok": false,
                "error": "tool is not allowed in batch (read-only allowlist enforced)",
                "elapsed_ms": 0
            }));
            if fail_fast {
                break;
            }
            continue;
        }

        let arguments = obj
            .get("arguments")
            .cloned()
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        if !arguments.is_object() && !arguments.is_null() {
            results.push(serde_json::json!({
                "index": index,
                "tool": tool_name,
                "ok": false,
                "error": "arguments must be a JSON object",
                "elapsed_ms": 0
            }));
            if fail_fast {
                break;
            }
            continue;
        }

        let args_json = match serde_json::to_string(&arguments) {
            Ok(s) => s,
            Err(e) => {
                results.push(serde_json::json!({
                    "index": index,
                    "tool": tool_name,
                    "ok": false,
                    "error": format!("failed to serialize arguments: {}", e),
                    "elapsed_ms": 0
                }));
                if fail_fast {
                    break;
                }
                continue;
            }
        };

        let tool_started = Instant::now();
        let result = execute_tool_with_editor::<tauri::Wry>(
            workspace_root,
            &tool_name,
            &args_json,
            editor_state,
            None,
        );
        let elapsed_ms = tool_started.elapsed().as_millis() as u64;
        let ok = result.success;
        let entry = if ok {
            serde_json::json!({
                "index": index,
                "tool": tool_name,
                "ok": true,
                "output": result.to_tool_content(),
                "elapsed_ms": elapsed_ms,
            })
        } else {
            let raw_error = result
                .error
                .clone()
                .unwrap_or_else(|| "unknown error".to_string());
            let feedback = if result.skipped {
                String::new()
            } else {
                build_tool_failure_feedback(&tool_name, &raw_error)
            };
            serde_json::json!({
                "index": index,
                "tool": tool_name,
                "ok": false,
                "error": raw_error,
                "feedback": feedback,
                "skipped": result.skipped,
                "elapsed_ms": elapsed_ms,
            })
        };
        results.push(entry);
        if fail_fast && !ok {
            break;
        }
    }

    if ordered {
        results.sort_by_key(|value| {
            value
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(u64::MAX)
        });
    }

    let succeeded = results
        .iter()
        .filter(|value| value.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
        .count();
    let failed = results.len().saturating_sub(succeeded);
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    record_tool_metric("batch", elapsed_ms, failed == 0);

    let mut output = serde_json::json!({
        "results": results,
        "meta": {
            "total": calls_array.len(),
            "succeeded": succeeded,
            "failed": failed,
            "elapsed_ms": elapsed_ms,
            "cancelled": false,
            "max_parallel": 1,
            "fail_fast": fail_fast,
            "ordered": ordered,
        }
    });
    if let Some(meta) = output.get_mut("meta").and_then(|v| v.as_object_mut()) {
        meta.insert("metrics".to_string(), metric_snapshot("batch"));
    }

    ToolResult::ok(serde_json::to_string_pretty(&output).unwrap_or_default())
}

fn extract_objective_keywords(objective: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "where", "what", "that", "from", "into", "when", "how",
        "find", "show", "this", "these", "those", "code", "repo", "project",
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

fn codebase_investigator(
    _workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
) -> ToolResult {
    let Some(objective) = get_str_arg(args, &["objective", "query", "task"]) else {
        return ToolResult::err("codebase_investigator requires 'objective'");
    };

    let started = Instant::now();
    let keywords = extract_objective_keywords(&objective);
    let output_format = get_str_arg(args, &["output_format"]).unwrap_or_else(|| "json".to_string());
    let elapsed_ms = started.elapsed().as_millis() as u64;
    record_tool_metric("codebase_investigator", elapsed_ms, true);

    let report = serde_json::json!({
        "objective": objective,
        "findings": [],
        "recommended_changes": [],
        "confidence": 0.0,
        "meta": {
            "keywords": keywords,
            "elapsed_ms": elapsed_ms,
            "metrics": metric_snapshot("codebase_investigator")
        }
    });

    if output_format.eq_ignore_ascii_case("markdown") {
        return ToolResult::ok(
            "# Codebase Investigator Report\n\n## Findings\n- No findings generated.\n",
        );
    }

    ToolResult::ok(serde_json::to_string_pretty(&report).unwrap_or_default())
}

fn get_project_index_overview<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
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

    if let Ok(service) = language_service_from_app_handle(app_handle) {
        let scope_root = if root == workspace_root {
            None
        } else {
            Some(root.as_path())
        };
        match service.build_semantic_project_overview(scope_root, 8, 6) {
            Ok(Some(content)) => {
                let (window, end, total_chars, has_more) =
                    slice_by_char_window(&content, offset, max_chars);
                let returned_chars = window.chars().count();
                let result = serde_json::json!({
                    "tool": "get_project_index_overview",
                    "workspace_root": root.display().to_string(),
                    "index_path": serde_json::Value::Null,
                    "total_chars": total_chars,
                    "offset": offset,
                    "end": end,
                    "returned_chars": returned_chars,
                    "max_chars": max_chars,
                    "has_more": has_more,
                    "next_offset": if has_more { Some(end) } else { None },
                    "content": window,
                    "source": "semantic_index",
                });
                return ToolResult::ok(serde_json::to_string_pretty(&result).unwrap_or_default());
            }
            Ok(None) => {}
            Err(_) => {}
        }
    }

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
        return Err(format!("project root does not exist: {}", root.display()));
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

fn slice_by_char_window(
    content: &str,
    offset: usize,
    max_chars: usize,
) -> (String, usize, usize, bool) {
    let total_chars = content.chars().count();
    if offset >= total_chars {
        return (String::new(), total_chars, total_chars, false);
    }
    let window: String = content.chars().skip(offset).take(max_chars).collect();
    let end = offset + window.chars().count();
    let has_more = end < total_chars;
    (window, end, total_chars, has_more)
}

fn fast_context_tool(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    editor_state: Option<&EditorState>,
) -> ToolResult {
    let Some(query) = get_str_arg(args, &["query", "task", "request"]) else {
        return ToolResult::err("fast_context requires 'query'");
    };
    let active_file = get_str_arg(args, &["active_file", "activePath"])
        .or_else(|| editor_state.and_then(|state| state.active_file.clone()));
    let open_files = get_string_array_arg(args, &["open_files", "openFiles"])
        .or_else(|| editor_state.map(|state| state.open_files.clone()))
        .unwrap_or_default();
    let request = crate::context_pack::ContextPackRequest {
        id: get_str_arg(args, &["id"]).unwrap_or_else(|| "tool-fast-context".to_string()),
        query,
        queries: get_string_array_arg(args, &["queries"]).unwrap_or_default(),
        intent: get_str_arg(args, &["intent"]),
        max_results: args
            .get("max_results")
            .or_else(|| args.get("limit"))
            .and_then(|value| value.as_u64())
            .map(|value| value as usize),
        include_tests: args.get("include_tests").and_then(|value| value.as_bool()),
        include_docs: args.get("include_docs").and_then(|value| value.as_bool()),
        include_memory: args.get("include_memory").and_then(|value| value.as_bool()),
        include_project_index_min: args
            .get("include_project_index_min")
            .and_then(|value| value.as_bool()),
    };
    let payload = crate::context_pack::build_context_pack(
        workspace_root,
        active_file.as_deref(),
        &open_files,
        &request,
    );
    let mut payload = serde_json::to_value(payload).unwrap_or(serde_json::Value::Null);
    payload["_meta"] = serde_json::json!({
        "tool": "fast_context",
        "language_support": language_support_meta_json(active_file.as_deref()),
        "index_schema_summary": compact_index_schema_summary_for_workspace(workspace_root),
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn compact_index_schema_summary_for_workspace(workspace_root: &Path) -> serde_json::Value {
    let Ok(service) = crate::context_pack::language_service_for_workspace(workspace_root) else {
        return serde_json::Value::Null;
    };
    let Ok(schema) = service.index_schema_snapshot() else {
        return serde_json::Value::Null;
    };

    serde_json::json!({
        "totals": schema.totals,
        "files_by_support_level": schema.files_by_support_level,
        "files_by_language": schema.files_by_language.into_iter().take(8).collect::<Vec<_>>(),
        "symbols_by_type": schema.symbols_by_type.into_iter().take(8).collect::<Vec<_>>(),
        "relationships": {
            "total_relationships": schema.relationships.total_relationships,
            "resolved_relationships": schema.relationships.resolved_relationships,
            "unresolved_symbol_relationships": schema.relationships.unresolved_symbol_relationships,
            "missing_source_symbols": schema.relationships.missing_source_symbols,
            "missing_target_symbols": schema.relationships.missing_target_symbols,
        }
    })
}

fn get_editor_state(editor_state: Option<&EditorState>) -> ToolResult {
    let Some(state) = editor_state else {
        return ToolResult::err("editor state not available");
    };
    let payload = serde_json::json!({
        "active_file": state.active_file,
        "open_files": state.open_files,
        "active_tab_index": state.active_tab_index,
        "cursor_line": state.cursor_line,
        "cursor_column": state.cursor_column,
        "selection_start_line": state.selection_start_line,
        "selection_end_line": state.selection_end_line,
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn read_file_range(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path", "file_path", "filepath", "filename"]) else {
        return ToolResult::err("missing required arg: path (or file_path)");
    };

    let abs = match validate_read_path(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    let content = match fs::read_to_string(&abs) {
        Ok(s) => s,
        Err(e) => return ToolResult::err(e.to_string()),
    };

    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let start_line = args.get("start_line").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    let end_line = args
        .get("end_line")
        .and_then(|v| v.as_u64())
        .unwrap_or(total_lines as u64) as usize;
    let context_lines = args
        .get("context_lines")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    let requested_start = start_line.saturating_sub(1).saturating_sub(context_lines);
    let requested_end = end_line.saturating_add(context_lines).min(total_lines);
    let start = requested_start.min(total_lines);
    let end = requested_end.max(start).min(total_lines);

    let selected_lines: Vec<String> = lines[start..end]
        .iter()
        .enumerate()
        .map(|(idx, line)| format!("{}: {}", start + idx + 1, line))
        .collect();

    ToolResult::ok(format!(
        "File: {}\nLines {}-{} (of {}):\n{}\n",
        path,
        start + 1,
        end,
        total_lines,
        selected_lines.join("\n")
    ))
}

fn write_file<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let Some(path) = get_str_arg(
        args,
        &[
            "path",
            "file_path",
            "filepath",
            "filename",
            "TargetFile",
            "target_file",
        ],
    ) else {
        return ToolResult::err("missing required arg: path (or file_path)");
    };
    let Some(content) = get_str_arg(
        args,
        &["content", "contents", "text", "data", "CodeContent"],
    ) else {
        return ToolResult::err("missing required arg: content (or contents/text)");
    };

    let abs = match resolve_path_in_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };
    let original_content = fs::read_to_string(&abs).unwrap_or_default();
    let change_id = get_str_arg(args, &["id", "change_id", "tool_call_id"])
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let tracking = prepare_tool_write_tracking(app_handle, &change_id, &abs, &original_content);

    if let Some(parent) = abs.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return ToolResult::err(format!("cannot create parent directory: {}", e));
        }
    }

    match fs::write(&abs, content.as_bytes()) {
        Ok(()) => {
            track_tool_write(app_handle, &abs, tracking);
            sync_after_tool_write(app_handle, &change_id, &abs, &path);
            ToolResult::ok(format!(
                "wrote {} bytes to {}",
                content.len(),
                abs.display()
            ))
        }
        Err(e) => ToolResult::err(format!("write failed: {}", e)),
    }
}

fn edit_file<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let Some(path) = get_str_arg(
        args,
        &[
            "path",
            "file_path",
            "filepath",
            "filename",
            "TargetFile",
            "target_file",
        ],
    ) else {
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
        Ok(content) => content,
        Err(e) => return ToolResult::err(e.to_string()),
    };

    let updated = match apply_patch_to_string(&content, &old_content, &new_content) {
        Ok(updated) => updated,
        Err(e) => return ToolResult::err(e),
    };
    let change_id = get_str_arg(args, &["id", "change_id", "tool_call_id"])
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let tracking = prepare_tool_write_tracking(app_handle, &change_id, &abs, &content);

    match fs::write(&abs, updated.as_bytes()) {
        Ok(()) => {
            track_tool_write(app_handle, &abs, tracking);
            sync_after_tool_write(app_handle, &change_id, &abs, &path);
            ToolResult::ok(format!("Edited {}", path))
        }
        Err(e) => ToolResult::err(format!("write failed: {}", e)),
    }
}

fn parse_grep_timeout_ms(args: &HashMap<String, serde_json::Value>) -> u64 {
    args.get("timeout_ms")
        .and_then(|v| v.as_u64())
        .map(|value| value.clamp(GREP_TIMEOUT_MIN_MS, GREP_TIMEOUT_MAX_MS))
        .unwrap_or(GREP_TIMEOUT_DEFAULT_MS)
}

fn parse_grep_max_results(args: &HashMap<String, serde_json::Value>) -> usize {
    let max_results = get_bounded_usize_arg(
        args,
        &["max_results", "limit"],
        GREP_SEARCH_DEFAULT_MAX_RESULTS,
        GREP_SEARCH_MAX_RESULTS_CAP,
    );
    if max_results == 0 {
        GREP_SEARCH_DEFAULT_MAX_RESULTS
    } else {
        max_results
    }
}

fn build_grep_next_step_hint(include_dependencies: bool) -> String {
    if include_dependencies {
        "Narrow the search path, refine the pattern, or lower dependency scope to reduce grep timeout risk.".to_string()
    } else {
        "Narrow the search path or refine the pattern if grep results are too broad.".to_string()
    }
}

fn grep_search(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    _grep_timeout_enforced: bool,
) -> ToolResult {
    let Some(pattern) = get_str_arg(args, &["pattern", "query"]) else {
        return ToolResult::err("grep_search requires 'pattern'");
    };
    let path = get_str_arg(args, &["path"]).unwrap_or_else(|| ".".to_string());
    let include_dependencies = get_bool_arg(args, &["include_dependencies"], false);
    let timeout_ms = parse_grep_timeout_ms(args);
    let max_results = parse_grep_max_results(args);
    let force_timeout = get_bool_arg(args, &["__test_force_timeout"], false);

    let abs = match resolve_path_in_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    let regex = match Regex::new(&pattern) {
        Ok(regex) => regex,
        Err(e) => return ToolResult::err(format!("invalid regex pattern: {}", e)),
    };
    let gitignore_filter = create_gitignore_filter(workspace_root);
    let started_at = Instant::now();
    let deadline = if force_timeout {
        Some(Instant::now())
    } else {
        Some(Instant::now() + Duration::from_millis(timeout_ms))
    };
    let mut partial_results = Vec::new();
    let mut out = String::new();
    let mut result_count = 0usize;
    let mut timed_out = false;

    'file_loop: for entry in WalkDir::new(&abs)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let entry_path = entry.path();
        if !include_dependencies
            && entry_path.components().any(|component| {
                let part = component.as_os_str().to_string_lossy();
                DEPENDENCY_DIRS.contains(&part.as_ref())
            })
        {
            continue;
        }
        if let Some(filter) = &gitignore_filter {
            if filter.should_ignore(entry_path) {
                continue;
            }
        }

        let Ok(text) = fs::read_to_string(entry_path) else {
            continue;
        };

        for (idx, line) in text.lines().enumerate() {
            if let Some(limit) = deadline {
                if Instant::now() >= limit {
                    timed_out = true;
                    break 'file_loop;
                }
            }
            if regex.is_match(line) {
                let hit = format!(
                    "{}:{}:{}",
                    entry_path
                        .strip_prefix(workspace_root)
                        .unwrap_or(entry_path)
                        .display(),
                    idx + 1,
                    line
                );
                out.push_str(&hit);
                out.push('\n');
                partial_results.push(hit);
                result_count += 1;
                if result_count >= max_results {
                    break 'file_loop;
                }
                if force_timeout {
                    timed_out = true;
                    break 'file_loop;
                }
            }
        }
    }

    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    record_grep_search_metric(elapsed_ms, timed_out, result_count);

    if timed_out {
        let payload = serde_json::json!({
            "timed_out": true,
            "timeout_ms": timeout_ms,
            "partial_results": partial_results,
            "result_count": result_count,
            "searched_path": path,
            "next_step_hint": build_grep_next_step_hint(include_dependencies),
            "metrics": grep_search_metric_snapshot(),
        });
        return ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default());
    }

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
    let re = match Regex::new(&query) {
        Ok(r) => r,
        Err(e) => return ToolResult::err(format!("invalid regex pattern: {}", e)),
    };
    let gitignore_filter = create_gitignore_filter(workspace_root);
    let mut results = Vec::new();
    let mut count = 0usize;

    for entry in WalkDir::new(&abs)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if let Some(filter) = &gitignore_filter {
            if filter.should_ignore(path) {
                continue;
            }
        }
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if let Some(pattern) = &file_pattern {
            let patterns: Vec<&str> = pattern.split(',').collect();
            let matches_pattern = patterns.iter().any(|p| {
                let p = p.trim();
                if p.starts_with("*.") || p.starts_with("*") {
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

    ToolResult::ok(format!(
        "Found {} matches for '{}' (showing up to {}):\n{}",
        count,
        query,
        max_results,
        results.join("\n")
    ))
}

fn language_service_from_app_handle<R: tauri::Runtime>(
    app_handle: Option<&tauri::AppHandle<R>>,
) -> Result<std::sync::Arc<crate::language_service::LanguageService>, String> {
    let Some(app_handle) = app_handle else {
        return Err("language service unavailable: missing app handle".to_string());
    };
    use tauri::Manager;
    app_handle
        .state::<crate::app_state::AppState>()
        .language_service()
}

fn list_directory(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let path = get_str_arg(args, &["path"]).unwrap_or_else(|| ".".to_string());
    let abs = match resolve_path_in_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };
    if !abs.exists() {
        return ToolResult::err(format!("path does not exist: {}", abs.display()));
    }
    if !abs.is_dir() {
        return ToolResult::err(format!("path is not a directory: {}", abs.display()));
    }

    let mut entries = Vec::new();
    let read_dir = match fs::read_dir(&abs) {
        Ok(iter) => iter,
        Err(e) => return ToolResult::err(format!("failed to read directory: {}", e)),
    };

    for entry in read_dir.filter_map(Result::ok) {
        let entry_path = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        let rel = entry_path
            .strip_prefix(workspace_root)
            .unwrap_or(&entry_path)
            .to_string_lossy()
            .to_string();
        entries.push(serde_json::json!({
            "path": rel,
            "name": entry.file_name().to_string_lossy().to_string(),
            "is_dir": metadata.is_dir(),
            "size": if metadata.is_file() { Some(metadata.len()) } else { None::<u64> },
        }));
    }
    entries.sort_by(|a, b| {
        let a_name = a.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let b_name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
        a_name.cmp(b_name)
    });

    ToolResult::ok(
        serde_json::to_string_pretty(&serde_json::json!({
            "path": path,
            "entries": entries,
        }))
        .unwrap_or_default(),
    )
}

fn symbol_path_arg(workspace_root: &Path, path: &str) -> Result<String, String> {
    let resolved = resolve_path_in_workspace(workspace_root, Path::new(path))?;
    let workspace = fs::canonicalize(workspace_root).map_err(|e| e.to_string())?;
    let relative = resolved
        .strip_prefix(&workspace)
        .map_err(|e| e.to_string())?;
    Ok(normalize_rel_path(relative))
}

fn symbol_to_json(symbol: &crate::tree_sitter::Symbol) -> serde_json::Value {
    serde_json::json!({
        "id": symbol.id,
        "name": symbol.name,
        "qualified_name": symbol.qualified_name,
        "symbol_type": symbol.symbol_type.to_string(),
        "file_path": symbol.file_path,
        "range": {
            "start": {
                "line": symbol.range.start.line,
                "character": symbol.range.start.character,
            },
            "end": {
                "line": symbol.range.end.line,
                "character": symbol.range.end.character,
            }
        },
        "byte_offset": symbol.byte_offset,
        "byte_length": symbol.byte_length,
        "parent_id": symbol.parent_id,
        "docstring": symbol.docstring,
        "signature": symbol.signature,
        "content_hash": symbol.content_hash,
    })
}

fn support_level_name(level: crate::tree_sitter::SupportLevel) -> &'static str {
    match level {
        crate::tree_sitter::SupportLevel::Full => "full",
        crate::tree_sitter::SupportLevel::Partial => "partial",
        crate::tree_sitter::SupportLevel::AnchorOnly => "anchor_only",
    }
}

fn parser_kind_name(parser: crate::tree_sitter::ParserKind) -> &'static str {
    match parser {
        crate::tree_sitter::ParserKind::TreeSitter(_) => "tree_sitter",
        crate::tree_sitter::ParserKind::Projection { .. } => "projection",
        crate::tree_sitter::ParserKind::Scanner => "scanner",
        crate::tree_sitter::ParserKind::MarkdownHeadings => "markdown_headings",
        crate::tree_sitter::ParserKind::AnchorOnly => "anchor_only",
    }
}

fn language_capability_json(
    capability: &crate::tree_sitter::LanguageCapability,
) -> serde_json::Value {
    serde_json::json!({
        "language": format!("{:?}", capability.language).to_lowercase(),
        "display_name": capability.display_name,
        "extensions": capability.extensions,
        "support_level": support_level_name(capability.support),
        "parser": parser_kind_name(capability.parser),
        "extractor_version": capability.extractor_version,
        "extracts": {
            "definitions": capability.extracts.definitions,
            "imports": capability.extracts.imports,
            "relationships": capability.extracts.relationships,
            "semantic_anchors": capability.extracts.semantic_anchors,
            "markdown_headings": capability.extracts.markdown_headings,
        }
    })
}

fn supported_symbol_language_capabilities_json() -> Vec<serde_json::Value> {
    crate::tree_sitter::Language::all_capabilities()
        .iter()
        .map(language_capability_json)
        .collect()
}

fn supported_symbol_extensions() -> Vec<&'static str> {
    crate::tree_sitter::Language::all_capabilities()
        .iter()
        .flat_map(|capability| capability.extensions.iter().copied())
        .collect()
}

fn language_support_for_path_json(path: &str) -> serde_json::Value {
    match crate::tree_sitter::Language::capability_for_path(path) {
        Some(capability) => {
            let mut value = language_capability_json(capability);
            value["supported"] = serde_json::json!(true);
            value
        }
        None => serde_json::json!({
            "supported": false,
            "language": null,
            "display_name": null,
            "extensions": [],
            "support_level": "unsupported",
            "parser": null,
            "extractor_version": null,
            "extracts": {
                "definitions": false,
                "imports": false,
                "relationships": false,
                "semantic_anchors": false,
                "markdown_headings": false,
            }
        }),
    }
}

fn language_support_meta_json(path: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "file": path
            .map(language_support_for_path_json)
            .unwrap_or(serde_json::Value::Null),
        "supported_languages": supported_symbol_language_capabilities_json(),
    })
}

fn symbol_language_diagnostics(path: Option<&str>, result_count: usize) -> Vec<String> {
    let supported_extensions = supported_symbol_extensions().join(", ");
    let mut diagnostics = Vec::new();

    if let Some(path) = path {
        match crate::tree_sitter::Language::capability_for_path(path) {
            Some(capability) if capability.support != crate::tree_sitter::SupportLevel::Full => {
                diagnostics.push(format!(
                    "{} has {} symbol support via {}. Empty or sparse symbol results may reflect partial extraction rather than absence in source.",
                    path,
                    support_level_name(capability.support),
                    parser_kind_name(capability.parser),
                ));
            }
            Some(_) => {}
            None => diagnostics.push(format!(
                "{} is not supported by the Symbols Index. Supported structural extensions: {}. Use grep_search or codebase_search for arbitrary text, and semantic_anchor_search for routes, config keys, translation keys, CSS/theme tokens, and other literals.",
                path, supported_extensions
            )),
        }
    }

    if result_count == 0 {
        diagnostics.push(
            "No indexed symbols matched. Confirm the target file type is supported and the index is fresh; for unsupported files or literal tokens, use semantic_anchor_search, grep_search, or codebase_search.".to_string(),
        );
    }

    diagnostics
}

fn symbol_outline_diagnostics(path: &str, total_symbols: usize) -> Vec<String> {
    let mut diagnostics = symbol_language_diagnostics(Some(path), usize::MAX);
    if total_symbols == 0 && crate::tree_sitter::Language::capability_for_path(path).is_some() {
        diagnostics.push(
            "No indexed symbols are available for this file. For partial languages this may be expected; otherwise confirm the index is fresh or fall back to read_file_range.".to_string(),
        );
    }
    diagnostics
}

fn paginate_tool_results<T>(items: Vec<T>, offset: usize, limit: usize) -> (Vec<T>, bool, usize) {
    let total_available = items.len();
    let has_more = total_available > offset.saturating_add(limit);
    let page = items.into_iter().skip(offset).take(limit).collect();
    (page, has_more, total_available)
}

fn truncate_chars(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    value.chars().take(max_chars).collect::<String>()
}

fn symbol_inventory_entry_to_json(
    symbol: &crate::tree_sitter::Symbol,
    child_count: usize,
    include_docstrings: bool,
) -> serde_json::Value {
    let mut value = serde_json::json!({
        "id": symbol.id,
        "name": symbol.name,
        "qualified_name": symbol.qualified_name,
        "symbol_type": symbol.symbol_type.to_string(),
        "signature": symbol
            .signature
            .as_ref()
            .map(|signature| truncate_chars(signature, SYMBOL_TEXT_PREVIEW_CHARS)),
        "line_range": {
            "start_line": symbol.range.start.line,
            "end_line": symbol.range.end.line,
        },
        "range": {
            "start": {
                "line": symbol.range.start.line,
                "character": symbol.range.start.character,
            },
            "end": {
                "line": symbol.range.end.line,
                "character": symbol.range.end.character,
            }
        },
        "parent_id": symbol.parent_id,
        "child_count": child_count,
    });

    if include_docstrings {
        value["docstring"] = serde_json::json!(symbol
            .docstring
            .as_ref()
            .map(|docstring| truncate_chars(docstring, SYMBOL_TEXT_PREVIEW_CHARS)));
    }

    value
}

fn symbol_inventory_summary(symbols: &[crate::tree_sitter::Symbol]) -> serde_json::Value {
    let mut by_type = BTreeMap::<String, usize>::new();
    let mut top_level_symbols = 0usize;
    let mut symbols_with_children = 0usize;
    let mut child_counts = HashMap::<String, usize>::new();

    for symbol in symbols {
        *by_type.entry(symbol.symbol_type.to_string()).or_default() += 1;
        if symbol.parent_id.is_none() {
            top_level_symbols += 1;
        }
        if let Some(parent_id) = symbol.parent_id.as_ref() {
            *child_counts.entry(parent_id.clone()).or_default() += 1;
        }
    }

    for symbol in symbols {
        if child_counts.get(&symbol.id).copied().unwrap_or_default() > 0 {
            symbols_with_children += 1;
        }
    }

    serde_json::json!({
        "total_symbols": symbols.len(),
        "top_level_symbols": top_level_symbols,
        "symbols_with_children": symbols_with_children,
        "by_type": by_type,
    })
}

fn symbol_inventory_entries(
    symbols: &[crate::tree_sitter::Symbol],
    max_symbols: usize,
    include_docstrings: bool,
) -> Vec<serde_json::Value> {
    let mut child_counts = HashMap::<String, usize>::new();
    for symbol in symbols {
        if let Some(parent_id) = symbol.parent_id.as_ref() {
            *child_counts.entry(parent_id.clone()).or_default() += 1;
        }
    }

    let mut sorted = symbols.to_vec();
    sorted.sort_by_key(|symbol| (symbol.range.start.line, symbol.range.start.character));
    sorted
        .into_iter()
        .take(max_symbols)
        .map(|symbol| {
            let child_count = child_counts.get(&symbol.id).copied().unwrap_or_default();
            symbol_inventory_entry_to_json(&symbol, child_count, include_docstrings)
        })
        .collect()
}

fn semantic_anchor_result_to_json(
    result: &crate::symbol_index::SemanticAnchorResult,
) -> serde_json::Value {
    serde_json::json!({
        "id": result.anchor.id,
        "file_path": result.anchor.file_path,
        "kind": result.anchor.kind,
        "value": result.anchor.value,
        "line": result.anchor.line,
        "character": result.anchor.character,
        "preview": result.anchor.preview,
        "confidence": result.anchor.confidence,
        "score": result.score,
    })
}

fn symbol_reference_resolution_json(
    reference: &crate::symbol_index::SymbolReference,
) -> serde_json::Value {
    let strategy = if reference.target_symbol_id.is_some() {
        "resolved_symbol_id"
    } else if reference.target_symbol.is_some() {
        "hydrated_target_symbol"
    } else if reference.relationship_type == crate::tree_sitter::SymbolRelationshipType::Import {
        "resolved_file_path"
    } else {
        "name_fallback"
    };
    let confidence = match strategy {
        "resolved_symbol_id" | "hydrated_target_symbol" => "high",
        "resolved_file_path" => "medium",
        _ => "low",
    };

    serde_json::json!({
        "strategy": strategy,
        "confidence": confidence,
        "resolved": reference.target_symbol_id.is_some()
            || reference.target_symbol.is_some()
            || reference.relationship_type == crate::tree_sitter::SymbolRelationshipType::Import,
    })
}

fn symbol_reference_to_json(reference: &crate::symbol_index::SymbolReference) -> serde_json::Value {
    serde_json::json!({
        "source_symbol": symbol_to_json(&reference.source_symbol),
        "relationship_type": reference.relationship_type.to_string(),
        "target_name": reference.target_name,
        "target_symbol_id": reference.target_symbol_id,
        "target_symbol": reference.target_symbol.as_ref().map(symbol_to_json),
        "line": reference.line,
        "resolution": symbol_reference_resolution_json(reference),
    })
}

fn symbol_trace_node_to_json(node: &crate::language_service::SymbolTraceNode) -> serde_json::Value {
    serde_json::json!({
        "symbol": symbol_to_json(&node.symbol),
        "depth": node.depth,
    })
}

fn symbol_trace_edge_to_json(edge: &crate::language_service::SymbolTraceEdge) -> serde_json::Value {
    serde_json::json!({
        "source_symbol": symbol_to_json(&edge.source_symbol),
        "target_symbol": edge.target_symbol.as_ref().map(symbol_to_json),
        "target_name": edge.target_name,
        "relationship_type": edge.relationship_type.to_string(),
        "direction": edge.direction.as_str(),
        "depth": edge.depth,
        "line": edge.line,
        "resolved": edge.resolved,
    })
}

fn related_symbol_to_json(related: &crate::language_service::RelatedSymbol) -> serde_json::Value {
    serde_json::json!({
        "symbol": symbol_to_json(&related.symbol),
        "relationship": related.relationship,
        "reason": related.reason,
        "score": related.score,
        "distance": related.distance,
    })
}

fn relationship_type_values() -> Vec<crate::tree_sitter::SymbolRelationshipType> {
    vec![
        crate::tree_sitter::SymbolRelationshipType::Call,
        crate::tree_sitter::SymbolRelationshipType::Import,
        crate::tree_sitter::SymbolRelationshipType::Export,
        crate::tree_sitter::SymbolRelationshipType::Extends,
        crate::tree_sitter::SymbolRelationshipType::Implements,
        crate::tree_sitter::SymbolRelationshipType::Usage,
    ]
}

fn parse_relationship_types_arg(
    args: &HashMap<String, serde_json::Value>,
) -> Result<Vec<crate::tree_sitter::SymbolRelationshipType>, String> {
    if let Some(kind) = get_str_arg(args, &["relationship", "relationship_type", "kind"]) {
        return Ok(vec![kind.parse()?]);
    }

    let values = get_string_array_arg(args, &["relationships", "relationship_types", "kinds"]);
    let Some(values) = values else {
        return Ok(relationship_type_values());
    };

    let mut parsed = Vec::new();
    for value in values {
        parsed.push(value.parse()?);
    }
    if parsed.is_empty() {
        Ok(relationship_type_values())
    } else {
        Ok(parsed)
    }
}

fn parse_symbol_trace_direction_arg(
    args: &HashMap<String, serde_json::Value>,
) -> Result<crate::language_service::SymbolTraceDirection, String> {
    let Some(direction) = get_str_arg(args, &["direction"]) else {
        return Ok(crate::language_service::SymbolTraceDirection::Both);
    };

    match direction.to_ascii_lowercase().as_str() {
        "incoming" | "inbound" | "in" => {
            Ok(crate::language_service::SymbolTraceDirection::Incoming)
        }
        "outgoing" | "outbound" | "out" => {
            Ok(crate::language_service::SymbolTraceDirection::Outgoing)
        }
        "both" => Ok(crate::language_service::SymbolTraceDirection::Both),
        _ => Err(format!(
            "unknown symbol_trace direction '{}'; expected incoming, outgoing, or both",
            direction
        )),
    }
}

fn is_reference_expansion_symbol(symbol: &crate::tree_sitter::Symbol) -> bool {
    matches!(
        symbol.symbol_type,
        crate::tree_sitter::SymbolType::Function
            | crate::tree_sitter::SymbolType::Method
            | crate::tree_sitter::SymbolType::Class
            | crate::tree_sitter::SymbolType::Struct
            | crate::tree_sitter::SymbolType::Interface
            | crate::tree_sitter::SymbolType::Type
            | crate::tree_sitter::SymbolType::Enum
            | crate::tree_sitter::SymbolType::Trait
            | crate::tree_sitter::SymbolType::Impl
            | crate::tree_sitter::SymbolType::Module
    )
}

fn is_probable_test_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("/__tests__/")
        || lower.starts_with("test/")
        || lower.starts_with("tests/")
        || lower.starts_with("__tests__/")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.ends_with("_test.rs")
}

fn is_impact_skip_path(path: &str) -> bool {
    path.split('/').any(|part| {
        matches!(
            part,
            ".git" | ".zblade" | "node_modules" | "target" | "dist" | "build" | ".next"
        )
    })
}

fn related_test_files_for_paths(
    workspace_root: &Path,
    paths: &[String],
    limit: usize,
) -> Vec<serde_json::Value> {
    let mut tests = Vec::new();
    let mut seen = HashSet::new();
    let stems = paths
        .iter()
        .filter_map(|path| {
            Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| stem.to_ascii_lowercase())
        })
        .collect::<Vec<_>>();

    for entry in WalkDir::new(workspace_root)
        .follow_links(false)
        .max_depth(8)
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry
            .path()
            .strip_prefix(workspace_root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if is_impact_skip_path(&rel) || !is_probable_test_path(&rel) || !seen.insert(rel.clone()) {
            continue;
        }
        let lower = rel.to_ascii_lowercase();
        let matched_stem = stems.iter().find(|stem| lower.contains(stem.as_str()));
        if let Some(stem) = matched_stem {
            tests.push(serde_json::json!({
                "path": rel,
                "reason": format!("Test path matches impacted source stem `{}`", stem),
                "score": 82
            }));
            if tests.len() >= limit {
                break;
            }
        }
    }

    tests
}

fn impact_risk_level(
    impacted_file_count: usize,
    reference_count: usize,
    test_count: usize,
) -> String {
    if impacted_file_count >= 8 || reference_count >= 16 {
        "high".to_string()
    } else if impacted_file_count >= 3 || reference_count >= 4 || test_count == 0 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

fn impact_confidence(index_fresh: bool, symbol_count: usize, impacted_file_count: usize) -> String {
    if index_fresh && symbol_count > 0 && impacted_file_count > 0 {
        "high".to_string()
    } else if symbol_count > 0 || impacted_file_count > 0 {
        "medium".to_string()
    } else {
        "low".to_string()
    }
}

fn resolve_symbol_from_graph_args(
    workspace_root: &Path,
    service: &crate::language_service::LanguageService,
    args: &HashMap<String, serde_json::Value>,
) -> Result<Option<crate::tree_sitter::Symbol>, String> {
    if let Some(symbol_id) = get_str_arg(args, &["symbol_id", "id"]) {
        return service
            .get_symbol(&symbol_id)
            .map_err(|err| err.to_string());
    }

    let Some(file_path) = get_str_arg(args, &["path", "file", "file_path"]) else {
        return Err("symbol relationship tools require 'symbol_id' or 'path'".to_string());
    };
    let file_path = symbol_path_arg(workspace_root, &file_path)?;
    let qualified_name = get_str_arg(args, &["qualified_name"]);
    let name = get_str_arg(args, &["name"]);
    if qualified_name.is_none() && name.is_none() {
        return Err(
            "symbol relationship tools require 'name' or 'qualified_name' when resolving by path"
                .to_string(),
        );
    }

    let symbols = service
        .get_file_symbols(&file_path)
        .map_err(|err| err.to_string())?;
    Ok(symbols.into_iter().find(|symbol| {
        qualified_name
            .as_ref()
            .map(|value| &symbol.qualified_name == value)
            .unwrap_or(false)
            || name
                .as_ref()
                .map(|value| &symbol.name == value)
                .unwrap_or(false)
    }))
}

fn compact_outline_nodes_for_parent(
    by_parent: &HashMap<Option<String>, Vec<crate::tree_sitter::Symbol>>,
    parent_id: Option<&str>,
    max_nodes: usize,
    max_depth: usize,
    depth: usize,
    emitted_nodes: &mut usize,
) -> Vec<serde_json::Value> {
    if *emitted_nodes >= max_nodes || depth >= max_depth {
        return Vec::new();
    }

    let mut symbols = by_parent
        .get(&parent_id.map(|id| id.to_string()))
        .cloned()
        .unwrap_or_default();
    symbols.sort_by_key(|symbol| (symbol.range.start.line, symbol.range.start.character));

    let mut nodes = Vec::new();
    for symbol in symbols {
        if *emitted_nodes >= max_nodes {
            break;
        }

        *emitted_nodes += 1;
        let child_count = by_parent
            .get(&Some(symbol.id.clone()))
            .map(Vec::len)
            .unwrap_or_default();
        let children = compact_outline_nodes_for_parent(
            by_parent,
            Some(&symbol.id),
            max_nodes,
            max_depth,
            depth + 1,
            emitted_nodes,
        );
        let children_returned = children.len();
        nodes.push(serde_json::json!({
            "id": symbol.id,
            "name": symbol.name,
            "qualified_name": symbol.qualified_name,
            "symbol_type": symbol.symbol_type.to_string(),
            "line_range": {
                "start_line": symbol.range.start.line,
                "end_line": symbol.range.end.line,
            },
            "child_count": child_count,
            "children": children,
            "children_truncated": child_count > children_returned,
        }));
    }

    nodes
}

const SYMBOL_SEARCH_CONNECTED_RESULT_CAP: usize = 10;
const SYMBOL_SEARCH_CONNECTED_DIRECTION_CAP: usize = 3;

fn symbol_search_connected_preview(
    service: &crate::language_service::LanguageService,
    symbol: &crate::tree_sitter::Symbol,
) -> serde_json::Value {
    let mut incoming = Vec::new();
    let mut outgoing = Vec::new();
    let mut errors = Vec::new();
    let mut incoming_truncated = false;
    let mut outgoing_truncated = false;

    for relationship in relationship_type_values() {
        match service.get_symbol_graph(symbol, relationship, SYMBOL_SEARCH_CONNECTED_DIRECTION_CAP)
        {
            Ok(graph) => {
                for reference in graph.incoming {
                    if incoming.len() >= SYMBOL_SEARCH_CONNECTED_DIRECTION_CAP {
                        incoming_truncated = true;
                        break;
                    }
                    incoming.push(symbol_search_connection_json(&reference, "incoming"));
                }

                for reference in graph.outgoing {
                    if outgoing.len() >= SYMBOL_SEARCH_CONNECTED_DIRECTION_CAP {
                        outgoing_truncated = true;
                        break;
                    }
                    outgoing.push(symbol_search_connection_json(&reference, "outgoing"));
                }
            }
            Err(err) => errors.push(serde_json::json!({
                "relationship_type": relationship.to_string(),
                "error": err.to_string(),
            })),
        }

        if incoming.len() >= SYMBOL_SEARCH_CONNECTED_DIRECTION_CAP
            && outgoing.len() >= SYMBOL_SEARCH_CONNECTED_DIRECTION_CAP
        {
            incoming_truncated = true;
            outgoing_truncated = true;
            break;
        }
    }

    serde_json::json!({
        "incoming": incoming,
        "outgoing": outgoing,
        "incoming_count_returned": incoming.len(),
        "outgoing_count_returned": outgoing.len(),
        "truncated": incoming_truncated || outgoing_truncated,
        "errors": errors,
    })
}

fn symbol_search_connection_json(
    reference: &crate::symbol_index::SymbolReference,
    direction: &str,
) -> serde_json::Value {
    let connected_symbol = if direction == "incoming" {
        Some(&reference.source_symbol)
    } else {
        reference.target_symbol.as_ref()
    };

    serde_json::json!({
        "direction": direction,
        "relationship_type": reference.relationship_type.to_string(),
        "target_name": &reference.target_name,
        "target_symbol_id": &reference.target_symbol_id,
        "line": reference.line,
        "resolution": symbol_reference_resolution_json(reference),
        "symbol": connected_symbol.map(|symbol| serde_json::json!({
            "id": &symbol.id,
            "name": &symbol.name,
            "qualified_name": &symbol.qualified_name,
            "symbol_type": symbol.symbol_type.to_string(),
            "file_path": &symbol.file_path,
            "line": symbol.range.start.line,
        })),
    })
}

fn symbol_search_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let Some(query) = get_str_arg(args, &["query"]) else {
        return ToolResult::err("symbol_search requires 'query'");
    };
    let limit = parse_bounded_usize_arg(args, "limit", 20, 100);
    let offset = parse_bounded_usize_arg(args, "offset", 0, SYMBOL_SEARCH_MAX_OFFSET);
    let fetch_limit = offset.saturating_add(limit).saturating_add(1);
    let file_path = get_str_arg(args, &["path", "file", "file_path"]);
    let file_pattern = get_str_arg(args, &["file_pattern", "path_pattern"]);
    let name_pattern = get_str_arg(args, &["name_pattern"]);
    let qualified_name_pattern =
        get_str_arg(args, &["qualified_name_pattern", "qualified_pattern"]);
    let include_connected = get_bool_arg(args, &["include_connected"], false);
    let symbol_types = match get_str_arg(args, &["kind", "symbol_type"]) {
        Some(kind) => match kind.parse::<crate::tree_sitter::SymbolType>() {
            Ok(symbol_type) => Some(vec![symbol_type]),
            Err(_) => return ToolResult::err(format!("unknown symbol kind: {}", kind)),
        },
        None => None,
    };
    let file_filter = match file_path {
        Some(path) => match symbol_path_arg(workspace_root, &path) {
            Ok(path) => Some(path),
            Err(err) => return ToolResult::err(err),
        },
        None => None,
    };
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let started = Instant::now();
    let file_is_unsupported = file_filter
        .as_deref()
        .is_some_and(|path| crate::tree_sitter::Language::capability_for_path(path).is_none());
    let results = if file_is_unsupported {
        Vec::new()
    } else {
        match service.search_symbols_filtered_with_patterns(
            &query,
            file_filter.as_deref(),
            symbol_types,
            file_pattern.as_deref(),
            name_pattern.as_deref(),
            qualified_name_pattern.as_deref(),
            fetch_limit,
        ) {
            Ok(results) => results,
            Err(err) => return ToolResult::err(err.to_string()),
        }
    };
    let available_count = results.len();
    let initial_top_score = results.first().map(|result| result.score);
    let (results, has_more, total_available) = paginate_tool_results(results, offset, limit);
    let result_count = results.len();
    let total_lower_bound = offset
        .saturating_add(result_count)
        .saturating_add(usize::from(has_more));
    let diagnostic_result_count = if available_count > 0 { 1 } else { 0 };
    let search_diagnostics =
        symbol_language_diagnostics(file_filter.as_deref(), diagnostic_result_count);
    let search_health = serde_json::json!({
        "enabled": false,
        "triggered": false,
        "reason": null,
        "confidence": symbol_search_confidence(available_count, initial_top_score),
        "initial_result_count": available_count,
        "initial_top_score": initial_top_score,
        "reran_after_reindex": false,
        "reindexed_files": [],
        "removed_files": [],
        "literal_matches": [],
        "semantic_anchor_matches": [],
        "diagnostics": search_diagnostics,
        "health_before": null,
        "health_after": null,
    });
    let language_support = language_support_meta_json(file_filter.as_deref());
    let payload = serde_json::json!({
        "query": query,
        "results": results.iter().enumerate().map(|(index, result)| {
            let mut value = symbol_to_json(&result.symbol);
            value["score"] = serde_json::json!(result.score);
            if include_connected && index < SYMBOL_SEARCH_CONNECTED_RESULT_CAP {
                value["connected"] = symbol_search_connected_preview(&service, &result.symbol);
            } else if include_connected {
                value["connected"] = serde_json::Value::Null;
                value["connected_truncated"] = serde_json::json!(true);
            }
            value
        }).collect::<Vec<_>>(),
        "_meta": {
            "tool": "symbol_search",
            "count": result_count,
            "offset": offset,
            "limit": limit,
            "has_more": has_more,
            "total_known": false,
            "total_lower_bound": total_lower_bound,
            "candidate_count": total_available,
            "filters": {
                "path": file_filter.clone(),
                "file_pattern": file_pattern.clone(),
                "name_pattern": name_pattern.clone(),
                "qualified_name_pattern": qualified_name_pattern.clone(),
                "include_connected": include_connected,
                "connected_result_cap": if include_connected { Some(SYMBOL_SEARCH_CONNECTED_RESULT_CAP) } else { None::<usize> },
                "connected_direction_cap": if include_connected { Some(SYMBOL_SEARCH_CONNECTED_DIRECTION_CAP) } else { None::<usize> },
            },
            "timing_ms": started.elapsed().as_millis(),
            "source": "language_service",
            "index_health": service.index_health_snapshot(),
            "search_health": search_health,
            "language_support": language_support,
            "truncated": false
        }
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn symbol_search_confidence(result_count: usize, top_score: Option<f32>) -> &'static str {
    let top_score = top_score.unwrap_or(0.0);
    if result_count == 0 {
        "empty"
    } else if top_score >= 0.85 {
        "high"
    } else if top_score >= 0.55 {
        "medium"
    } else {
        "low"
    }
}

fn semantic_anchor_search_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let Some(query) = get_str_arg(args, &["query"]) else {
        return ToolResult::err("semantic_anchor_search requires 'query'");
    };
    let limit = parse_bounded_usize_arg(args, "limit", 20, 100);
    let file_path = get_str_arg(args, &["path", "file", "file_path"]);
    let file_filter = match file_path {
        Some(path) => match symbol_path_arg(workspace_root, &path) {
            Ok(path) => Some(path),
            Err(err) => return ToolResult::err(err),
        },
        None => None,
    };
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let started = Instant::now();
    let anchors = match service.search_semantic_anchors(&query, file_filter.as_deref(), limit) {
        Ok(anchors) => anchors,
        Err(err) => return ToolResult::err(err.to_string()),
    };
    let count = anchors.len();
    let payload = serde_json::json!({
        "query": query,
        "path": file_filter,
        "anchors": anchors.iter().map(semantic_anchor_result_to_json).collect::<Vec<_>>(),
        "_meta": {
            "tool": "semantic_anchor_search",
            "count": count,
            "timing_ms": started.elapsed().as_millis(),
            "source": "language_service",
            "index_health": service.index_health_snapshot(),
            "language_support": language_support_meta_json(file_filter.as_deref()),
            "truncated": false
        }
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn symbol_resolve_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let started = Instant::now();
    let resolved = if let Some(symbol_id) = get_str_arg(args, &["symbol_id", "id"]) {
        match service.get_symbol(&symbol_id) {
            Ok(Some(symbol)) => symbol,
            Ok(None) => return ToolResult::err(format!("symbol not found: {}", symbol_id)),
            Err(err) => return ToolResult::err(err.to_string()),
        }
    } else {
        let Some(file_path) = get_str_arg(args, &["path", "file", "file_path"]) else {
            return ToolResult::err("symbol_resolve requires 'symbol_id' or 'path'");
        };
        let file_path = match symbol_path_arg(workspace_root, &file_path) {
            Ok(path) => path,
            Err(err) => return ToolResult::err(err),
        };
        let qualified_name = get_str_arg(args, &["qualified_name"]);
        let name = get_str_arg(args, &["name"]);
        let symbols = match service.get_file_symbols(&file_path) {
            Ok(symbols) => symbols,
            Err(err) => return ToolResult::err(err.to_string()),
        };
        let Some(symbol) = symbols.into_iter().find(|symbol| {
            qualified_name
                .as_ref()
                .map(|value| &symbol.qualified_name == value)
                .unwrap_or(false)
                || name
                    .as_ref()
                    .map(|value| &symbol.name == value)
                    .unwrap_or(false)
        }) else {
            return ToolResult::err("symbol not found".to_string());
        };
        symbol
    };

    let mut payload = symbol_to_json(&resolved);
    payload["_meta"] = serde_json::json!({
        "tool": "symbol_resolve",
        "timing_ms": started.elapsed().as_millis(),
        "source": "language_service",
        "language_support": language_support_meta_json(Some(&resolved.file_path)),
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn symbol_outline_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path", "file", "file_path"]) else {
        return ToolResult::err("symbol_outline requires 'path'");
    };
    let path = match symbol_path_arg(workspace_root, &path) {
        Ok(path) => path,
        Err(err) => return ToolResult::err(err),
    };
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let started = Instant::now();
    if crate::tree_sitter::Language::capability_for_path(&path).is_none() {
        let payload = serde_json::json!({
            "path": path,
            "summary": {},
            "symbols": [],
            "outline": serde_json::Value::Null,
            "_meta": {
                "tool": "symbol_outline",
                "source": "language_service",
                "timing_ms": started.elapsed().as_millis(),
                "index_health": service.index_health_snapshot(),
                "language_support": language_support_meta_json(Some(&path)),
                "diagnostics": symbol_outline_diagnostics(&path, 0),
                "line_count": null,
                "total_symbols": 0,
                "returned_symbols": 0,
                "truncated": false,
                "symbols_truncated": false,
                "outline_nodes_returned": 0,
                "outline_truncated": false,
                "include_outline": false,
                "include_docstrings": false,
                "max_symbols": 0,
                "max_outline_nodes": 0,
                "max_outline_depth": 0,
                "guidance": "Use grep_search or codebase_search for unsupported file types. Use semantic_anchor_search for indexed literals such as routes, config keys, translation keys, and CSS/theme tokens."
            }
        });
        return ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default());
    }
    let include_outline = get_bool_arg(args, &["include_outline"], false);
    let include_docstrings = get_bool_arg(args, &["include_docstrings", "include_docs"], false);
    let max_symbols = get_bounded_usize_arg(
        args,
        &["max_symbols", "limit"],
        SYMBOL_OUTLINE_DEFAULT_MAX_SYMBOLS,
        SYMBOL_OUTLINE_MAX_SYMBOLS_CAP,
    );
    let max_outline_nodes = get_bounded_usize_arg(
        args,
        &["max_outline_nodes", "outline_limit"],
        SYMBOL_OUTLINE_DEFAULT_MAX_NODES,
        SYMBOL_OUTLINE_MAX_NODES_CAP,
    );
    let max_outline_depth = get_bounded_usize_arg(
        args,
        &["max_outline_depth", "outline_depth"],
        SYMBOL_OUTLINE_DEFAULT_MAX_DEPTH,
        SYMBOL_OUTLINE_MAX_DEPTH_CAP,
    );
    let symbols = match service.get_file_symbols(&path) {
        Ok(symbols) => symbols,
        Err(err) => return ToolResult::err(err.to_string()),
    };
    let indexed_file = match service.indexed_file_record(&path) {
        Ok(record) => record,
        Err(err) => return ToolResult::err(err.to_string()),
    };
    let total_symbols = symbols.len();
    let summary = symbol_inventory_summary(&symbols);
    let inventory_symbols = symbol_inventory_entries(&symbols, max_symbols, include_docstrings);

    let mut by_parent: HashMap<Option<String>, Vec<crate::tree_sitter::Symbol>> = HashMap::new();
    for symbol in symbols.iter().cloned() {
        by_parent
            .entry(symbol.parent_id.clone())
            .or_default()
            .push(symbol);
    }
    let mut outline_nodes_returned = 0usize;
    let outline = if include_outline {
        serde_json::Value::Array(compact_outline_nodes_for_parent(
            &by_parent,
            None,
            max_outline_nodes,
            max_outline_depth,
            0,
            &mut outline_nodes_returned,
        ))
    } else {
        serde_json::Value::Null
    };
    let outline_truncated = include_outline && outline_nodes_returned < total_symbols;
    let diagnostics = symbol_outline_diagnostics(&path, total_symbols);
    let payload = serde_json::json!({
        "path": path,
        "summary": summary,
        "symbols": inventory_symbols,
        "outline": outline,
        "_meta": {
            "tool": "symbol_outline",
            "source": "language_service",
            "timing_ms": started.elapsed().as_millis(),
            "index_health": service.index_health_snapshot(),
            "language_support": language_support_meta_json(Some(&path)),
            "diagnostics": diagnostics,
            "line_count": indexed_file.as_ref().and_then(|record| record.line_count),
            "total_symbols": total_symbols,
            "returned_symbols": total_symbols.min(max_symbols),
            "truncated": total_symbols > max_symbols || outline_truncated,
            "symbols_truncated": total_symbols > max_symbols,
            "outline_nodes_returned": outline_nodes_returned,
            "outline_truncated": outline_truncated,
            "include_outline": include_outline,
            "include_docstrings": include_docstrings,
            "max_symbols": max_symbols,
            "max_outline_nodes": max_outline_nodes,
            "max_outline_depth": max_outline_depth,
            "guidance": if total_symbols > max_symbols || outline_truncated {
                "Use symbol_search to narrow candidates and symbol_resolve for full details on a specific symbol."
            } else {
                "Use symbol_resolve for full details on a specific symbol."
            }
        }
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn symbol_related_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let symbol = match resolve_symbol_from_graph_args(workspace_root, &service, args) {
        Ok(Some(symbol)) => symbol,
        Ok(None) => return ToolResult::err("symbol not found".to_string()),
        Err(err) => return ToolResult::err(err),
    };
    let limit = parse_bounded_usize_arg(args, "limit", 24, 100);
    let started = Instant::now();
    let related = match service.get_related_symbols(&symbol, limit) {
        Ok(related) => related,
        Err(err) => return ToolResult::err(err.to_string()),
    };
    let payload = serde_json::json!({
        "seed": symbol_to_json(&symbol),
        "related": related.iter().map(related_symbol_to_json).collect::<Vec<_>>(),
        "_meta": {
            "tool": "symbol_related",
            "count": related.len(),
            "timing_ms": started.elapsed().as_millis(),
            "source": "language_service",
            "index_health": service.index_health_snapshot(),
            "language_support": language_support_meta_json(Some(&symbol.file_path)),
            "truncated": related.len() >= limit,
        }
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn symbol_references_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let started = Instant::now();
    let limit = parse_bounded_usize_arg(args, "limit", 20, 100);
    let max_symbols = get_bounded_usize_arg(args, &["max_symbols"], 24, 100);
    let relationships = match parse_relationship_types_arg(args) {
        Ok(values) => values,
        Err(err) => return ToolResult::err(err),
    };

    let target_symbols = if get_str_arg(args, &["symbol_id", "id"]).is_some()
        || get_str_arg(args, &["name", "qualified_name"]).is_some()
    {
        match resolve_symbol_from_graph_args(workspace_root, &service, args) {
            Ok(Some(symbol)) => vec![symbol],
            Ok(None) => return ToolResult::err("symbol not found".to_string()),
            Err(err) => return ToolResult::err(err),
        }
    } else {
        let Some(file_path) = get_str_arg(args, &["path", "file", "file_path"]) else {
            return ToolResult::err(
                "symbol_references requires 'symbol_id', 'name' with 'path', or 'path'".to_string(),
            );
        };
        let file_path = match symbol_path_arg(workspace_root, &file_path) {
            Ok(path) => path,
            Err(err) => return ToolResult::err(err),
        };
        let symbols = match service.get_file_symbols(&file_path) {
            Ok(symbols) => symbols,
            Err(err) => return ToolResult::err(err.to_string()),
        };
        symbols
            .into_iter()
            .filter(is_reference_expansion_symbol)
            .take(max_symbols)
            .collect::<Vec<_>>()
    };

    if target_symbols.is_empty() {
        return ToolResult::err("no expandable symbols found".to_string());
    }

    let mut total_incoming = 0usize;
    let mut total_outgoing = 0usize;
    let mut relationship_totals = BTreeMap::<String, usize>::new();
    let expansions = target_symbols
        .iter()
        .map(|symbol| {
            let mut incoming = BTreeMap::<String, Vec<serde_json::Value>>::new();
            let mut outgoing = BTreeMap::<String, Vec<serde_json::Value>>::new();
            let mut seen_incoming = HashSet::<(String, String, String, u32)>::new();
            let mut seen_outgoing = HashSet::<(String, String, String, u32)>::new();

            for relationship in &relationships {
                let graph = match service.get_symbol_graph(symbol, *relationship, limit) {
                    Ok(graph) => graph,
                    Err(err) => {
                        return serde_json::json!({
                            "symbol": symbol_to_json(symbol),
                            "error": err.to_string(),
                        })
                    }
                };

                for reference in graph.incoming {
                    let key = (
                        reference.source_symbol.id.clone(),
                        reference.target_name.clone(),
                        reference.relationship_type.to_string(),
                        reference.line,
                    );
                    if seen_incoming.insert(key) {
                        let relationship_type = reference.relationship_type.to_string();
                        incoming
                            .entry(relationship_type.clone())
                            .or_default()
                            .push(symbol_reference_to_json(&reference));
                        *relationship_totals.entry(relationship_type).or_default() += 1;
                        total_incoming += 1;
                    }
                }

                for reference in graph.outgoing {
                    let key = (
                        reference.source_symbol.id.clone(),
                        reference.target_name.clone(),
                        reference.relationship_type.to_string(),
                        reference.line,
                    );
                    if seen_outgoing.insert(key) {
                        let relationship_type = reference.relationship_type.to_string();
                        outgoing
                            .entry(relationship_type.clone())
                            .or_default()
                            .push(symbol_reference_to_json(&reference));
                        *relationship_totals.entry(relationship_type).or_default() += 1;
                        total_outgoing += 1;
                    }
                }
            }

            let incoming_count = incoming.values().map(Vec::len).sum::<usize>();
            let outgoing_count = outgoing.values().map(Vec::len).sum::<usize>();
            serde_json::json!({
                "symbol": symbol_to_json(symbol),
                "incoming": incoming,
                "outgoing": outgoing,
                "summary": {
                    "incoming_count": incoming_count,
                    "outgoing_count": outgoing_count,
                    "relationship_count": incoming_count + outgoing_count,
                }
            })
        })
        .collect::<Vec<_>>();

    let payload = serde_json::json!({
        "symbols": expansions,
        "summary": {
            "symbols_expanded": target_symbols.len(),
            "incoming_count": total_incoming,
            "outgoing_count": total_outgoing,
            "relationship_count": total_incoming + total_outgoing,
            "by_relationship_type": relationship_totals,
        },
        "_meta": {
            "tool": "symbol_references",
            "source": "language_service",
            "timing_ms": started.elapsed().as_millis(),
            "index_health": service.index_health_snapshot(),
            "relationship_types": relationships.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "limit_per_relationship": limit,
            "max_symbols": max_symbols,
            "language_support": language_support_meta_json(
                target_symbols.first().map(|symbol| symbol.file_path.as_str())
            ),
            "truncated_symbols": target_symbols.len() >= max_symbols,
        }
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn edit_impact_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let started = Instant::now();
    let limit = parse_bounded_usize_arg(args, "limit", 20, 100);
    let max_symbols = get_bounded_usize_arg(args, &["max_symbols"], 24, 100);
    let relationship_types = relationship_type_values();
    let explicit_symbol = get_str_arg(args, &["symbol_id", "id"]).is_some()
        || get_str_arg(args, &["name", "qualified_name"]).is_some();

    let target_path = match get_str_arg(args, &["path", "file", "file_path"]) {
        Some(path) => match symbol_path_arg(workspace_root, &path) {
            Ok(path) => Some(path),
            Err(err) => return ToolResult::err(err),
        },
        None => None,
    };

    let target_symbols = if explicit_symbol {
        match resolve_symbol_from_graph_args(workspace_root, &service, args) {
            Ok(Some(symbol)) => vec![symbol],
            Ok(None) => return ToolResult::err("symbol not found".to_string()),
            Err(err) => return ToolResult::err(err),
        }
    } else if let Some(path) = target_path.as_deref() {
        match service.get_file_symbols(path) {
            Ok(symbols) => symbols
                .into_iter()
                .filter(is_reference_expansion_symbol)
                .take(max_symbols)
                .collect::<Vec<_>>(),
            Err(err) => return ToolResult::err(err.to_string()),
        }
    } else {
        return ToolResult::err("edit_impact requires 'path' or a symbol selector".to_string());
    };

    if target_symbols.is_empty() {
        return ToolResult::err("no impact-analysis symbols found".to_string());
    }

    let mut impacted = BTreeMap::<String, (u32, Vec<String>, Vec<serde_json::Value>)>::new();
    let mut reference_count = 0usize;
    let mut symbol_payloads = Vec::new();

    for symbol in &target_symbols {
        impacted.entry(symbol.file_path.clone()).or_insert_with(|| {
            (
                90,
                vec!["Direct edit target".to_string()],
                vec![serde_json::json!({
                    "start_line": symbol.range.start.line.saturating_add(1),
                    "end_line": symbol.range.end.line.saturating_add(1),
                    "reason": format!("Target symbol `{}`", symbol.name),
                })],
            )
        });

        let mut incoming_count = 0usize;
        let mut outgoing_count = 0usize;
        let mut relationship_counts = BTreeMap::<String, usize>::new();

        for relationship in &relationship_types {
            let graph = match service.get_symbol_graph(symbol, *relationship, limit) {
                Ok(graph) => graph,
                Err(_) => continue,
            };

            for reference in graph.incoming {
                reference_count += 1;
                incoming_count += 1;
                let relationship_name = reference.relationship_type.to_string();
                *relationship_counts
                    .entry(relationship_name.clone())
                    .or_default() += 1;
                let entry = impacted
                    .entry(reference.source_symbol.file_path.clone())
                    .or_insert_with(|| (0, Vec::new(), Vec::new()));
                entry.0 = entry.0.max(if reference.target_symbol_id.is_some() {
                    78
                } else {
                    62
                });
                let reason = format!(
                    "Incoming {} reference to `{}`",
                    relationship_name, symbol.name
                );
                if !entry.1.iter().any(|existing| existing == &reason) {
                    entry.1.push(reason);
                }
                entry.2.push(serde_json::json!({
                    "start_line": reference.line.saturating_add(1).saturating_sub(8).max(1),
                    "end_line": reference.line.saturating_add(1).saturating_add(24),
                    "reason": format!("{} reference", relationship_name),
                }));
            }

            for reference in graph.outgoing {
                reference_count += 1;
                outgoing_count += 1;
                let relationship_name = reference.relationship_type.to_string();
                *relationship_counts
                    .entry(relationship_name.clone())
                    .or_default() += 1;
                let related_path = reference
                    .target_symbol
                    .as_ref()
                    .map(|symbol| symbol.file_path.clone())
                    .or_else(|| {
                        (reference.relationship_type
                            == crate::tree_sitter::SymbolRelationshipType::Import)
                            .then(|| reference.target_name.clone())
                    });
                let Some(related_path) = related_path else {
                    continue;
                };
                if related_path == symbol.file_path {
                    continue;
                }
                let entry = impacted
                    .entry(related_path.clone())
                    .or_insert_with(|| (0, Vec::new(), Vec::new()));
                entry.0 = entry.0.max(if reference.target_symbol_id.is_some() {
                    70
                } else {
                    56
                });
                let reason = format!(
                    "Outgoing {} dependency from `{}`",
                    relationship_name, symbol.name
                );
                if !entry.1.iter().any(|existing| existing == &reason) {
                    entry.1.push(reason);
                }
            }
        }

        symbol_payloads.push(serde_json::json!({
            "symbol": symbol_to_json(symbol),
            "incoming_count": incoming_count,
            "outgoing_count": outgoing_count,
            "relationship_counts": relationship_counts,
        }));
    }

    let mut impacted_files = impacted
        .into_iter()
        .map(|(path, (score, reasons, mut ranges))| {
            ranges.truncate(6);
            serde_json::json!({
                "path": path,
                "score": score,
                "reasons": reasons,
                "suggested_ranges": ranges,
            })
        })
        .collect::<Vec<_>>();
    impacted_files.sort_by(|a, b| {
        b["score"]
            .as_u64()
            .cmp(&a["score"].as_u64())
            .then_with(|| a["path"].as_str().cmp(&b["path"].as_str()))
    });
    impacted_files.truncate(limit);

    let impacted_paths = impacted_files
        .iter()
        .filter_map(|file| file["path"].as_str().map(ToString::to_string))
        .collect::<Vec<_>>();
    let likely_tests = related_test_files_for_paths(workspace_root, &impacted_paths, limit.min(12));
    let health = service.index_health_snapshot();
    let index_fresh = matches!(
        health.status,
        crate::language_service::IndexHealthStatus::Fresh
    );
    let risk = impact_risk_level(impacted_files.len(), reference_count, likely_tests.len());
    let confidence = impact_confidence(index_fresh, target_symbols.len(), impacted_files.len());
    let recommended_next_steps = vec![
        "Read suggested_ranges for high-score impacted files before editing".to_string(),
        "Run or inspect likely_tests after the change".to_string(),
        "Use symbol_references on high-risk symbols if the impact surface is unclear".to_string(),
    ];
    let language_support_path = target_path.as_deref().or_else(|| {
        target_symbols
            .first()
            .map(|symbol| symbol.file_path.as_str())
    });

    let payload = serde_json::json!({
        "target": {
            "path": target_path,
            "symbols": symbol_payloads,
        },
        "impact": {
            "risk": risk,
            "confidence": confidence,
            "impacted_files": impacted_files,
            "likely_tests": likely_tests,
            "reference_count": reference_count,
            "recommended_next_steps": recommended_next_steps,
        },
        "_meta": {
            "tool": "edit_impact",
            "source": "language_service",
            "timing_ms": started.elapsed().as_millis(),
            "index_health": health,
            "limit": limit,
            "max_symbols": max_symbols,
            "relationship_types": relationship_types.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "language_support": language_support_meta_json(language_support_path),
            "truncated_files": impacted_paths.len() >= limit,
        }
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn symbol_graph_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let symbol = match resolve_symbol_from_graph_args(workspace_root, &service, args) {
        Ok(Some(symbol)) => symbol,
        Ok(None) => return ToolResult::err("symbol not found".to_string()),
        Err(err) => return ToolResult::err(err),
    };
    let relationship = match get_str_arg(args, &["relationship", "relationship_type", "kind"]) {
        Some(kind) => match kind.parse::<crate::tree_sitter::SymbolRelationshipType>() {
            Ok(value) => value,
            Err(err) => return ToolResult::err(err),
        },
        None => crate::tree_sitter::SymbolRelationshipType::Call,
    };
    let limit = parse_bounded_usize_arg(args, "limit", 20, 100);
    let started = Instant::now();
    let graph = match service.get_symbol_graph(&symbol, relationship, limit) {
        Ok(graph) => graph,
        Err(err) => return ToolResult::err(err.to_string()),
    };
    let payload = serde_json::json!({
        "symbol": symbol_to_json(&graph.symbol),
        "incoming": graph.incoming.iter().map(symbol_reference_to_json).collect::<Vec<_>>(),
        "outgoing": graph.outgoing.iter().map(symbol_reference_to_json).collect::<Vec<_>>(),
        "relationship_type": relationship.to_string(),
        "_meta": {
            "tool": "symbol_graph",
            "source": "language_service",
            "timing_ms": started.elapsed().as_millis(),
            "index_health": service.index_health_snapshot(),
            "language_support": language_support_meta_json(Some(&graph.symbol.file_path)),
            "limit": limit,
            "truncated": graph.incoming.len() >= limit || graph.outgoing.len() >= limit,
        }
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn symbol_trace_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let symbol = match resolve_symbol_from_graph_args(workspace_root, &service, args) {
        Ok(Some(symbol)) => symbol,
        Ok(None) => return ToolResult::err("symbol not found".to_string()),
        Err(err) => return ToolResult::err(err),
    };
    let relationships = match parse_relationship_types_arg(args) {
        Ok(values) => values,
        Err(err) => return ToolResult::err(err),
    };
    let direction = match parse_symbol_trace_direction_arg(args) {
        Ok(direction) => direction,
        Err(err) => return ToolResult::err(err),
    };
    let max_depth = parse_bounded_usize_arg(args, "depth", 2, 4);
    let edge_limit = get_bounded_usize_arg(args, &["edge_limit", "limit"], 80, 200);
    let per_node_limit = get_bounded_usize_arg(args, &["per_node_limit"], 16, 50);
    let started = Instant::now();
    let trace = match service.trace_symbol_graph(
        &symbol,
        &relationships,
        direction,
        max_depth,
        edge_limit,
        per_node_limit,
    ) {
        Ok(trace) => trace,
        Err(err) => return ToolResult::err(err.to_string()),
    };
    let payload = serde_json::json!({
        "seed": symbol_to_json(&trace.seed),
        "nodes": trace.nodes.iter().map(symbol_trace_node_to_json).collect::<Vec<_>>(),
        "edges": trace.edges.iter().map(symbol_trace_edge_to_json).collect::<Vec<_>>(),
        "summary": {
            "node_count": trace.nodes.len(),
            "edge_count": trace.edges.len(),
            "max_depth": trace.max_depth,
            "unresolved_edge_count": trace.unresolved_edges,
            "truncated": trace.truncated,
        },
        "_meta": {
            "tool": "symbol_trace",
            "source": "language_service",
            "timing_ms": started.elapsed().as_millis(),
            "index_health": service.index_health_snapshot(),
            "language_support": language_support_meta_json(Some(&trace.seed.file_path)),
            "direction": direction.as_str(),
            "relationship_types": relationships.iter().map(ToString::to_string).collect::<Vec<_>>(),
            "edge_limit": edge_limit,
            "per_node_limit": per_node_limit,
        }
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn symbol_schema_tool<R: tauri::Runtime>(
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    let service = match language_service_from_app_handle(app_handle) {
        Ok(service) => service,
        Err(err) => return ToolResult::err(err),
    };
    let scope_path = args
        .get("path")
        .or_else(|| args.get("scope"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let started = Instant::now();
    let schema = match service.index_schema_snapshot_for_path(scope_path) {
        Ok(schema) => schema,
        Err(err) => return ToolResult::err(err.to_string()),
    };
    let scope_meta = schema.scope.as_ref().map(|scope| {
        serde_json::json!({
            "requested_path": scope.requested_path,
            "normalized_path": scope.normalized_path,
            "root_totals": scope.root_totals,
            "scoped_totals": schema.totals,
        })
    });
    let payload = serde_json::json!({
        "schema": schema,
        "_meta": {
            "tool": "symbol_schema",
            "source": "language_service",
            "timing_ms": started.elapsed().as_millis(),
            "scope": scope_meta,
            "index_health": service.index_health_snapshot(),
            "language_support": language_support_meta_json(scope_path),
        }
    });
    ToolResult::ok(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

pub fn apply_patch_to_string(
    content: &str,
    old_text: &str,
    new_text: &str,
) -> Result<String, String> {
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
        let mut out = String::with_capacity(content.len() - old_text.len() + new_text.len());
        out.push_str(&content[..pos]);
        out.push_str(new_text);
        out.push_str(&content[pos + old_text.len()..]);
        return Ok(out);
    }

    Err(format!(
        "old_text not found in file (searched {} chars). Exact match required; whitespace-normalized fuzzy matching is disabled for safety.",
        old_text.len()
    ))
}

fn line_window_byte_range(
    content: &str,
    start_line: usize,
    end_line: Option<usize>,
) -> Result<(usize, usize), String> {
    if start_line == 0 {
        return Err("start_line must be 1-indexed".to_string());
    }

    let last_line = content.lines().count().max(1);
    let start = start_line.min(last_line);
    let end = end_line.unwrap_or(start).max(start).min(last_line);
    let mut current_line = 1;
    let mut start_byte = 0;
    let mut end_byte = content.len();

    for (idx, ch) in content.char_indices() {
        if current_line == start {
            start_byte = idx;
            break;
        }
        if ch == '\n' {
            current_line += 1;
        }
    }

    current_line = 1;
    for (idx, ch) in content.char_indices() {
        if current_line > end {
            end_byte = idx;
            break;
        }
        if ch == '\n' {
            current_line += 1;
        }
    }

    Ok((start_byte, end_byte))
}

pub fn apply_patch_to_string_with_line_hint(
    content: &str,
    old_text: &str,
    new_text: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<String, String> {
    let Some(start_line) = start_line else {
        return apply_patch_to_string(content, old_text, new_text);
    };
    let (window_start, window_end) = line_window_byte_range(content, start_line, end_line)?;
    let window = &content[window_start..window_end];

    if !old_text.is_empty() {
        let mut exact_matches = window.match_indices(old_text);
        if let Some((relative_pos, _)) = exact_matches.next() {
            if exact_matches.next().is_some() {
                return Err(format!(
                    "Ambiguous match: old_text appears multiple times within line hint {}-{}. Please provide more unique context.",
                    start_line,
                    end_line.unwrap_or(start_line)
                ));
            }

            let pos = window_start + relative_pos;
            let mut out = String::with_capacity(content.len() - old_text.len() + new_text.len());
            out.push_str(&content[..pos]);
            out.push_str(new_text);
            out.push_str(&content[pos + old_text.len()..]);
            return Ok(out);
        }
    }

    Err(format!(
        "old_text not found in hinted line range {}-{} (searched {} chars). Exact match required.",
        start_line,
        end_line.unwrap_or(start_line),
        old_text.len()
    ))
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct PatchHunk {
    old_text: String,
    new_text: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
}

fn apply_multi_patch_to_string(content: &str, patches: &[PatchHunk]) -> Result<String, String> {
    if patches.is_empty() {
        return Err("No patches provided".to_string());
    }

    let mut working = content.to_string();
    for (idx, patch) in patches.iter().enumerate() {
        match apply_patch_to_string_with_line_hint(
            &working,
            &patch.old_text,
            &patch.new_text,
            patch.start_line,
            patch.end_line,
        ) {
            Ok(new_content) => working = new_content,
            Err(e) => {
                return Err(format!("Patch {} failed (no changes made): {}", idx + 1, e));
            }
        }
    }

    Ok(working)
}

struct SemanticPatchWrite {
    file_path: String,
    abs_path: PathBuf,
    original_content: String,
    new_content: String,
}

struct StagedSemanticPatchWrite {
    write: SemanticPatchWrite,
    temp_path: PathBuf,
    backup_path: PathBuf,
}

fn semantic_patch_sidecar_path(
    abs_path: &Path,
    stage_id: &str,
    idx: usize,
    suffix: &str,
) -> PathBuf {
    let file_name = abs_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("semantic-patch");
    let sidecar_name = format!(
        ".{}.zblade-semantic-{}-{}.{}",
        file_name, stage_id, idx, suffix
    );
    abs_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(sidecar_name)
}

fn write_synced_file(path: &Path, content: &str) -> Result<(), String> {
    let mut file = fs::File::create(path)
        .map_err(|error| format!("create {} failed: {}", path.display(), error))?;
    file.write_all(content.as_bytes())
        .map_err(|error| format!("write {} failed: {}", path.display(), error))?;
    file.sync_all()
        .map_err(|error| format!("sync {} failed: {}", path.display(), error))
}

fn cleanup_semantic_patch_stage_files(staged_writes: &[StagedSemanticPatchWrite]) {
    for staged in staged_writes {
        let _ = fs::remove_file(&staged.temp_path);
        let _ = fs::remove_file(&staged.backup_path);
    }
}

fn cleanup_semantic_patch_backups(staged_writes: &[StagedSemanticPatchWrite]) {
    for staged in staged_writes {
        let _ = fs::remove_file(&staged.backup_path);
    }
}

fn stage_semantic_patch_writes(
    writes: Vec<SemanticPatchWrite>,
) -> Result<Vec<StagedSemanticPatchWrite>, String> {
    let stage_id = uuid::Uuid::new_v4().simple().to_string();
    let mut staged_writes = Vec::with_capacity(writes.len());

    for (idx, write) in writes.into_iter().enumerate() {
        let temp_path = semantic_patch_sidecar_path(&write.abs_path, &stage_id, idx, "tmp");
        let backup_path = semantic_patch_sidecar_path(&write.abs_path, &stage_id, idx, "bak");

        if let Err(error) = write_synced_file(&temp_path, &write.new_content) {
            let _ = fs::remove_file(&temp_path);
            cleanup_semantic_patch_stage_files(&staged_writes);
            return Err(format!("Failed to stage {}: {}", write.file_path, error));
        }

        staged_writes.push(StagedSemanticPatchWrite {
            write,
            temp_path,
            backup_path,
        });
    }

    Ok(staged_writes)
}

fn parse_semantic_patch_args(
    args: &HashMap<String, serde_json::Value>,
) -> Result<Option<crate::semantic_patch::SemanticPatch>, String> {
    let Some(patch_value) = args.get("semantic_patch").or_else(|| args.get("patch")) else {
        return Ok(None);
    };

    if !patch_value.is_object() {
        return Err("semantic_patch must be an object".to_string());
    }

    serde_json::from_value::<crate::semantic_patch::SemanticPatch>(patch_value.clone())
        .map(Some)
        .map_err(|error| format!("invalid semantic_patch payload: {}", error))
}

fn collect_semantic_patch_writes(
    workspace_root: &Path,
    result: crate::semantic_patch::ApplyResult,
) -> Result<Vec<SemanticPatchWrite>, String> {
    let mut writes = Vec::with_capacity(1 + result.additional_changes.len());

    let primary_abs = validate_path_under_workspace(workspace_root, Path::new(&result.file_path))?;
    writes.push(SemanticPatchWrite {
        file_path: result.file_path,
        abs_path: primary_abs,
        original_content: result.original_content,
        new_content: result.new_content,
    });

    for change in result.additional_changes {
        let abs_path = validate_path_under_workspace(workspace_root, Path::new(&change.file_path))?;
        writes.push(SemanticPatchWrite {
            file_path: change.file_path,
            abs_path,
            original_content: change.original_content,
            new_content: change.new_content,
        });
    }

    Ok(writes)
}

fn rollback_semantic_patch_writes(
    service: &std::sync::Arc<crate::language_service::LanguageService>,
    staged_writes: &[StagedSemanticPatchWrite],
    applied_count: usize,
) {
    for staged in staged_writes.iter().take(applied_count).rev() {
        let _ = fs::remove_file(&staged.write.abs_path);
        if fs::rename(&staged.backup_path, &staged.write.abs_path).is_err() {
            let _ = fs::write(&staged.write.abs_path, &staged.write.original_content);
        }
        let _ = service.did_open(&staged.write.file_path, &staged.write.original_content);
    }
    cleanup_semantic_patch_stage_files(staged_writes);
}

fn apply_semantic_patch_writes_with_service<F>(
    workspace_root: &Path,
    service: &std::sync::Arc<crate::language_service::LanguageService>,
    patch: &crate::semantic_patch::SemanticPatch,
    before_commit: F,
) -> Result<Vec<SemanticPatchWrite>, String>
where
    F: FnOnce(&[SemanticPatchWrite]),
{
    let applier = crate::semantic_patch::PatchApplier::new(service.clone());
    let result = applier.apply(patch).map_err(|error| error.to_string())?;
    let writes = collect_semantic_patch_writes(workspace_root, result)?;
    before_commit(&writes);
    let staged_writes = stage_semantic_patch_writes(writes)?;
    let mut applied_count = 0usize;

    for staged in &staged_writes {
        if let Err(error) = fs::rename(&staged.write.abs_path, &staged.backup_path) {
            cleanup_semantic_patch_stage_files(&staged_writes);
            return Err(format!(
                "Failed to backup {}: {}",
                staged.write.file_path, error
            ));
        }
        if let Err(error) = fs::rename(&staged.temp_path, &staged.write.abs_path) {
            let _ = fs::rename(&staged.backup_path, &staged.write.abs_path);
            rollback_semantic_patch_writes(service, &staged_writes, applied_count);
            return Err(format!(
                "Failed to commit {}: {}",
                staged.write.file_path, error
            ));
        }
        applied_count += 1;
    }

    for staged in &staged_writes {
        if let Err(error) = service.did_open(&staged.write.file_path, &staged.write.new_content) {
            rollback_semantic_patch_writes(service, &staged_writes, applied_count);
            return Err(format!(
                "Failed to index {}: {}",
                staged.write.file_path, error
            ));
        }
    }

    cleanup_semantic_patch_backups(&staged_writes);
    Ok(staged_writes
        .into_iter()
        .map(|staged| staged.write)
        .collect())
}

#[cfg(test)]
fn apply_semantic_patch_with_service(
    workspace_root: &Path,
    service: &std::sync::Arc<crate::language_service::LanguageService>,
    patch: &crate::semantic_patch::SemanticPatch,
) -> Result<Vec<String>, String> {
    Ok(
        apply_semantic_patch_writes_with_service(workspace_root, service, patch, |_| {})?
            .into_iter()
            .map(|write| write.file_path)
            .collect(),
    )
}

fn emit_change_applied_for_paths<R: tauri::Runtime>(
    app_handle: &tauri::AppHandle<R>,
    change_id: &str,
    file_paths: &[String],
) {
    if file_paths.is_empty() {
        return;
    }

    use tauri::Emitter;

    crate::blade_event_scheduler::queue_refresh_explorer(app_handle);
    let _ = app_handle.emit(
        crate::events::event_names::CHANGE_APPLIED,
        crate::events::ChangeAppliedPayload {
            change_id: change_id.to_string(),
            file_path: file_paths[0].clone(),
            file_paths: file_paths.to_vec(),
        },
    );
}

fn sync_after_tool_write<R: tauri::Runtime>(
    app_handle: Option<&tauri::AppHandle<R>>,
    change_id: &str,
    abs_path: &Path,
    display_path: &str,
) {
    if let Some(handle) = app_handle {
        crate::file_state_sync::sync_from_disk_after_write(handle, abs_path);
        let changed_paths = vec![display_path.to_string()];
        emit_change_applied_for_paths(handle, change_id, &changed_paths);
    }
}

struct ToolWriteTracking {
    change_id: String,
    snapshot_id: String,
    base_content: String,
}

fn prepare_tool_write_tracking<R: tauri::Runtime>(
    app_handle: Option<&tauri::AppHandle<R>>,
    change_id: &str,
    abs_path: &Path,
    original_content: &str,
) -> Option<ToolWriteTracking> {
    let handle = app_handle?;
    use tauri::Manager;
    let state = handle.state::<crate::app_state::AppState>();

    if let Some(existing_change) = state
        .uncommitted_changes
        .get_by_path(&abs_path.to_path_buf())
    {
        let base_content = state
            .history_service()
            .ok()
            .and_then(|history_service| {
                history_service
                    .get_snapshot_content(&existing_change.snapshot_id)
                    .ok()
            })
            .unwrap_or_else(|| original_content.to_string());
        return Some(ToolWriteTracking {
            change_id: existing_change.id,
            snapshot_id: existing_change.snapshot_id,
            base_content,
        });
    }

    let history_service = match state.history_service() {
        Ok(service) => service,
        Err(error) => {
            eprintln!(
                "[HISTORY] Failed to initialize history service for {}: {}",
                abs_path.display(),
                error
            );
            return None;
        }
    };

    let snapshot = if abs_path.exists() {
        history_service.create_snapshot(abs_path, Some(change_id.to_string()))
    } else {
        history_service.create_missing_file_snapshot(abs_path, Some(change_id.to_string()))
    };

    match snapshot {
        Ok(entry) => {
            use tauri::Emitter;
            let snapshot_id = entry.id.clone();
            let _ = handle.emit(
                crate::events::event_names::HISTORY_ENTRY_ADDED,
                crate::events::HistoryEntryAddedPayload { entry },
            );
            Some(ToolWriteTracking {
                change_id: change_id.to_string(),
                snapshot_id,
                base_content: original_content.to_string(),
            })
        }
        Err(error) => {
            eprintln!(
                "[HISTORY] Failed to create snapshot for {}: {}",
                abs_path.display(),
                error
            );
            None
        }
    }
}

fn track_tool_write<R: tauri::Runtime>(
    app_handle: Option<&tauri::AppHandle<R>>,
    abs_path: &Path,
    tracking: Option<ToolWriteTracking>,
) {
    let Some(handle) = app_handle else {
        return;
    };
    let Some(tracking) = tracking else {
        return;
    };

    use tauri::Manager;
    let state = handle.state::<crate::app_state::AppState>();
    let new_content = fs::read_to_string(abs_path).unwrap_or_default();
    let diff =
        crate::uncommitted_changes::generate_unified_diff(&tracking.base_content, &new_content);
    let (added, removed) = crate::uncommitted_changes::count_diff_stats(&diff);

    let uncommitted = crate::uncommitted_changes::UncommittedChange {
        id: tracking.change_id,
        file_path: abs_path.to_path_buf(),
        snapshot_id: tracking.snapshot_id,
        unified_diff: diff,
        added_lines: added,
        removed_lines: removed,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        file_modified_ms: crate::uncommitted_changes::file_modified_ms(&abs_path.to_path_buf()),
    };

    state.uncommitted_changes.track(uncommitted);
}

fn apply_edit_tool<R: tauri::Runtime>(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
    app_handle: Option<&tauri::AppHandle<R>>,
) -> ToolResult {
    match parse_semantic_patch_args(args) {
        Ok(Some(patch)) => {
            let service = match language_service_from_app_handle(app_handle) {
                Ok(service) => service,
                Err(err) => return ToolResult::err(err),
            };

            let mut tracking = Vec::<(PathBuf, Option<ToolWriteTracking>)>::new();
            return match apply_semantic_patch_writes_with_service(
                workspace_root,
                &service,
                &patch,
                |writes| {
                    tracking = writes
                        .iter()
                        .map(|write| {
                            (
                                write.abs_path.clone(),
                                prepare_tool_write_tracking(
                                    app_handle,
                                    &patch.id,
                                    &write.abs_path,
                                    &write.original_content,
                                ),
                            )
                        })
                        .collect();
                },
            ) {
                Ok(writes) => {
                    if let Some(handle) = app_handle {
                        for (abs_path, item) in tracking {
                            track_tool_write(app_handle, &abs_path, item);
                            crate::file_state_sync::sync_from_disk_after_write(handle, &abs_path);
                        }
                        let paths = writes
                            .iter()
                            .map(|write| write.file_path.clone())
                            .collect::<Vec<_>>();
                        emit_change_applied_for_paths(handle, &patch.id, &paths);
                    }

                    let paths = writes
                        .into_iter()
                        .map(|write| write.file_path)
                        .collect::<Vec<_>>();
                    ToolResult::ok(format!("Applied semantic patch to {}", paths.join(", ")))
                }
                Err(err) => ToolResult::err(err),
            };
        }
        Ok(None) => {}
        Err(err) => return ToolResult::err(err),
    }

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
    let change_id = get_str_arg(args, &["id", "change_id", "tool_call_id"])
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

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
                Ok(new_content) => {
                    let tracking =
                        prepare_tool_write_tracking(app_handle, &change_id, &abs, &content);
                    match fs::write(&abs, new_content.as_bytes()) {
                        Ok(()) => {
                            track_tool_write(app_handle, &abs, tracking);
                            sync_after_tool_write(app_handle, &change_id, &abs, &path);
                            let count = patches.len();
                            ToolResult::ok(format!(
                                "Applied {} patch{} atomically to {}",
                                count,
                                if count == 1 { "" } else { "es" },
                                path
                            ))
                        }
                        Err(e) => ToolResult::err(format!("Failed to write file: {}", e)),
                    }
                }
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
        let start_line = args
            .get("start_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);
        let end_line = args
            .get("end_line")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize);

        match apply_patch_to_string_with_line_hint(
            &content, &old_text, &new_text, start_line, end_line,
        ) {
            Ok(new_content) => {
                let tracking = prepare_tool_write_tracking(app_handle, &change_id, &abs, &content);
                match fs::write(&abs, new_content.as_bytes()) {
                    Ok(()) => {
                        track_tool_write(app_handle, &abs, tracking);
                        sync_after_tool_write(app_handle, &change_id, &abs, &path);
                        ToolResult::ok(format!("Applied edit to {}", path))
                    }
                    Err(e) => ToolResult::err(e.to_string()),
                }
            }
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
    let depth = args
        .get("depth")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_LIST_DEPTH as u64) as usize;
    let limit = args
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(DEFAULT_LIST_LIMIT as u64) as usize;
    let limit = limit.min(MAX_LIST_LIMIT); // Cap at maximum

    let abs = match validate_path_under_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    // Load gitignore filter if enabled in project settings
    let gitignore_filter = create_gitignore_filter(workspace_root);

    // Collect entries with BFS traversal (like Codex)
    let mut entries: Vec<ListEntry> = Vec::new();
    collect_dir_entries(&abs, &abs, depth, gitignore_filter.as_ref(), &mut entries);

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
        output.push_str(&format!(
            "\n(showing {} of more entries, use a more specific path or increase limit)\n",
            limit
        ));
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
    let rel_to_base = current_path
        .strip_prefix(base_path)
        .unwrap_or(Path::new(""));
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
