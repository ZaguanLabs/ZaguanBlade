use serde::Serialize;
use std::collections::HashMap;
use std::process::Command;
use tauri::State;

use crate::blade_client::{BladeClient, WorkspaceInfo};
use crate::AppState;

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

fn collect_changes_for_message(root: &str) -> Result<(Vec<String>, String, bool), String> {
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

    let diff_args = if staged {
        vec!["diff", "--cached", "--unified=3"]
    } else {
        vec!["diff", "--unified=3"]
    };
    let mut diff = run_git(root, &diff_args)?;

    const DIFF_LIMIT: usize = 8000;
    if diff.len() > DIFF_LIMIT {
        diff.truncate(DIFF_LIMIT);
        diff.push_str("\n...diff truncated...");
    }

    Ok((files, diff, staged))
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

    let (files, _diff, _staged) = collect_changes_for_message(&root)?;

    if files.is_empty() {
        return Ok("Update files".to_string());
    }

    let message = if files.len() == 1 {
        format!("Update {}", files[0])
    } else if files.len() <= 3 {
        format!("Update {}", files.join(", "))
    } else {
        format!("Update {} files", files.len())
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

    let (files, diff, staged) = collect_changes_for_message(&root)?;
    if files.is_empty() {
        return Ok("Update files".to_string());
    }

    let file_list = files
        .iter()
        .map(|f| format!("- {}", f))
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        r#"You are a Git commit message expert. Generate a professional commit message for the following changes.

RULES:
1. Use Conventional Commits format: type(scope): description
2. Types: feat, fix, refactor, docs, style, test, chore, perf
3. Scope is optional but helpful (e.g., component name, file area)
4. Description should be imperative mood, lowercase, no period at end
5. Keep the first line under 72 characters
6. If the changes are significant, add a blank line and a brief body (2-3 lines max)

CHANGED FILES ({stage}):
{files}

DIFF:
{diff}

Respond with ONLY the commit message, nothing else. No quotes, no explanation."#,
        stage = if staged { "staged" } else { "unstaged" },
        files = file_list,
        diff = diff
    );

    let (blade_url, api_key) = {
        let config = state.config.lock().unwrap();
        (config.blade_url.clone(), config.api_key.clone())
    };

    let http_client = reqwest::Client::new();
    let blade_client = BladeClient::new(blade_url, http_client, api_key);

    let workspace = WorkspaceInfo {
        root: root.clone(),
        project_id: None,
        active_file: None,
        cursor_position: None,
        open_files: Vec::new(),
    };

    let mut events = blade_client
        .send_message(None, model_id, prompt, workspace, HashMap::new())
        .await?;

    let mut content = String::new();
    while let Some(event) = events.recv().await {
        match event {
            crate::blade_client::BladeEvent::Text(chunk) => {
                content.push_str(&chunk);
            }
            crate::blade_client::BladeEvent::Done { .. } => break,
            crate::blade_client::BladeEvent::Error {
                message, details, ..
            } => {
                return Err(format!("AI generation failed: {} {}", message, details));
            }
            _ => {}
        }
    }

    let message = content.lines().next().unwrap_or("Update files").trim();
    if message.is_empty() {
        Ok("Update files".to_string())
    } else {
        Ok(message.to_string())
    }
}
