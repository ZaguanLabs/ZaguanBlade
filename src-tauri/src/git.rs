use serde::Serialize;
use std::process::Command;
use tauri::State;

use crate::AppState;
use crate::models::{ollama, openai_compat, registry};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusSummary {
    pub is_repo: bool,
    pub changed_count: u32,
    pub staged_count: u32,
    pub unstaged_count: u32,
    pub untracked_count: u32,
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitFileStatus {
    pub path: String,
    pub display_path: Option<String>,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub conflicted: bool,
    pub status_code: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitPreflightResult {
    pub can_commit: bool,
    pub is_repo: bool,
    pub branch: Option<String>,
    pub is_detached: bool,
    pub has_upstream: bool,
    pub has_conflicts: bool,
    pub staged_count: u32,
    pub error_message: Option<String>,
}

fn empty_summary() -> GitStatusSummary {
    GitStatusSummary {
        is_repo: false,
        changed_count: 0,
        staged_count: 0,
        unstaged_count: 0,
        untracked_count: 0,
        branch: None,
        ahead: 0,
        behind: 0,
        dirty: false,
    }
}

fn parse_git_status(output: &str) -> GitStatusSummary {
    let mut summary = GitStatusSummary {
        is_repo: true,
        changed_count: 0,
        staged_count: 0,
        unstaged_count: 0,
        untracked_count: 0,
        branch: None,
        ahead: 0,
        behind: 0,
        dirty: false,
    };

    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            if let Some(head) = rest.strip_prefix("branch.head ") {
                let head = head.trim();
                if head != "(detached)" && head != "(unknown)" {
                    summary.branch = Some(head.to_string());
                }
                continue;
            }
            if let Some(ab) = rest.strip_prefix("branch.ab ") {
                let parts: Vec<&str> = ab.split_whitespace().collect();
                for part in parts {
                    if let Some(ahead) = part.strip_prefix('+') {
                        summary.ahead = ahead.parse::<u32>().unwrap_or(0);
                    } else if let Some(behind) = part.strip_prefix('-') {
                        summary.behind = behind.parse::<u32>().unwrap_or(0);
                    }
                }
                continue;
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("1 ") {
            let mut parts = rest.split_whitespace();
            let xy = parts.next().unwrap_or("..");
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or('.');
            let y = chars.next().unwrap_or('.');
            if x != '.' {
                summary.staged_count += 1;
            }
            if y != '.' {
                summary.unstaged_count += 1;
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("2 ") {
            let mut parts = rest.split_whitespace();
            let xy = parts.next().unwrap_or("..");
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or('.');
            let y = chars.next().unwrap_or('.');
            if x != '.' {
                summary.staged_count += 1;
            }
            if y != '.' {
                summary.unstaged_count += 1;
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix("u ") {
            let mut parts = rest.split_whitespace();
            let xy = parts.next().unwrap_or("..");
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or('.');
            let y = chars.next().unwrap_or('.');
            if x != '.' {
                summary.staged_count += 1;
            }
            if y != '.' {
                summary.unstaged_count += 1;
            }
            continue;
        }

        if line.starts_with("? ") {
            summary.untracked_count += 1;
        }
    }

    summary.changed_count = summary.staged_count + summary.unstaged_count + summary.untracked_count;
    summary.dirty = summary.changed_count > 0;

    summary
}

fn workspace_root(state: &State<'_, AppState>) -> Option<String> {
    let ws = state.workspace.lock().unwrap();
    ws.workspace
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
}

fn run_git(root: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git {:?}: {}", args, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git {:?} failed: {}", args, stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

struct CommitContext {
    files: Vec<String>,
    diff: String,
    diff_stat: String,
    new_file_content: String,
    staged: bool,
    branch: Option<String>,
    last_commit_message: Option<String>,
    recent_commits: Vec<String>,
}

async fn load_available_models(state: &State<'_, AppState>) -> Vec<registry::ModelInfo> {
    let (blade_url, api_key, ollama_enabled, ollama_url, openai_compat_enabled, openai_compat_url) = {
        let config = state.config.lock().unwrap();
        (
            config.blade_url.clone(),
            config.api_key.clone(),
            config.ollama_enabled,
            config.ollama_url.clone(),
            config.openai_compat_enabled,
            config.openai_compat_url.clone(),
        )
    };

    let mut models = registry::get_models(&blade_url, &api_key).await;
    if ollama_enabled {
        let mut ollama_models = ollama::get_models(&ollama_url).await;
        models.append(&mut ollama_models);
    }

    if openai_compat_enabled {
        let mut openai_compat_models = openai_compat::get_models(&openai_compat_url).await;
        models.append(&mut openai_compat_models);
    }

    models
}

fn resolve_model_id(models: &[registry::ModelInfo], requested_id: &str) -> String {
    let matched = models
        .iter()
        .position(|m| m.id == requested_id)
        .or_else(|| models.iter().position(|m| m.api_id.as_deref() == Some(requested_id)))
        .or_else(|| {
            let id_lower = requested_id.to_lowercase();
            models
                .iter()
                .position(|m| m.id.to_lowercase() == id_lower)
                .or_else(|| {
                    models.iter().position(|m| {
                        m.api_id
                            .as_ref()
                            .map(|s| s.to_lowercase())
                            .as_deref()
                            == Some(&id_lower)
                    })
                })
        });

    if let Some(idx) = matched {
        let model = &models[idx];
        let provider = model.provider.as_deref().unwrap_or("");
        if provider == "ollama" || provider == "openai-compat" {
            model.id.clone()
        } else {
            model.api_id.as_ref().unwrap_or(&model.id).clone()
        }
    } else {
        requested_id.to_string()
    }
}

fn collect_changes_for_message(root: &str) -> Result<CommitContext, String> {
    let staged_files = run_git(root, &["diff", "--cached", "--name-only"])?;
    let mut files: Vec<String> = staged_files
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    let staged = !files.is_empty();

    if !staged {
        let unstaged_files = run_git(root, &["diff", "--name-only"])?;
        files = unstaged_files
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
    }

    // Get untracked files
    let untracked_output = run_git(root, &["ls-files", "--others", "--exclude-standard"])?;
    let untracked: Vec<String> = untracked_output
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    // Add untracked to file list if not looking at staged
    if !staged {
        files.extend(untracked.clone());
    }

    let diff_args = if staged {
        vec!["diff", "--cached", "--unified=3"]
    } else {
        vec!["diff", "--unified=3"]
    };
    let mut diff = run_git(root, &diff_args)?;

    // Get diff stats (insertions/deletions summary)
    let diff_stat_args = if staged {
        vec!["diff", "--cached", "--stat"]
    } else {
        vec!["diff", "--stat"]
    };
    let diff_stat = run_git(root, &diff_stat_args).unwrap_or_default();

    // Get current branch
    let branch = run_git(root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD");

    // Get last commit message for style reference
    let last_commit_message = run_git(root, &["log", "-1", "--format=%B"])
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Get recent commits for context
    let recent_commits: Vec<String> = run_git(root, &["log", "--oneline", "-5"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();

    // For untracked files, include a preview of their content
    let mut new_file_content = String::new();
    let files_to_preview = if staged { vec![] } else { untracked };
    const MAX_PREVIEW_PER_FILE: usize = 2000;
    const MAX_TOTAL_PREVIEW: usize = 6000;

    for file in files_to_preview {
        if new_file_content.len() >= MAX_TOTAL_PREVIEW {
            new_file_content.push_str("\n...more new files omitted...");
            break;
        }
        let path = std::path::Path::new(root).join(&file);
        if let Ok(content) = std::fs::read_to_string(&path) {
            let preview: String = content.chars().take(MAX_PREVIEW_PER_FILE).collect();
            new_file_content.push_str(&format!("\n=== NEW FILE: {} ===\n{}", file, preview));
            if content.len() > MAX_PREVIEW_PER_FILE {
                new_file_content.push_str("\n...truncated...");
            }
        }
    }

    const DIFF_LIMIT: usize = 8000;
    if diff.len() > DIFF_LIMIT {
        diff.truncate(DIFF_LIMIT);
        diff.push_str("\n...diff truncated...");
    }

    Ok(CommitContext {
        files,
        diff,
        diff_stat,
        new_file_content,
        staged,
        branch,
        last_commit_message,
        recent_commits,
    })
}

fn parse_git_status_files(output: &str) -> Vec<GitFileStatus> {
    let mut files = Vec::new();

    for line in output.lines() {
        if line.starts_with("# ") {
            continue;
        }

        if let Some(rest) = line.strip_prefix("1 ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let xy = parts[0];
            let path = parts.last().unwrap_or(&"").to_string();
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or('.');
            let y = chars.next().unwrap_or('.');
            files.push(GitFileStatus {
                path,
                display_path: None,
                staged: x != '.',
                unstaged: y != '.',
                untracked: false,
                conflicted: false,
                status_code: format!("{}{}", x, y),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("2 ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 3 {
                continue;
            }
            let xy = parts[0];
            let new_path = parts.get(parts.len().saturating_sub(2)).unwrap_or(&"");
            let old_path = parts.get(parts.len().saturating_sub(1)).unwrap_or(&"");
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or('.');
            let y = chars.next().unwrap_or('.');
            files.push(GitFileStatus {
                path: new_path.to_string(),
                display_path: Some(format!("{} → {}", old_path, new_path)),
                staged: x != '.',
                unstaged: y != '.',
                untracked: false,
                conflicted: false,
                status_code: format!("{}{}", x, y),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("u ") {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let xy = parts[0];
            let path = parts.last().unwrap_or(&"").to_string();
            let mut chars = xy.chars();
            let x = chars.next().unwrap_or('.');
            let y = chars.next().unwrap_or('.');
            files.push(GitFileStatus {
                path,
                display_path: None,
                staged: x != '.',
                unstaged: y != '.',
                untracked: false,
                conflicted: true,
                status_code: format!("{}{}", x, y),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("? ") {
            let path = rest.trim().to_string();
            files.push(GitFileStatus {
                path,
                display_path: None,
                staged: false,
                unstaged: true,
                untracked: true,
                conflicted: false,
                status_code: "??".to_string(),
            });
        }
    }

    files
}

#[tauri::command]
pub fn git_status_summary(state: State<'_, AppState>) -> Result<GitStatusSummary, String> {
    let Some(root) = workspace_root(&state) else {
        return Ok(empty_summary());
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("status")
        .arg("--porcelain=v2")
        .arg("-uall")
        .arg("--branch")
        .output()
        .map_err(|e| format!("failed to run git status: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if stderr.contains("not a git repository") {
            return Ok(empty_summary());
        }
        return Err(format!("git status failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(parse_git_status(&stdout))
}

#[tauri::command]
pub fn git_status_files(state: State<'_, AppState>) -> Result<Vec<GitFileStatus>, String> {
    let Some(root) = workspace_root(&state) else {
        return Ok(Vec::new());
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("status")
        .arg("--porcelain=v2")
        .arg("-uall")
        .output()
        .map_err(|e| format!("failed to run git status: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if stderr.contains("not a git repository") {
            return Ok(Vec::new());
        }
        return Err(format!("git status failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(parse_git_status_files(&stdout))
}

#[tauri::command]
pub fn git_stage_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("add")
        .arg("--")
        .arg(&path)
        .output()
        .map_err(|e| format!("failed to run git add: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git add failed: {}", stderr.trim()));
    }

    Ok(())
}

#[tauri::command]
pub fn git_unstage_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("restore")
        .arg("--staged")
        .arg("--")
        .arg(&path)
        .output()
        .map_err(|e| format!("failed to run git restore: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git restore failed: {}", stderr.trim()));
    }

    Ok(())
}

#[tauri::command]
pub fn git_stage_all(state: State<'_, AppState>) -> Result<(), String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("add")
        .arg("-A")
        .output()
        .map_err(|e| format!("failed to run git add -A: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git add -A failed: {}", stderr.trim()));
    }

    Ok(())
}

#[tauri::command]
pub fn git_unstage_all(state: State<'_, AppState>) -> Result<(), String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("restore")
        .arg("--staged")
        .arg(".")
        .output()
        .map_err(|e| format!("failed to run git restore --staged: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git restore --staged failed: {}", stderr.trim()));
    }

    Ok(())
}

#[tauri::command]
pub fn git_commit(state: State<'_, AppState>, message: String) -> Result<String, String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let message = message.trim();
    if message.is_empty() {
        return Err("Commit message is required".to_string());
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("commit")
        .arg("-m")
        .arg(message)
        .output()
        .map_err(|e| format!("failed to run git commit: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git commit failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

#[tauri::command]
pub fn git_commit_preflight(state: State<'_, AppState>) -> Result<CommitPreflightResult, String> {
    let Some(root) = workspace_root(&state) else {
        return Ok(CommitPreflightResult {
            can_commit: false,
            is_repo: false,
            branch: None,
            is_detached: false,
            has_upstream: false,
            has_conflicts: false,
            staged_count: 0,
            error_message: Some("No workspace open".to_string()),
        });
    };

    // Check if it's a git repo
    let is_repo = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("rev-parse")
        .arg("--git-dir")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !is_repo {
        return Ok(CommitPreflightResult {
            can_commit: false,
            is_repo: false,
            branch: None,
            is_detached: false,
            has_upstream: false,
            has_conflicts: false,
            staged_count: 0,
            error_message: Some("Not a Git repository".to_string()),
        });
    }

    // Get current branch (HEAD for detached)
    let branch_output = run_git(&root, &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_else(|_| "HEAD".to_string());
    let branch_name = branch_output.trim();
    let is_detached = branch_name == "HEAD";
    let branch = if is_detached { None } else { Some(branch_name.to_string()) };

    // Check for upstream
    let has_upstream = if !is_detached {
        run_git(&root, &["rev-parse", "--abbrev-ref", &format!("{}@{{upstream}}", branch_name)])
            .is_ok()
    } else {
        false
    };

    // Check for conflicts
    let status_output = run_git(&root, &["status", "--porcelain=v2"])
        .unwrap_or_default();
    let has_conflicts = status_output.lines().any(|l| l.starts_with("u "));

    // Count staged files
    let staged_count = status_output
        .lines()
        .filter(|l| {
            if let Some(rest) = l.strip_prefix("1 ").or_else(|| l.strip_prefix("2 ")) {
                let xy = rest.split_whitespace().next().unwrap_or("..");
                xy.chars().next().unwrap_or('.') != '.'
            } else {
                false
            }
        })
        .count() as u32;

    // Determine if we can commit
    let mut error_message = None;
    let can_commit = if has_conflicts {
        error_message = Some("Resolve merge conflicts before committing".to_string());
        false
    } else if staged_count == 0 {
        error_message = Some("No staged changes to commit".to_string());
        false
    } else if is_detached {
        error_message = Some("Warning: HEAD is detached. Commits may be lost.".to_string());
        true // Allow but warn
    } else {
        true
    };

    Ok(CommitPreflightResult {
        can_commit,
        is_repo: true,
        branch,
        is_detached,
        has_upstream,
        has_conflicts,
        staged_count,
        error_message,
    })
}

#[tauri::command]
pub fn git_push(state: State<'_, AppState>) -> Result<String, String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .arg("push")
        .output()
        .map_err(|e| format!("failed to run git push: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git push failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

#[tauri::command]
pub fn git_diff(state: State<'_, AppState>, path: String, staged: bool) -> Result<String, String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(&root).arg("diff").arg("--no-color");

    if staged {
        cmd.arg("--staged");
    }

    let output = cmd
        .arg("--")
        .arg(&path)
        .output()
        .map_err(|e| format!("failed to run git diff: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git diff failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    Ok(stdout)
}

#[tauri::command]
pub fn git_generate_commit_message(state: State<'_, AppState>) -> Result<String, String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let ctx = collect_changes_for_message(&root)?;

    if ctx.files.is_empty() {
        return Err("No changes to commit".to_string());
    }

    let message = if ctx.files.len() == 1 {
        format!("Update {}", ctx.files[0])
    } else if ctx.files.len() <= 3 {
        format!("Update {}", ctx.files.join(", "))
    } else {
        format!("Update {} files", ctx.files.len())
    };

    Ok(message)
}

#[tauri::command]
pub async fn git_generate_commit_message_ai(
    state: State<'_, AppState>,
    model_id: String,
) -> Result<String, String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let ctx = collect_changes_for_message(&root)?;
    if ctx.files.is_empty() {
        return Err("No changes to commit".to_string());
    }

    let file_list = ctx.files
        .iter()
        .map(|f| format!("- {}", f))
        .collect::<Vec<_>>()
        .join("\n");

    let new_files_section = if ctx.new_file_content.is_empty() {
        String::new()
    } else {
        format!("\nNEW FILES CONTENT:\n{}", ctx.new_file_content)
    };

    // Build context sections
    let branch_section = ctx.branch
        .as_ref()
        .map(|b| format!("BRANCH: {}\n", b))
        .unwrap_or_default();

    let stats_section = if !ctx.diff_stat.is_empty() {
        format!("CHANGE STATS:\n{}\n", ctx.diff_stat.trim())
    } else {
        String::new()
    };

    let style_section = if let Some(ref last_msg) = ctx.last_commit_message {
        let recent = ctx.recent_commits.join("\n");
        format!(
            "RECENT COMMITS (for style reference):\n{}\n\nLAST COMMIT MESSAGE:\n{}\n",
            recent, last_msg
        )
    } else {
        String::new()
    };

    let prompt = format!(
        r#"Generate a Git commit message for these changes. Use Conventional Commits: type(scope): description

Types: feat, fix, refactor, docs, style, test, chore, perf
Keep under 72 chars. Imperative mood. No period at end.
Match the style of recent commits if possible.

{branch}FILES ({stage}):
{files}
{stats}{style}{new_files}
DIFF:
{diff}

Respond with ONLY the commit message, nothing else."#,
        branch = branch_section,
        stage = if ctx.staged { "staged" } else { "unstaged" },
        files = file_list,
        stats = stats_section,
        style = style_section,
        new_files = new_files_section,
        diff = ctx.diff
    );

    let workspace_info = crate::blade_ws_client::WorkspaceInfo {
        root: root.clone(),
        project_id: None,
        active_file: None,
        cursor_position: None,
        open_files: Vec::new(),
    };

    let available_models = load_available_models(&state).await;
    let resolved_model_id = resolve_model_id(&available_models, &model_id);

    // Use shared WebSocket connection manager from AppState
    let ws_manager = state.ws_connection.clone();
    
    // Connect (or reuse existing connection)
    let mut ws_rx = ws_manager.ensure_connected().await
        .map_err(|e| format!("WebSocket connection failed: {}", e))?;
    
    // Wait for authentication
    let mut authenticated = false;
    while let Some(event) = ws_rx.recv().await {
        if let crate::blade_ws_client::BladeWsEvent::Connected { .. } = event {
            authenticated = true;
            break;
        }
        if let crate::blade_ws_client::BladeWsEvent::Error { message, .. } = event {
            return Err(format!("Authentication failed: {}", message));
        }
    }
    
    if !authenticated {
        return Err("WebSocket authentication timeout".to_string());
    }

    // Send the commit message generation request
    ws_manager
        .send_message(None, resolved_model_id, prompt, None, Some(workspace_info))
        .await
        .map_err(|e| format!("Failed to send message: {}", e))?;

    // Collect response
    let mut content = String::new();
    while let Some(event) = ws_rx.recv().await {
        match event {
            crate::blade_ws_client::BladeWsEvent::TextChunk(chunk) => {
                content.push_str(&chunk);
            }
            crate::blade_ws_client::BladeWsEvent::ChatDone { .. } => break,
            crate::blade_ws_client::BladeWsEvent::Error { message, .. } => {
                return Err(format!("AI generation failed: {}", message));
            }
            crate::blade_ws_client::BladeWsEvent::Disconnected => break,
            _ => {}
        }
    }

    let message = content.lines().next().unwrap_or("").trim();
    if message.is_empty() {
        Err("AI returned empty response".to_string())
    } else {
        Ok(message.to_string())
    }
}
