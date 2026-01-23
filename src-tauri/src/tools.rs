use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

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
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        Self {
            success: true,
            content: content.into(),
            error: None,
        }
    }

    pub fn err(error: impl Into<String>) -> Self {
        Self {
            success: false,
            content: String::new(),
            error: Some(error.into()),
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
    let settings = project_settings::load_project_settings(workspace_root);
    
    // If allow_gitignored_files is true, don't create a filter (allow all files)
    if settings.allow_gitignored_files {
        eprintln!("[GITIGNORE] Filtering disabled by project settings");
        return None;
    }
    
    // Create filter to respect .gitignore
    let filter = GitignoreFilter::new(workspace_root);
    eprintln!("[GITIGNORE] Filtering enabled for workspace: {}", workspace_root.display());
    Some(filter)
}

// Editor state for IDE-specific tools
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
    _app_handle: Option<&tauri::AppHandle<R>>,
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

    match tool_name {
        // Legacy tools (kept for compatibility)
        "read_file" => read_file(workspace_root, &args),
        "write_file" | "create_file" => write_file(workspace_root, &args),
        "edit_file" => edit_file(workspace_root, &args),
        "grep_search" | "rg" => grep_search(workspace_root, &args),
        "codebase_search" => codebase_search(workspace_root, &args),
        "list_directory" | "list_dir" => list_directory(workspace_root, &args),

        // Phase 1 IDE-specific tools
        "get_editor_state" => get_editor_state(editor_state),
        "read_file_range" => read_file_range(workspace_root, &args),
        "apply_edit" | "apply_patch" => apply_edit_tool(workspace_root, &args),
        "get_workspace_structure" => get_workspace_structure(workspace_root, &args),


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

fn validate_path_under_workspace(workspace_root: &Path, path: &Path) -> Result<PathBuf, String> {
    let ws = fs::canonicalize(workspace_root).map_err(|e| e.to_string())?;

    let candidate = if path.is_absolute() {
        path.to_path_buf()
    } else {
        ws.join(path)
    };

    let candidate = fs::canonicalize(&candidate).map_err(|e| e.to_string())?;
    if !candidate.starts_with(&ws) {
        return Err("path is outside workspace".to_string());
    }

    Ok(candidate)
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

fn write_file(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(path) = get_str_arg(args, &["path", "file_path", "filepath", "filename"]) else {
        return ToolResult::err("missing required arg: path (or file_path)");
    };
    let Some(content) = get_str_arg(args, &["content", "contents", "text", "data"]) else {
        return ToolResult::err("missing required arg: content (or contents/text)");
    };

    let ws = match fs::canonicalize(workspace_root) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(format!("cannot canonicalize workspace: {}", e)),
    };

    let requested = Path::new(&path);
    let target = if requested.is_absolute() {
        PathBuf::from(&path)
    } else {
        ws.join(requested)
    };

    let Some(parent) = target.parent() else {
        return ToolResult::err("invalid path: no parent directory".to_string());
    };

    if let Err(e) = fs::create_dir_all(parent) {
        return ToolResult::err(format!("cannot create parent directory: {}", e));
    }
    let parent_canon = match fs::canonicalize(parent) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(format!("cannot canonicalize parent: {}", e)),
    };

    if !parent_canon.starts_with(&ws) {
        return ToolResult::err(format!(
            "path is outside workspace (workspace: {:?}, parent: {:?})",
            ws, parent_canon
        ));
    }
    let Some(fname) = target.file_name() else {
        return ToolResult::err("invalid path: missing file name".to_string());
    };
    let abs = parent_canon.join(fname);

    match fs::write(&abs, content.as_bytes()) {
        Ok(()) => ToolResult::ok(format!("wrote {} bytes to {:?}", content.len(), abs)),
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

fn grep_search(workspace_root: &Path, args: &HashMap<String, serde_json::Value>) -> ToolResult {
    let Some(pattern) = get_str_arg(args, &["pattern", "query", "regex"]) else {
        return ToolResult::err(
            "grep_search requires a 'pattern' argument. Example: {\"pattern\": \"Priority\"}",
        );
    };
    let path = get_str_arg(args, &["path", "dir", "directory"]).unwrap_or_else(|| ".".to_string());

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

    let mut out = String::new();
    for entry in WalkDir::new(abs)
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

        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };

        for (idx, line) in text.lines().enumerate() {
            if re.is_match(line) {
                out.push_str(&format!(
                    "{}:{}:{}\n",
                    path.to_string_lossy(),
                    idx + 1,
                    line
                ));
            }
        }
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
    let start = start_line.saturating_sub(1).saturating_sub(context_lines);
    let end = end_line
        .min(total_lines)
        .saturating_add(context_lines)
        .min(total_lines);

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
    if let Some(pos) = content.find(old_text) {
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

fn get_workspace_structure(
    workspace_root: &Path,
    args: &HashMap<String, serde_json::Value>,
) -> ToolResult {
    let path = get_str_arg(args, &["path", "dir", "directory"]).unwrap_or_else(|| ".".to_string());
    let max_depth = args.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
    let include_hidden = args
        .get("include_hidden")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let abs = match validate_path_under_workspace(workspace_root, Path::new(&path)) {
        Ok(p) => p,
        Err(e) => return ToolResult::err(e),
    };

    // Load gitignore filter if needed
    let gitignore_filter = create_gitignore_filter(workspace_root);

    let mut output = format!("Directory: {}\n", abs.to_string_lossy());
    build_tree_structure(
        &abs,
        &mut output,
        0,
        max_depth,
        include_hidden,
        "",
        gitignore_filter.as_ref(),
    );

    ToolResult::ok(output)
}

fn build_tree_structure(
    path: &Path,
    output: &mut String,
    depth: usize,
    max_depth: usize,
    include_hidden: bool,
    prefix: &str,
    gitignore_filter: Option<&GitignoreFilter>,
) {
    if depth >= max_depth {
        return;
    }

    let Ok(entries) = fs::read_dir(path) else {
        return;
    };

    let mut items: Vec<_> = entries.filter_map(Result::ok).collect();
    items.sort_by_key(|e| e.path());

    // Separate directories and files
    let mut dirs = Vec::new();
    let mut files = Vec::new();

    for entry in items {
        let name = entry.file_name().to_string_lossy().to_string();

        // Skip hidden files unless requested
        if !include_hidden && name.starts_with('.') {
            continue;
        }

        // Check gitignore filter
        if let Some(filter) = gitignore_filter {
            if filter.should_ignore(&entry.path()) {
                continue;
            }
        }

        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            dirs.push((name, entry.path()));
        } else {
            let size = entry.metadata().ok().map(|m| m.len()).unwrap_or(0);
            files.push((name, size));
        }
    }

    // Output directories first
    for (idx, (name, dir_path)) in dirs.iter().enumerate() {
        let is_last = idx == dirs.len() - 1 && files.is_empty();
        let connector = if is_last { "└── " } else { "├── " };
        output.push_str(&format!("{}{}{}/\n", prefix, connector, name));

        let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
        build_tree_structure(
            dir_path,
            output,
            depth + 1,
            max_depth,
            include_hidden,
            &new_prefix,
            gitignore_filter,
        );
    }

    // Output files
    for (idx, (name, size)) in files.iter().enumerate() {
        let is_last = idx == files.len() - 1;
        let connector = if is_last { "└── " } else { "├── " };
        let size_str = format_file_size(*size);
        output.push_str(&format!(
            "{}{}{} ({})
",
            prefix, connector, name, size_str
        ));
    }
}

fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
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
