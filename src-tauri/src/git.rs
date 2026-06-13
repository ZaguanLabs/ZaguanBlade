use serde::Serialize;
use serde_json::json;
use std::collections::BTreeSet;
use tauri::{AppHandle, Manager, State};
use tokio::process::Command as TokioCommand;

use crate::{
    app_state::AppState,
    blade_ws_client::LocalConversationState,
    config::normalize_openai_compat_url,
    models::{catalog, registry},
    providers,
};

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
    pub has_upstream: bool,
    pub has_remote: bool,
    pub can_push: bool,
    pub dirty: bool,
}

fn is_local_git_status_excluded_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    let trimmed = normalized.trim_start_matches("./");
    trimmed == ".zblade"
        || trimmed.starts_with(".zblade/")
        || trimmed.ends_with("/.zblade")
        || trimmed.contains("/.zblade/")
        || trimmed == ".pnpm-store"
        || trimmed.starts_with(".pnpm-store/")
        || trimmed.ends_with("/.pnpm-store")
        || trimmed.contains("/.pnpm-store/")
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
pub struct GitStatusSnapshot {
    pub summary: GitStatusSummary,
    pub files: Vec<GitFileStatus>,
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
    pub error_key: Option<String>,
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
        has_upstream: false,
        has_remote: false,
        can_push: false,
        dirty: false,
    }
}

fn empty_snapshot() -> GitStatusSnapshot {
    GitStatusSnapshot {
        summary: empty_summary(),
        files: Vec::new(),
    }
}

async fn generate_commit_message_via_openai_compat(
    api_config: &crate::config::ApiConfig,
    model_id: &str,
    prompt: &str,
) -> Result<String, String> {
    let model_name = model_id.strip_prefix("openai-compat/").unwrap_or(model_id);
    let openai_compat_base_url = normalize_openai_compat_url(&api_config.openai_compat_url);
    let url = format!("{}/v1/chat/completions", openai_compat_base_url);

    let body = build_openai_compat_commit_message_body(model_name, prompt);

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("OpenAI-compatible request failed: {}", e))?;

    let status = response.status();
    let payload = response
        .text()
        .await
        .map_err(|e| format!("Failed to read OpenAI-compatible response: {}", e))?;

    if !status.is_success() {
        return Err(format!(
            "OpenAI-compatible server returned HTTP {}: {}",
            status,
            payload.chars().take(500).collect::<String>()
        ));
    }

    let parsed: serde_json::Value = serde_json::from_str(&payload)
        .map_err(|e| format!("Invalid OpenAI-compatible response: {}", e))?;

    let raw = parsed
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let message = extract_commit_message(raw);
    if message.is_empty() {
        Err("AI returned empty response".to_string())
    } else {
        Ok(message)
    }
}

fn build_openai_compat_commit_message_body(model_name: &str, prompt: &str) -> serde_json::Value {
    json!({
        "model": model_name,
        "messages": [
            {
                "role": "user",
                "content": prompt
            }
        ],
        "stream": false
    })
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
        has_upstream: false,
        has_remote: false,
        can_push: false,
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
            if rest.strip_prefix("branch.upstream ").is_some() {
                summary.has_upstream = true;
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
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let xy = parts[0];
            let path = parts.last().unwrap_or(&"");
            if is_local_git_status_excluded_path(path) {
                continue;
            }
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
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 3 {
                continue;
            }
            let xy = parts[0];
            let new_path = parts.get(parts.len().saturating_sub(2)).unwrap_or(&"");
            if is_local_git_status_excluded_path(new_path) {
                continue;
            }
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
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }
            let xy = parts[0];
            let path = parts.last().unwrap_or(&"");
            if is_local_git_status_excluded_path(path) {
                continue;
            }
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

        if let Some(rest) = line.strip_prefix("? ") {
            if is_local_git_status_excluded_path(rest.trim()) {
                continue;
            }
            summary.untracked_count += 1;
        }
    }

    summary.changed_count = summary.staged_count + summary.unstaged_count + summary.untracked_count;
    summary.dirty = summary.changed_count > 0;

    summary
}

fn status_has_conflicts(output: &str) -> bool {
    output.lines().any(|line| {
        let Some(rest) = line.strip_prefix("u ") else {
            return false;
        };
        let path = rest.split_whitespace().last().unwrap_or("");
        !is_local_git_status_excluded_path(path)
    })
}

fn commit_preflight_from_status(output: &str, has_commits: bool) -> CommitPreflightResult {
    let summary = parse_git_status(output);
    let has_conflicts = status_has_conflicts(output);
    let is_detached = summary.branch.is_none() && has_commits;
    let staged_count = summary.staged_count;

    let mut error_message = None;
    let can_commit = if has_conflicts {
        error_message = Some("Resolve merge conflicts before committing".to_string());
        false
    } else if staged_count == 0 {
        error_message = Some("No staged changes to commit".to_string());
        false
    } else if is_detached {
        error_message = Some("Warning: HEAD is detached. Commits may be lost.".to_string());
        true
    } else {
        true
    };

    CommitPreflightResult {
        can_commit,
        is_repo: true,
        branch: summary.branch,
        is_detached,
        has_upstream: summary.has_upstream,
        has_conflicts,
        staged_count,
        error_message,
        error_key: None,
    }
}

fn workspace_root(state: &State<'_, AppState>) -> Option<String> {
    let ws = state.workspace.lock().unwrap();
    ws.workspace
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
}

fn open_repo(state: &State<'_, AppState>) -> Result<gix::Repository, String> {
    let root = workspace_root(state).ok_or("Not a Git repository")?;
    gix::discover(&root).map_err(|e| format!("Failed to discover git repository: {}", e))
}

fn normalize_remote_url(url: &str) -> String {
    if url.starts_with("git@") {
        let without_prefix = url.trim_start_matches("git@");
        let normalized = without_prefix.replacen(':', "/", 1);
        let trimmed = normalized.trim_end_matches(".git");
        format!("https://{}", trimmed)
    } else if url.starts_with("https://") || url.starts_with("http://") {
        url.trim_end_matches(".git").to_string()
    } else {
        url.to_string()
    }
}

fn collect_git_status_snapshot_cli(root: &str) -> Result<GitStatusSnapshot, String> {
    let stdout = match git_status_output(root)? {
        Some(output) => output,
        None => return Ok(empty_snapshot()),
    };

    let mut summary = parse_git_status(&stdout);
    summary.has_remote = git_has_remote(root);
    let has_commits = git_has_commits(root);
    let upstream_ref_exists = summary.has_upstream && git_upstream_ref_exists(root);
    if summary.has_upstream && !upstream_ref_exists {
        summary.has_upstream = false;
    }
    summary.can_push = compute_can_push(&summary, has_commits, upstream_ref_exists);

    Ok(GitStatusSnapshot {
        summary,
        files: parse_git_status_files(&stdout),
    })
}

fn collect_git_status_snapshot(state: &State<'_, AppState>) -> Result<GitStatusSnapshot, String> {
    let Some(root) = workspace_root(state) else {
        return Ok(empty_snapshot());
    };

    collect_git_status_snapshot_cli(&root)
}

async fn run_git_async_output(
    root: &str,
    args: &[&str],
    context: &str,
) -> Result<std::process::Output, String> {
    let output = TokioCommand::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .await
        .map_err(|e| format!("failed to run git {}: {}", context, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git {} failed: {}", context, stderr.trim()));
    }

    Ok(output)
}

async fn run_git_async(root: &str, args: &[&str], context: &str) -> Result<String, String> {
    let output = run_git_async_output(root, args, context).await?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[allow(clippy::disallowed_methods, clippy::disallowed_types)]
fn git_status_output(root: &str) -> Result<Option<String>, String> {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .arg("status")
        .arg("--porcelain=v2")
        .arg("-uall")
        .arg("--branch")
        .output()
        .map_err(|e| format!("failed to run git status: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if stderr.contains("not a git repository") {
            return Ok(None);
        }
        return Err(format!("git status failed: {}", stderr.trim()));
    }

    Ok(Some(String::from_utf8_lossy(&output.stdout).to_string()))
}

#[allow(clippy::disallowed_methods, clippy::disallowed_types)]
fn run_git(root: &str, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git")
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

fn git_has_remote(root: &str) -> bool {
    run_git(root, &["remote"][..])
        .map(|output| output.lines().any(|line| !line.trim().is_empty()))
        .unwrap_or(false)
}

fn git_has_commits(root: &str) -> bool {
    run_git(root, &["rev-parse", "--verify", "HEAD"][..]).is_ok()
}

fn git_upstream_ref_exists(root: &str) -> bool {
    run_git(root, &["rev-parse", "--verify", "@{upstream}"][..]).is_ok()
}

fn compute_can_push(
    summary: &GitStatusSummary,
    has_commits: bool,
    upstream_ref_exists: bool,
) -> bool {
    if summary.branch.is_none() || !summary.has_remote || !has_commits {
        return false;
    }

    if summary.has_upstream {
        summary.ahead > 0 || !upstream_ref_exists
    } else {
        true
    }
}

fn default_push_remote(root: &str, branch: &str) -> Option<String> {
    let branch_remote_key = format!("branch.{}.remote", branch);
    if let Ok(remote) = run_git(root, &["config", "--get", branch_remote_key.as_str()][..]) {
        let remote = remote.trim();
        if !remote.is_empty() {
            return Some(remote.to_string());
        }
    }

    let remotes = run_git(root, &["remote"][..]).ok()?;
    if remotes.lines().any(|line| line.trim() == "origin") {
        return Some("origin".to_string());
    }

    remotes
        .lines()
        .map(str::trim)
        .find(|remote| !remote.is_empty())
        .map(ToString::to_string)
}

fn find_subject_start_in_line(line: &str) -> Option<usize> {
    const TYPES: [&str; 11] = [
        "feat", "fix", "refactor", "docs", "style", "test", "chore", "perf", "build", "ci",
        "revert",
    ];

    let lowercase = line.to_ascii_lowercase();
    let mut best: Option<usize> = None;

    for typ in TYPES {
        for marker in [format!("{}:", typ), format!("{}(", typ)] {
            let mut search_from = 0usize;
            while let Some(rel_idx) = lowercase[search_from..].find(&marker) {
                let idx = search_from + rel_idx;

                // Prefer starts that look like token boundaries.
                let boundary_ok = if idx == 0 {
                    true
                } else {
                    let prev = lowercase[..idx].chars().next_back().unwrap_or(' ');
                    !prev.is_ascii_alphanumeric() && prev != '_' && prev != '-'
                };

                if boundary_ok {
                    best = Some(best.map_or(idx, |current| current.min(idx)));
                    break;
                }

                search_from = idx + marker.len();
            }
        }
    }

    best
}

fn find_last_subject_occurrence(lines: &[&str]) -> Option<(usize, usize)> {
    let mut last: Option<(usize, usize)> = None;
    for (line_idx, line) in lines.iter().enumerate() {
        if let Some(col_idx) = find_subject_start_in_line(line) {
            last = Some((line_idx, col_idx));
        }
    }
    last
}

fn extract_commit_message(raw: &str) -> String {
    let mut normalized = raw
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string();
    if normalized.is_empty() {
        return String::new();
    }

    // If model wrapped output in a code fence, prefer fenced content.
    if let Some(start) = normalized.find("```") {
        let after_start = &normalized[start + 3..];
        let fence_content = if let Some(first_newline) = after_start.find('\n') {
            &after_start[first_newline + 1..]
        } else {
            after_start
        };

        if let Some(end) = fence_content.find("```") {
            normalized = fence_content[..end].trim().to_string();
        }
    }

    let lines: Vec<&str> = normalized.lines().map(str::trim_end).collect();
    if lines.is_empty() {
        return String::new();
    }

    let mut selected: Vec<String> =
        if let Some((line_idx, col_idx)) = find_last_subject_occurrence(&lines) {
            let mut out: Vec<String> = Vec::with_capacity(lines.len().saturating_sub(line_idx));
            out.push(lines[line_idx][col_idx..].trim_start().to_string());
            out.extend(lines.iter().skip(line_idx + 1).map(|line| line.to_string()));
            out
        } else {
            // Fallback: if no recognizable conventional subject was found,
            // use only the trailing paragraph (avoid taking the entire reasoning transcript).
            let end = lines.iter().rposition(|line| !line.trim().is_empty());
            let Some(end_idx) = end else {
                return String::new();
            };

            let mut start_idx = end_idx;
            while start_idx > 0 && !lines[start_idx - 1].trim().is_empty() {
                start_idx -= 1;
            }

            lines[start_idx..=end_idx]
                .iter()
                .map(|line| line.to_string())
                .collect()
        };

    while selected.first().is_some_and(|line| line.trim().is_empty()) {
        selected.remove(0);
    }
    while selected.last().is_some_and(|line| line.trim().is_empty()) {
        selected.pop();
    }

    selected
        .join("\n")
        .trim()
        .trim_matches('`')
        .trim_matches('"')
        .trim()
        .to_string()
}

struct CommitContext {
    files: Vec<String>,
    diff: String,
    diff_stat: String,
    untracked_files: Vec<String>,
    new_file_content: String,
    staged: bool,
    branch: Option<String>,
    last_commit_message: Option<String>,
    recent_commits: Vec<String>,
}

async fn load_models(state: &State<'_, AppState>) -> Vec<registry::ModelInfo> {
    let config = { state.config.lock().unwrap().clone() };
    catalog::list_all_models(&config).await
}

fn resolve_model_id(models: &[registry::ModelInfo], requested_id: &str) -> String {
    if let Some(idx) = catalog::resolve_model_index(models, requested_id) {
        providers::resolve_model_selection(models, idx).model_id_for_request
    } else {
        requested_id.to_string()
    }
}

async fn generate_commit_message_via_ollama(
    api_config: &crate::config::ApiConfig,
    model_id: &str,
    prompt: &str,
) -> Result<String, String> {
    let model_name = model_id.strip_prefix("ollama/").unwrap_or(model_id);
    let url = format!("{}/api/chat", api_config.ollama_url.trim_end_matches('/'));

    let body = build_ollama_commit_message_body(model_name, prompt);

    let client = reqwest::Client::new();
    let mut request = client.post(&url).json(&body);

    if api_config.ollama_cloud_enabled && !api_config.ollama_cloud_api_key.is_empty() {
        request = request.header(
            "Authorization",
            format!("Bearer {}", api_config.ollama_cloud_api_key),
        );
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("Ollama request failed: {}", e))?;

    let status = response.status();
    let payload = response
        .text()
        .await
        .map_err(|e| format!("Failed to read Ollama response: {}", e))?;

    if !status.is_success() {
        return Err(format!(
            "Ollama returned HTTP {}: {}",
            status,
            payload.chars().take(500).collect::<String>()
        ));
    }

    let parsed: serde_json::Value =
        serde_json::from_str(&payload).map_err(|e| format!("Invalid Ollama response: {}", e))?;

    if let Some(err) = parsed.get("error").and_then(|v| v.as_str()) {
        return Err(format!("Ollama error: {}", err));
    }

    let raw = parsed
        .pointer("/message/content")
        .and_then(|v| v.as_str())
        .or_else(|| parsed.get("response").and_then(|v| v.as_str()))
        .unwrap_or("");

    let message = extract_commit_message(raw);
    if message.is_empty() {
        Err("AI returned empty response".to_string())
    } else {
        Ok(message)
    }
}

fn build_ollama_commit_message_body(model_name: &str, prompt: &str) -> serde_json::Value {
    json!({
        "model": model_name,
        "messages": [
            {
                "role": "user",
                "content": prompt
            }
        ],
        "stream": false
    })
}

fn collect_git_paths(output: &str) -> Vec<String> {
    output
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty() && !is_local_git_status_excluded_path(line))
        .collect()
}

fn extend_unique_paths(target: &mut Vec<String>, source: impl IntoIterator<Item = String>) {
    let mut seen: BTreeSet<String> = target.iter().cloned().collect();
    for path in source {
        if seen.insert(path.clone()) {
            target.push(path);
        }
    }
}

fn collect_changes_for_message(root: &str) -> Result<CommitContext, String> {
    let staged_files = collect_git_paths(&run_git(root, &["diff", "--cached", "--name-only"][..])?);
    let mut files = staged_files.clone();

    let staged = !files.is_empty();
    let unstaged_files = collect_git_paths(&run_git(root, &["diff", "--name-only"][..])?);

    if !staged {
        files = unstaged_files.clone();
    }

    let untracked = collect_git_paths(&run_git(
        root,
        &["ls-files", "--others", "--exclude-standard"][..],
    )?);

    if !staged {
        files.extend(untracked.clone());
    }

    let added_files_output = if staged {
        run_git(
            root,
            &["diff", "--cached", "--name-only", "--diff-filter=A"][..],
        )?
    } else {
        run_git(root, &["diff", "--name-only", "--diff-filter=A"][..])?
    };
    let mut preview_files = collect_git_paths(&added_files_output);
    extend_unique_paths(&mut preview_files, untracked.iter().cloned());

    let mut diff = if staged {
        run_git(root, &["diff", "--cached", "--unified=3"][..])?
    } else {
        run_git(root, &["diff", "--unified=3"][..])?
    };

    // Get diff stats (insertions/deletions summary)
    let diff_stat = if staged {
        run_git(root, &["diff", "--cached", "--stat"][..])
    } else {
        run_git(root, &["diff", "--stat"][..])
    }
    .unwrap_or_default();

    // Get current branch, last commit, and recent commits via gix if possible
    let (branch, last_commit_message, recent_commits) = {
        let repo = gix::discover(root).ok();
        if let Some(repo) = repo {
            let branch = repo.head().ok().and_then(|h| {
                if h.is_detached() {
                    None
                } else {
                    h.referent_name().map(|n| n.shorten().to_string())
                }
            });
            let head_id = repo.head_id().ok();
            let last_commit_message = head_id.as_ref().and_then(|id| {
                id.object()
                    .ok()
                    .map(|o| {
                        let c = o.into_commit();
                        c.decode().ok().map(|cr| cr.message().summary().to_string())
                    })
                    .flatten()
            });
            let recent_commits: Vec<String> = head_id
                .map(|id| {
                    id.ancestors()
                        .all()
                        .ok()
                        .map(|walk| {
                            walk.take(5)
                                .filter_map(|info| {
                                    let info = info.ok()?;
                                    let commit = info.id().object().ok()?.into_commit();
                                    let cr = commit.decode().ok()?;
                                    let short = &info.id().to_string()[..7];
                                    Some(format!("{} {}", short, cr.message().summary()))
                                })
                                .collect()
                        })
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            (branch, last_commit_message, recent_commits)
        } else {
            // Fallback to CLI
            let branch = run_git(root, &["rev-parse", "--abbrev-ref", "HEAD"][..])
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s != "HEAD");
            let last_commit_message = run_git(root, &["log", "-1", "--format=%B"][..])
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let recent_commits: Vec<String> = run_git(root, &["log", "--oneline", "-5"][..])
                .unwrap_or_default()
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            (branch, last_commit_message, recent_commits)
        }
    };

    let mut new_file_content = String::new();
    const MAX_PREVIEW_PER_FILE: usize = 2000;
    const MAX_TOTAL_PREVIEW: usize = 6000;

    for file in preview_files {
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
        untracked_files: untracked,
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
            if is_local_git_status_excluded_path(&path) {
                continue;
            }
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
            if is_local_git_status_excluded_path(new_path) {
                continue;
            }
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
            if is_local_git_status_excluded_path(&path) {
                continue;
            }
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
            if is_local_git_status_excluded_path(&path) {
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn run_git_for_test(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("run git command");

        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn porcelain_status_parser_matches_cli_changed_files() {
        let output = "\
# branch.oid 64b114a287e09f5ad611a0b4b8558cdd546a19f3
# branch.head main
# branch.upstream origin/main
# branch.ab +0 -0
1 .M N... 100644 100644 100644 94b82ab759dd6ee3290523c156281955b563b43c 94b82ab759dd6ee3290523c156281955b563b43c src-tauri/src/ai_workflow.rs
1 .M N... 100644 100644 100644 205850851942178d16e6992efa810ed1427dfee6 205850851942178d16e6992efa810ed1427dfee6 src/index.css
? scratch.txt
";

        let summary = parse_git_status(output);
        let files = parse_git_status_files(output);

        assert_eq!(summary.changed_count, 3);
        assert_eq!(summary.unstaged_count, 2);
        assert_eq!(summary.untracked_count, 1);
        assert!(summary.has_upstream);
        assert_eq!(summary.ahead, 0);
        assert_eq!(summary.behind, 0);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "src-tauri/src/ai_workflow.rs");
        assert_eq!(files[2].status_code, "??");
    }

    #[test]
    fn porcelain_status_parser_handles_branch_without_upstream() {
        let output = "\
# branch.oid 64b114a287e09f5ad611a0b4b8558cdd546a19f3
# branch.head main
";

        let summary = parse_git_status(output);

        assert_eq!(summary.branch.as_deref(), Some("main"));
        assert!(!summary.has_upstream);
        assert_eq!(summary.ahead, 0);
        assert_eq!(summary.behind, 0);
    }

    #[test]
    fn commit_preflight_allows_unborn_branch_with_staged_changes() {
        let output = "\
# branch.oid (initial)
# branch.head main
1 A. N... 000000 100644 100644 0000000000000000000000000000000000000000 ce013625030ba8dba906f756967f9e9ca394464a README.md
";

        let preflight = commit_preflight_from_status(output, false);

        assert!(preflight.can_commit);
        assert!(preflight.is_repo);
        assert_eq!(preflight.branch.as_deref(), Some("main"));
        assert!(!preflight.is_detached);
        assert_eq!(preflight.staged_count, 1);
        assert!(preflight.error_message.is_none());
    }

    #[test]
    fn status_allows_publish_after_initial_commit_to_empty_remote() {
        let temp = tempfile::tempdir().expect("create temp dir");
        let remote = temp.path().join("remote.git");
        let worktree = temp.path().join("worktree");

        let init_output = std::process::Command::new("git")
            .arg("init")
            .arg("--bare")
            .arg(&remote)
            .output()
            .expect("init bare remote");
        assert!(
            init_output.status.success(),
            "git init --bare failed: {}",
            String::from_utf8_lossy(&init_output.stderr)
        );

        let clone_output = std::process::Command::new("git")
            .arg("clone")
            .arg(&remote)
            .arg(&worktree)
            .output()
            .expect("clone bare remote");
        assert!(
            clone_output.status.success(),
            "git clone failed: {}",
            String::from_utf8_lossy(&clone_output.stderr)
        );

        std::fs::write(worktree.join("README.md"), "hello\n").expect("write README");
        run_git_for_test(&worktree, &["add", "README.md"]);
        run_git_for_test(
            &worktree,
            &[
                "-c",
                "user.name=Test User",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "init",
            ],
        );

        let snapshot = collect_git_status_snapshot_cli(worktree.to_str().expect("utf-8 worktree"))
            .expect("collect git status");

        assert!(snapshot.summary.is_repo);
        assert!(snapshot.summary.has_remote);
        assert!(!snapshot.summary.has_upstream);
        assert_eq!(snapshot.summary.ahead, 0);
        assert!(snapshot.summary.can_push);
    }

    #[test]
    fn local_ai_commit_message_requests_do_not_include_system_prompt() {
        for body in [
            build_ollama_commit_message_body("llama3.2", "Generate a commit message"),
            build_openai_compat_commit_message_body("local-model", "Generate a commit message"),
        ] {
            let messages = body
                .get("messages")
                .and_then(|value| value.as_array())
                .expect("messages array");

            assert_eq!(messages.len(), 1);
            assert_eq!(
                messages
                    .first()
                    .and_then(|message| message.get("role"))
                    .and_then(|role| role.as_str()),
                Some("user")
            );
            assert!(messages
                .iter()
                .all(
                    |message| message.get("role").and_then(|role| role.as_str()) != Some("system")
                ));
        }
    }
}

#[tauri::command]
pub async fn git_status_summary(
    _state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<GitStatusSummary, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        collect_git_status_snapshot(&state).map(|snapshot| snapshot.summary)
    })
    .await
    .map_err(|e| format!("git status summary task failed: {}", e))?
}

#[tauri::command]
pub async fn git_status_files(
    _state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<Vec<GitFileStatus>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        collect_git_status_snapshot(&state).map(|snapshot| snapshot.files)
    })
    .await
    .map_err(|e| format!("git status files task failed: {}", e))?
}

#[tauri::command]
pub async fn git_status(
    _state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<GitStatusSnapshot, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        collect_git_status_snapshot(&state)
    })
    .await
    .map_err(|e| format!("git status task failed: {}", e))?
}

#[tauri::command]
pub async fn git_stage_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    if is_local_git_status_excluded_path(&path) {
        return Ok(());
    }

    run_git_async(&root, &["add", "--", &path][..], "add").await?;
    Ok(())
}

#[tauri::command]
pub async fn git_unstage_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    run_git_async(&root, &["restore", "--staged", "--", &path][..], "restore").await?;
    Ok(())
}

#[tauri::command]
pub async fn git_stage_all(state: State<'_, AppState>) -> Result<(), String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    run_git_async(&root, &["add", "-A"][..], "add -A").await?;

    let _ = TokioCommand::new("git")
        .arg("-C")
        .arg(&root)
        .arg("rm")
        .arg("-r")
        .arg("--cached")
        .arg("--ignore-unmatch")
        .arg("--")
        .arg(".zblade")
        .output()
        .await;

    Ok(())
}

#[tauri::command]
pub async fn git_unstage_all(state: State<'_, AppState>) -> Result<(), String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    run_git_async(&root, &["restore", "--staged", "."][..], "restore --staged").await?;
    Ok(())
}

#[tauri::command]
pub async fn git_commit(state: State<'_, AppState>, message: String) -> Result<String, String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let message = message.trim().to_string();
    if message.is_empty() {
        return Err("Commit message is required".to_string());
    }

    run_git_async(&root, &["commit", "-m", &message][..], "commit").await
}

#[tauri::command]
pub async fn git_commit_preflight(
    _state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<CommitPreflightResult, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let root = match workspace_root(&state) {
            Some(r) => r,
            None => return Err("No workspace open".to_string()),
        };

        let Some(status_output) = git_status_output(&root)? else {
            return Ok(CommitPreflightResult {
                can_commit: false,
                is_repo: false,
                branch: None,
                is_detached: false,
                has_upstream: false,
                has_conflicts: false,
                staged_count: 0,
                error_message: Some("Not a Git repository".to_string()),
                error_key: Some("git.notRepository".to_string()),
            });
        };

        Ok(commit_preflight_from_status(
            &status_output,
            git_has_commits(&root),
        ))
    })
    .await
    .map_err(|e| format!("git commit preflight task failed: {}", e))?
}

#[tauri::command]
pub async fn git_push(state: State<'_, AppState>) -> Result<String, String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let snapshot = collect_git_status_snapshot_cli(&root)?;
    if snapshot.summary.has_upstream {
        return run_git_async(&root, &["push"][..], "push").await;
    }

    let branch = snapshot
        .summary
        .branch
        .ok_or_else(|| "Cannot push without an active branch".to_string())?;
    let remote = default_push_remote(&root, &branch)
        .ok_or_else(|| "Cannot push because no Git remote is configured".to_string())?;

    run_git_async(&root, &["push", "-u", &remote, &branch][..], "push -u").await
}

#[tauri::command]
pub async fn git_diff(
    state: State<'_, AppState>,
    path: String,
    staged: bool,
    _app_handle: AppHandle,
) -> Result<String, String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let mut cmd = TokioCommand::new("git");
    cmd.arg("-C").arg(&root).arg("diff").arg("--no-color");

    if staged {
        cmd.arg("--staged");
    }

    let output = cmd
        .arg("--")
        .arg(&path)
        .output()
        .await
        .map_err(|e| format!("failed to run git diff: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(format!("git diff failed: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

#[tauri::command]
pub async fn git_generate_commit_message(
    state: State<'_, AppState>,
    _app_handle: AppHandle,
) -> Result<String, String> {
    let Some(root) = workspace_root(&state) else {
        return Err("No workspace open".to_string());
    };

    let blocking_root = root.clone();
    let ctx = tokio::task::spawn_blocking(move || collect_changes_for_message(&blocking_root))
        .await
        .map_err(|e| format!("failed to collect commit context: {}", e))??;

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

    let blocking_root = root.clone();
    let ctx = tokio::task::spawn_blocking(move || collect_changes_for_message(&blocking_root))
        .await
        .map_err(|e| format!("failed to collect commit context: {}", e))??;
    if ctx.files.is_empty() {
        return Err("No changes to commit".to_string());
    }

    let file_list = ctx
        .files
        .iter()
        .map(|f| format!("- {}", f))
        .collect::<Vec<_>>()
        .join("\n");

    let new_files_section = if ctx.new_file_content.is_empty() {
        String::new()
    } else {
        format!("\nNEW FILES CONTENT:\n{}", ctx.new_file_content)
    };

    let untracked_section = if ctx.staged && !ctx.untracked_files.is_empty() {
        format!(
            "ADDITIONAL UNTRACKED FILES IN WORKTREE (not currently staged):\n{}\n",
            ctx.untracked_files
                .iter()
                .map(|f| format!("- {}", f))
                .collect::<Vec<_>>()
                .join("\n")
        )
    } else {
        String::new()
    };

    // Build context sections
    let branch_section = ctx
        .branch
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
        r#"Generate a Git commit message for these changes.
This is a text-only commit-message task. Do not request, propose, or emit tool calls.
Do not use commands, shells, filesystem tools, repository tools, or external actions.
Use only the provided diff and metadata below.

Use Conventional Commits format: type(scope): description

Allowed types: feat, fix, refactor, docs, style, test, chore, perf
Subject requirements: <=72 chars, imperative mood, no period at end.
You may include an optional body after a blank line for key details.
Match the style of recent commits if possible.

{branch}FILES ({stage}):
{files}
{stats}{style}{untracked}{new_files}
DIFF:
{diff}

Respond with ONLY the final commit message text (subject + optional body).
Do NOT include analysis, reasoning, explanations, or multiple options."#,
        branch = branch_section,
        stage = if ctx.staged { "staged" } else { "unstaged" },
        files = file_list,
        stats = stats_section,
        style = style_section,
        untracked = untracked_section,
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

    let available_models = load_models(&state).await;
    let resolved_model_id = resolve_model_id(&available_models, &model_id);
    let resolved_provider = catalog::resolve_model_index(&available_models, &model_id)
        .map(|idx| providers::resolve_model_selection(&available_models, idx).provider);

    if matches!(resolved_provider, Some(providers::ProviderId::Ollama))
        || resolved_model_id.starts_with("ollama/")
    {
        let api_config = { state.config.lock().unwrap().clone() };
        return generate_commit_message_via_ollama(&api_config, &resolved_model_id, &prompt).await;
    }

    if matches!(resolved_provider, Some(providers::ProviderId::OpenAiCompat))
        || resolved_model_id.starts_with("openai-compat/")
    {
        let api_config = { state.config.lock().unwrap().clone() };
        return generate_commit_message_via_openai_compat(&api_config, &resolved_model_id, &prompt)
            .await;
    }

    let storage_mode = Some(
        tokio::task::spawn_blocking({
            let root = root.clone();
            move || {
                let settings = crate::project_settings::load_project_settings_or_default(
                    std::path::Path::new(&root),
                );
                match settings.storage.mode {
                    crate::project_settings::StorageMode::Local => "local".to_string(),
                    crate::project_settings::StorageMode::Server => "server".to_string(),
                }
            }
        })
        .await
        .map_err(|e| format!("storage mode task failed: {}", e))?,
    );

    // Use shared WebSocket connection manager from AppState
    let ws_manager = state.ws_connection.clone();

    // Connect (or reuse existing connection)
    let mut ws_rx = ws_manager
        .ensure_connected()
        .await
        .map_err(|e| format!("WebSocket connection failed: {}", e))?;

    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while let Some(event) = ws_rx.recv().await {
            match event {
                crate::blade_ws_client::BladeWsEvent::Connected { .. } => return Ok(()),
                crate::blade_ws_client::BladeWsEvent::Error { message, .. } => {
                    return Err(format!("Authentication failed: {}", message));
                }
                crate::blade_ws_client::BladeWsEvent::Disconnected => {
                    return Err("WebSocket disconnected before authentication".to_string());
                }
                _ => {}
            }
        }

        Err("WebSocket closed before authentication".to_string())
    })
    .await
    .map_err(|_| "WebSocket authentication timed out".to_string())??;

    // Send the commit message generation request
    ws_manager
        .send_message_with_storage_mode(
            None,
            resolved_model_id,
            prompt,
            None,
            Some(workspace_info),
            storage_mode,
            Some("ask".to_string()),
            Some(LocalConversationState {
                message_count: 0,
                fingerprint: "git-commit-message".to_string(),
            }),
            Some(Vec::new()),
            Some(serde_json::json!("none")),
            Some(false),
            Some("GitCommitMessage".to_string()),
            None,
        )
        .await
        .map_err(|e| format!("Failed to send message: {}", e))?;

    // Collect response (some models stream answer via reasoning_chunk).
    let mut content = String::new();
    let mut reasoning = String::new();
    let mut finish_reason: Option<String> = None;
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        while let Some(event) = ws_rx.recv().await {
            match event {
                crate::blade_ws_client::BladeWsEvent::TextChunk {
                    content: chunk,
                    phase,
                    ..
                } => {
                    if phase.as_deref() == Some("commentary") && content.trim().is_empty() {
                        reasoning.push_str(&chunk);
                    } else {
                        content.push_str(&chunk);
                    }
                }
                crate::blade_ws_client::BladeWsEvent::ReasoningChunk { content: chunk, .. } => {
                    reasoning.push_str(&chunk);
                }
                crate::blade_ws_client::BladeWsEvent::GetConversationContext {
                    request_id,
                    session_id,
                } => {
                    ws_manager
                        .send_conversation_context(request_id, session_id, Vec::new())
                        .await
                        .map_err(|e| {
                            format!("Failed to send empty commit-message context: {}", e)
                        })?;
                }
                crate::blade_ws_client::BladeWsEvent::ToolCall { name, .. } => {
                    return Err(format!(
                        "Commit message generation unexpectedly requested tool `{}`",
                        name
                    ));
                }
                crate::blade_ws_client::BladeWsEvent::ApprovalRequest { tool_name, .. } => {
                    return Err(format!(
                        "Commit message generation unexpectedly requested approval for `{}`",
                        tool_name
                    ));
                }
                crate::blade_ws_client::BladeWsEvent::ChatDone {
                    finish_reason: reason,
                    ..
                } => {
                    finish_reason = Some(reason);
                    return Ok(());
                }
                crate::blade_ws_client::BladeWsEvent::Error { message, .. } => {
                    return Err(format!("AI generation failed: {}", message));
                }
                crate::blade_ws_client::BladeWsEvent::Disconnected => {
                    return Err(
                        "WebSocket disconnected during AI commit message generation".to_string()
                    );
                }
                _ => {}
            }
        }

        Err("WebSocket closed during AI commit message generation".to_string())
    })
    .await
    .map_err(|_| "Timed out waiting for AI commit message".to_string())??;

    // Prefer final assistant text, but fall back to reasoning stream when providers
    // emit the completion there. Then extract the last non-empty line because some
    // models include analysis before the final commit message.
    let raw = if content.trim().is_empty() {
        reasoning.as_str()
    } else {
        content.as_str()
    };

    let message = extract_commit_message(raw);

    if message.is_empty() {
        eprintln!(
            "[GIT] AI commit message generation returned empty output (finish_reason={:?}, content_len={}, reasoning_len={})",
            finish_reason,
            content.len(),
            reasoning.len()
        );
        Err(format!(
            "AI returned empty commit message{}",
            finish_reason
                .as_deref()
                .map(|reason| format!(" (finish reason: {})", reason))
                .unwrap_or_default()
        ))
    } else {
        Ok(message)
    }
}

// ── Git Graph / Log ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitStats {
    pub insertions: u32,
    pub deletions: u32,
    pub files_changed: u32,
}

#[tauri::command]
pub async fn git_commit_stats(
    _state: State<'_, AppState>,
    hash: String,
    app_handle: AppHandle,
) -> Result<GitCommitStats, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let repo = open_repo(&state)?;
        let commit = repo
            .rev_parse_single(hash.as_str())
            .map_err(|e| format!("Failed to resolve revision {hash}: {}", e))?
            .object()
            .map_err(|e| format!("Failed to read revision {hash}: {}", e))?
            .try_into_commit()
            .map_err(|e| format!("Revision {hash} is not a commit: {}", e))?;

        let commit_tree = commit
            .tree()
            .map_err(|e| format!("Failed to read commit tree for {hash}: {}", e))?;

        let base_tree = if let Some(parent_id) = commit.parent_ids().next() {
            parent_id
                .object()
                .map_err(|e| format!("Failed to read parent commit for {hash}: {}", e))?
                .try_into_commit()
                .map_err(|e| format!("Parent revision for {hash} is not a commit: {}", e))?
                .tree()
                .map_err(|e| format!("Failed to read parent tree for {hash}: {}", e))?
        } else {
            repo.empty_tree()
        };

        let mut changes = base_tree
            .changes()
            .map_err(|e| format!("Failed to initialize tree diff for {hash}: {}", e))?;
        changes.options(|options| {
            options.track_rewrites(None);
        });

        let stats = changes
            .stats(&commit_tree)
            .map_err(|e| format!("Failed to calculate commit stats for {hash}: {}", e))?;

        Ok(GitCommitStats {
            insertions: stats.lines_added.min(u64::from(u32::MAX)) as u32,
            deletions: stats.lines_removed.min(u64::from(u32::MAX)) as u32,
            files_changed: stats.files_changed.min(u64::from(u32::MAX)) as u32,
        })
    })
    .await
    .map_err(|e| format!("git commit stats task failed: {}", e))?
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitLogEntry {
    pub hash: String,
    pub short_hash: String,
    pub author_name: String,
    pub author_email: String,
    pub date: String, // ISO 8601
    pub relative_date: String,
    pub subject: String,
    pub refs: Vec<String>, // e.g. ["HEAD -> main", "origin/main", "tag: v1.0"]
    pub parents: Vec<String>, // short parent hashes
    pub insertions: u32,
    pub deletions: u32,
    pub files_changed: u32,
}

#[tauri::command]
pub async fn git_log(
    _state: State<'_, AppState>,
    count: Option<u32>,
    app_handle: AppHandle,
) -> Result<Vec<GitLogEntry>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let repo = match open_repo(&state) {
            Ok(r) => r,
            Err(_) => return Ok(Vec::new()),
        };

        let n = count.unwrap_or(50).min(500) as usize;
        let ref_map = build_ref_map(&repo);

        let head_id = match repo.head_id() {
            Ok(id) => id,
            Err(_) => return Ok(Vec::new()),
        };

        let walk = head_id
            .ancestors()
            .all()
            .map_err(|e| format!("Failed to walk commits: {}", e))?;

        let now = chrono::Utc::now();
        let mut entries = Vec::with_capacity(n);

        for info in walk {
            if entries.len() >= n {
                break;
            }
            let info = info.map_err(|e| format!("Commit walk error: {}", e))?;
            let commit = info
                .id()
                .object()
                .map_err(|e| format!("Failed to read commit: {}", e))?;
            let commit = commit.into_commit();
            let commit_ref = commit
                .decode()
                .map_err(|e| format!("Failed to decode commit: {}", e))?;

            let hash = info.id().to_string();
            let short_hash = hash[..7.min(hash.len())].to_string();

            let author = commit_ref
                .author()
                .map_err(|e| format!("Failed to decode commit author: {}", e))?;
            let author_name = author.name.to_string();
            let author_email = author.email.to_string();

            let (ts, offset_secs) = parse_signature_time(author.time);
            let fixed_offset = chrono::FixedOffset::east_opt(offset_secs)
                .unwrap_or_else(|| chrono::FixedOffset::east_opt(0).unwrap());
            let dt = chrono::DateTime::from_timestamp(ts, 0)
                .unwrap_or_default()
                .with_timezone(&fixed_offset);
            let date = dt.to_rfc3339();
            let relative_date = format_relative_date(now, dt.with_timezone(&chrono::Utc));

            let subject = commit_ref.message().summary().to_string();

            let parents: Vec<String> = commit_ref
                .parents()
                .map(|id| {
                    let s = id.to_string();
                    s[..7.min(s.len())].to_string()
                })
                .collect();

            let refs = ref_map
                .get(&info.id().detach())
                .cloned()
                .unwrap_or_default();

            entries.push(GitLogEntry {
                hash,
                short_hash,
                author_name,
                author_email,
                date,
                relative_date,
                subject,
                refs,
                parents,
                insertions: 0,
                deletions: 0,
                files_changed: 0,
            });
        }

        Ok(entries)
    })
    .await
    .map_err(|e| format!("git log task failed: {}", e))?
}

fn build_ref_map(repo: &gix::Repository) -> std::collections::HashMap<gix::ObjectId, Vec<String>> {
    let mut map: std::collections::HashMap<gix::ObjectId, Vec<String>> =
        std::collections::HashMap::new();

    let head_ref_name = repo
        .head()
        .ok()
        .and_then(|h| h.referent_name().map(|n| n.as_bstr().to_string()));

    if let Ok(refs) = repo.references() {
        if let Ok(all) = refs.all() {
            for reference in all.flatten() {
                let full_name = reference.name().as_bstr().to_string();
                let id = match reference.try_id() {
                    Some(id) => id.detach(),
                    None => continue,
                };

                let decoration = if full_name.starts_with("refs/heads/") {
                    let branch = full_name.strip_prefix("refs/heads/").unwrap();
                    if head_ref_name.as_deref() == Some(&full_name) {
                        format!("HEAD -> {}", branch)
                    } else {
                        branch.to_string()
                    }
                } else if full_name.starts_with("refs/remotes/") {
                    let remote_branch = full_name.strip_prefix("refs/remotes/").unwrap();
                    if remote_branch == "HEAD" || remote_branch.ends_with("/HEAD") {
                        continue;
                    }
                    remote_branch.to_string()
                } else if full_name.starts_with("refs/tags/") {
                    let tag = full_name.strip_prefix("refs/tags/").unwrap();
                    format!("tag: {}", tag)
                } else {
                    continue;
                };

                map.entry(id).or_default().push(decoration);
            }
        }
    }

    map
}

fn parse_signature_time(raw: &str) -> (i64, i32) {
    let mut parts = raw.split_whitespace();
    let ts = parts
        .next()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let offset_secs = parts.next().and_then(parse_git_tz_offset).unwrap_or(0);

    (ts, offset_secs)
}

fn parse_git_tz_offset(value: &str) -> Option<i32> {
    if value.len() != 5 {
        return None;
    }

    let sign = match &value[0..1] {
        "+" => 1,
        "-" => -1,
        _ => return None,
    };

    let hours = value[1..3].parse::<i32>().ok()?;
    let minutes = value[3..5].parse::<i32>().ok()?;

    Some(sign * (hours * 3600 + minutes * 60))
}

fn format_relative_date(
    now: chrono::DateTime<chrono::Utc>,
    then: chrono::DateTime<chrono::Utc>,
) -> String {
    let duration = now.signed_duration_since(then);
    let secs = duration.num_seconds();
    if secs < 0 {
        return "in the future".to_string();
    }
    let secs = secs as u64;
    if secs < 60 {
        return format!("{} seconds ago", secs);
    }
    let mins = secs / 60;
    if mins < 60 {
        return if mins == 1 {
            "1 minute ago".to_string()
        } else {
            format!("{} minutes ago", mins)
        };
    }
    let hours = mins / 60;
    if hours < 24 {
        return if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{} hours ago", hours)
        };
    }
    let days = hours / 24;
    if days < 7 {
        return if days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{} days ago", days)
        };
    }
    let weeks = days / 7;
    if weeks < 5 {
        return if weeks == 1 {
            "1 week ago".to_string()
        } else {
            format!("{} weeks ago", weeks)
        };
    }
    let months = days / 30;
    if months < 12 {
        return if months == 1 {
            "1 month ago".to_string()
        } else {
            format!("{} months ago", months)
        };
    }
    let years = days / 365;
    if years == 1 {
        "1 year ago".to_string()
    } else {
        format!("{} years ago", years)
    }
}

#[tauri::command]
pub async fn git_remote_url(
    _state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let repo = match open_repo(&state) {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        let remote = match repo.find_remote("origin") {
            Ok(r) => r,
            Err(_) => return Ok(None),
        };

        let url = match remote.url(gix::remote::Direction::Fetch) {
            Some(u) => u.to_bstring().to_string(),
            None => return Ok(None),
        };

        if url.is_empty() {
            return Ok(None);
        }

        Ok(Some(normalize_remote_url(&url)))
    })
    .await
    .map_err(|e| format!("git remote url task failed: {}", e))?
}
