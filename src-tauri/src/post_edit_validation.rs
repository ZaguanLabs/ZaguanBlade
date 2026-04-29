use crate::ai_workflow::PendingToolBatch;
use crate::app_state::AppState;
use crate::config::ApiConfig;
use crate::protocol::ToolCall;
use crate::tools;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime};

const MAX_VALIDATION_FILE_BYTES: u64 = 512 * 1024;
const MAX_VALIDATION_ERRORS_SHOWN: usize = 5;
const VALIDATION_TIMEOUT_SECS: u64 = 12;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ValidationDiagnostic {
    severity: String,
    message: String,
    code: Option<String>,
    source: Option<String>,
    line: Option<u64>,
    column: Option<u64>,
}

#[derive(Debug, Clone)]
struct ValidationSnapshot {
    tier: Option<String>,
    diagnostics: Vec<ValidationDiagnostic>,
}

pub async fn augment_batch_with_validation_feedback<R: Runtime>(
    app_handle: &AppHandle<R>,
    batch: &mut PendingToolBatch,
) {
    let state = app_handle.state::<AppState>();
    let workspace_root = {
        let workspace = state.workspace.lock().unwrap();
        workspace.workspace.clone()
    };
    let Some(workspace_root) = workspace_root else {
        return;
    };

    let config = state.config.lock().unwrap().clone();
    if config.api_key.trim().is_empty() || config.blade_url.trim().is_empty() {
        return;
    }

    let mut feedback_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut capability_cache: HashMap<String, bool> = HashMap::new();
    let mut validation_cache: HashMap<String, Option<ValidationSnapshot>> = HashMap::new();
    let ws_manager = state.ws_connection.clone();

    for (call, result) in &mut batch.file_results {
        if !result.success || !is_validation_candidate_tool(call.function.name.as_str()) {
            continue;
        }

        let Some(raw_path) = extract_tool_path(call) else {
            continue;
        };

        let resolved_path = resolve_tool_path(&workspace_root, raw_path.as_str());
        let cache_key = resolved_path.to_string_lossy().to_string();

        let feedback = if let Some(cached) = feedback_cache.get(cache_key.as_str()) {
            cached.clone()
        } else {
            let computed = match build_validation_feedback(
                ws_manager.clone(),
                &config,
                &workspace_root,
                &resolved_path,
                call,
                &mut capability_cache,
                &mut validation_cache,
            )
            .await
            {
                Ok(value) => value,
                Err(error) => {
                    eprintln!(
                        "[POST-EDIT VALIDATION] Failed to validate {}: {}",
                        resolved_path.display(),
                        error
                    );
                    None
                }
            };
            feedback_cache.insert(cache_key, computed.clone());
            computed
        };

        if let Some(feedback) = feedback {
            if result.content.trim().is_empty() {
                result.content = feedback;
            } else if !result.content.contains(feedback.as_str()) {
                result.content.push_str("\n\n");
                result.content.push_str(feedback.as_str());
            }
        }
    }
}

fn is_validation_candidate_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "write_file"
            | "write_file_validated"
            | "create_file"
            | "replace_file_content"
            | "apply_edit"
            | "apply_patch"
            | "apply_patch_validated"
            | "edit_file"
            | "multi_replace_file_content"
    )
}

fn extract_tool_path(call: &ToolCall) -> Option<String> {
    let args: Value = serde_json::from_str(&call.function.arguments).ok()?;
    [
        "path",
        "file_path",
        "filepath",
        "filename",
        "TargetFile",
        "target_file",
    ]
    .iter()
    .find_map(|key| {
        args.get(key)
            .and_then(Value::as_str)
            .map(ToString::to_string)
    })
}

fn resolve_tool_path(workspace_root: &Path, raw_path: &str) -> PathBuf {
    let path = Path::new(raw_path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    }
}

async fn build_validation_feedback(
    ws_manager: std::sync::Arc<crate::ws_connection_manager::WsConnectionManager>,
    config: &ApiConfig,
    workspace_root: &Path,
    resolved_path: &Path,
    call: &ToolCall,
    capability_cache: &mut HashMap<String, bool>,
    validation_cache: &mut HashMap<String, Option<ValidationSnapshot>>,
) -> Result<Option<String>, String> {
    let Some(language) =
        resolve_supported_language(ws_manager.clone(), config, resolved_path, capability_cache)
            .await?
    else {
        return Ok(None);
    };

    let metadata = match tokio::fs::metadata(resolved_path).await {
        Ok(metadata) => metadata,
        Err(_) => return Ok(None),
    };

    if !metadata.is_file() || metadata.len() > MAX_VALIDATION_FILE_BYTES {
        return Ok(None);
    }

    let content = match tokio::fs::read_to_string(resolved_path).await {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };

    if content.trim().is_empty() {
        return Ok(None);
    }

    let cache_key = format!("{}::{}", resolved_path.to_string_lossy(), language);
    let post_snapshot = if let Some(cached) = validation_cache.get(cache_key.as_str()) {
        cached.clone()
    } else {
        let computed = validate_snapshot(
            ws_manager.clone(),
            config,
            resolved_path,
            language.as_str(),
            content.clone(),
        )
        .await?;
        validation_cache.insert(cache_key, computed.clone());
        computed
    };

    let Some(post_snapshot) = post_snapshot else {
        return Ok(None);
    };

    let introduced_diagnostics = match derive_pre_edit_content(call, content.as_str())? {
        Some(pre_edit_content) => {
            let Some(pre_snapshot) = validate_snapshot(
                ws_manager.clone(),
                config,
                resolved_path,
                language.as_str(),
                pre_edit_content,
            )
            .await?
            else {
                return Ok(None);
            };
            diff_newly_introduced_diagnostics(&pre_snapshot.diagnostics, &post_snapshot.diagnostics)
        }
        None => post_snapshot
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity.eq_ignore_ascii_case("error"))
            .cloned()
            .collect(),
    };

    Ok(format_validation_feedback(
        workspace_root,
        resolved_path,
        post_snapshot.tier.as_deref(),
        &introduced_diagnostics,
    ))
}

async fn validate_snapshot(
    ws_manager: std::sync::Arc<crate::ws_connection_manager::WsConnectionManager>,
    config: &ApiConfig,
    resolved_path: &Path,
    language: &str,
    content: String,
) -> Result<Option<ValidationSnapshot>, String> {
    if content.trim().is_empty() {
        return Ok(Some(ValidationSnapshot {
            tier: None,
            diagnostics: Vec::new(),
        }));
    }

    if content.len() as u64 > MAX_VALIDATION_FILE_BYTES {
        return Ok(None);
    }

    let response =
        request_zlp_validation(ws_manager, config, resolved_path, content, language).await?;
    let (tier, diagnostics) = parse_validation_response(&response);
    Ok(Some(ValidationSnapshot { tier, diagnostics }))
}

fn derive_pre_edit_content(
    call: &ToolCall,
    post_edit_content: &str,
) -> Result<Option<String>, String> {
    let args: Value = serde_json::from_str(&call.function.arguments)
        .map_err(|error| format!("invalid tool args json: {}", error))?;
    let Some(object) = args.as_object() else {
        return Ok(None);
    };

    match call.function.name.as_str() {
        "write_file" | "write_file_validated" | "create_file" => Ok(None),
        "edit_file"
        | "apply_edit"
        | "apply_patch"
        | "apply_patch_validated"
        | "replace_file_content"
        | "multi_replace_file_content" => {
            if let Some(patches) = object.get("patches").and_then(Value::as_array) {
                let mut pre_edit_content = post_edit_content.to_string();
                for patch in patches.iter().rev() {
                    let Some(old_text) = patch.get("old_text").and_then(Value::as_str) else {
                        continue;
                    };
                    let Some(new_text) = patch.get("new_text").and_then(Value::as_str) else {
                        continue;
                    };
                    pre_edit_content = tools::apply_patch_to_string(
                        pre_edit_content.as_str(),
                        new_text,
                        old_text,
                    )?;
                }
                Ok(Some(pre_edit_content))
            } else {
                let old_content = object
                    .get("old_content")
                    .or_else(|| object.get("old"))
                    .or_else(|| object.get("from"))
                    .or_else(|| object.get("old_text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let new_content = object
                    .get("new_content")
                    .or_else(|| object.get("new"))
                    .or_else(|| object.get("to"))
                    .or_else(|| object.get("new_text"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();

                Ok(Some(tools::apply_patch_to_string(
                    post_edit_content,
                    new_content,
                    old_content,
                )?))
            }
        }
        _ => Ok(None),
    }
}

fn diff_newly_introduced_diagnostics(
    before: &[ValidationDiagnostic],
    after: &[ValidationDiagnostic],
) -> Vec<ValidationDiagnostic> {
    let existing_errors: HashSet<ValidationDiagnostic> = before
        .iter()
        .filter(|diagnostic| diagnostic.severity.eq_ignore_ascii_case("error"))
        .cloned()
        .collect();

    after
        .iter()
        .filter(|diagnostic| diagnostic.severity.eq_ignore_ascii_case("error"))
        .filter(|diagnostic| !existing_errors.contains(*diagnostic))
        .cloned()
        .collect()
}

fn candidate_languages_for_path(path: &Path) -> Vec<&'static str> {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return Vec::new();
    };

    let ext = extension.to_ascii_lowercase();
    let mut candidates = Vec::with_capacity(4);
    let mut push = |language: &'static str| {
        if !candidates.contains(&language) {
            candidates.push(language);
        }
    };

    match ext.as_str() {
        "ts" => {
            push("typescript");
            push("ts");
        }
        "tsx" => {
            push("tsx");
            push("typescript");
            push("ts");
        }
        "js" | "mjs" | "cjs" => {
            push("javascript");
            push("js");
        }
        "jsx" => {
            push("jsx");
            push("javascript");
            push("js");
        }
        "rs" => push("rust"),
        "py" => push("python"),
        "go" => push("go"),
        "java" => push("java"),
        "kt" | "kts" => push("kotlin"),
        "cs" => {
            push("csharp");
            push("c#");
        }
        "c" => push("c"),
        "cc" | "cpp" | "cxx" => {
            push("cpp");
            push("c++");
        }
        "h" => {
            push("c");
            push("cpp");
        }
        "hh" | "hpp" | "hxx" => push("cpp"),
        "php" => push("php"),
        "rb" => push("ruby"),
        "swift" => push("swift"),
        "scala" => push("scala"),
        "lua" => push("lua"),
        "sql" => push("sql"),
        "json" => push("json"),
        "toml" => push("toml"),
        "xml" => push("xml"),
        "html" | "htm" => push("html"),
        "css" => push("css"),
        "scss" => push("scss"),
        "sass" => push("sass"),
        "less" => push("less"),
        "yml" | "yaml" => push("yaml"),
        "sh" => {
            push("shell");
            push("bash");
            push("sh");
        }
        "bash" => {
            push("bash");
            push("shell");
        }
        "zsh" => {
            push("zsh");
            push("shell");
        }
        "fish" => {
            push("fish");
            push("shell");
        }
        "vue" => push("vue"),
        "svelte" => push("svelte"),
        "astro" => push("astro"),
        _ => {}
    }

    candidates
}

async fn resolve_supported_language(
    ws_manager: std::sync::Arc<crate::ws_connection_manager::WsConnectionManager>,
    config: &ApiConfig,
    path: &Path,
    capability_cache: &mut HashMap<String, bool>,
) -> Result<Option<String>, String> {
    for candidate in candidate_languages_for_path(path) {
        let supported = if let Some(cached) = capability_cache.get(candidate) {
            *cached
        } else {
            let supports_validation =
                request_zlp_capabilities(ws_manager.clone(), config, candidate)
                    .await
                    .map(|response| parse_capabilities_supports_validation(&response))
                    .unwrap_or(false);
            capability_cache.insert(candidate.to_string(), supports_validation);
            supports_validation
        };

        if supported {
            return Ok(Some(candidate.to_string()));
        }
    }

    Ok(None)
}

async fn request_zlp_capabilities(
    ws_manager: std::sync::Arc<crate::ws_connection_manager::WsConnectionManager>,
    config: &ApiConfig,
    language: &str,
) -> Result<Value, String> {
    let payload = serde_json::json!({
        "method": "zlp.capabilities",
        "params": {
            "language": language
        }
    });

    tokio::time::timeout(Duration::from_secs(VALIDATION_TIMEOUT_SECS), async move {
        let _ = config;
        ws_manager.request_zlp(payload).await
    })
    .await
    .map_err(|_| "zlp capabilities timed out".to_string())?
}

fn parse_capabilities_supports_validation(response: &Value) -> bool {
    let payload = response.get("result").unwrap_or(response);

    let support_validate = payload
        .get("support")
        .and_then(|support| support.get("validate"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let support_validate_fast = payload
        .get("support")
        .and_then(|support| support.get("validate_fast"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let supports_validate = payload
        .get("supports")
        .and_then(|support| support.get("validate"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let supports_validate_fast = payload
        .get("supports")
        .and_then(|support| support.get("validate_fast"))
        .and_then(Value::as_bool)
        .unwrap_or(false);

    support_validate || support_validate_fast || supports_validate || supports_validate_fast
}

async fn request_zlp_validation(
    ws_manager: std::sync::Arc<crate::ws_connection_manager::WsConnectionManager>,
    config: &ApiConfig,
    path: &Path,
    content: String,
    language: &str,
) -> Result<Value, String> {
    let payload = serde_json::json!({
        "method": "zlp.validate",
        "params": {
            "path": path.to_string_lossy().to_string(),
            "content": content,
            "language": language,
            "mode": "fast"
        }
    });

    tokio::time::timeout(Duration::from_secs(VALIDATION_TIMEOUT_SECS), async move {
        let _ = config;
        ws_manager.request_zlp(payload).await
    })
    .await
    .map_err(|_| "zlp validation timed out".to_string())?
}

fn parse_validation_response(response: &Value) -> (Option<String>, Vec<ValidationDiagnostic>) {
    let payload = response.get("result").unwrap_or(response);
    let tier = payload
        .get("tier")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let errors = if let Some(array) = payload.as_array() {
        array
            .iter()
            .filter_map(parse_validation_diagnostic)
            .collect()
    } else if let Some(array) = payload.get("errors").and_then(Value::as_array) {
        array
            .iter()
            .filter_map(parse_validation_diagnostic)
            .collect()
    } else if let Some(array) = payload
        .get("result")
        .and_then(|value| value.get("errors"))
        .and_then(Value::as_array)
    {
        array
            .iter()
            .filter_map(parse_validation_diagnostic)
            .collect()
    } else {
        Vec::new()
    };

    (tier, errors)
}

fn parse_validation_diagnostic(value: &Value) -> Option<ValidationDiagnostic> {
    let message = value.get("message")?.as_str()?.trim().to_string();
    if message.is_empty() {
        return None;
    }

    let line = value
        .get("range")
        .and_then(|range| range.get("start"))
        .and_then(|start| start.get("line"))
        .and_then(Value::as_u64)
        .map(|line| line + 1);
    let column = value
        .get("range")
        .and_then(|range| range.get("start"))
        .and_then(|start| start.get("column"))
        .and_then(Value::as_u64)
        .map(|column| column + 1);

    Some(ValidationDiagnostic {
        severity: value
            .get("severity")
            .and_then(Value::as_str)
            .unwrap_or("error")
            .to_string(),
        message,
        code: value.get("code").map(|code| match code {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        }),
        source: value
            .get("source")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        line,
        column,
    })
}

fn format_validation_feedback(
    workspace_root: &Path,
    resolved_path: &Path,
    tier: Option<&str>,
    diagnostics: &[ValidationDiagnostic],
) -> Option<String> {
    let errors: Vec<&ValidationDiagnostic> = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity.eq_ignore_ascii_case("error"))
        .collect();
    if errors.is_empty() {
        return None;
    }

    let display_path = resolved_path
        .strip_prefix(workspace_root)
        .ok()
        .and_then(Path::to_str)
        .unwrap_or_else(|| resolved_path.to_str().unwrap_or("unknown file"));

    let mut lines = Vec::with_capacity(errors.len().min(MAX_VALIDATION_ERRORS_SHOWN) + 3);
    let mut header = format!(
        "POST-EDIT VALIDATION: {} newly introduced error{} found in {}",
        errors.len(),
        if errors.len() == 1 { "" } else { "s" },
        display_path
    );
    if let Some(tier) = tier.filter(|value| !value.is_empty()) {
        header.push_str(" (");
        header.push_str(tier);
        header.push(')');
    }
    lines.push(header);

    for diagnostic in errors.iter().take(MAX_VALIDATION_ERRORS_SHOWN) {
        let mut line = String::from("- ");
        if let Some(location) = format_location(diagnostic) {
            line.push_str(location.as_str());
            line.push_str(": ");
        }
        line.push_str(diagnostic.message.as_str());

        let code_or_source = match (
            diagnostic
                .source
                .as_deref()
                .filter(|value| !value.is_empty()),
            diagnostic.code.as_deref().filter(|value| !value.is_empty()),
        ) {
            (Some(source), Some(code)) => Some(format!("{}:{}", source, code)),
            (Some(source), None) => Some(source.to_string()),
            (None, Some(code)) => Some(code.to_string()),
            (None, None) => None,
        };
        if let Some(code_or_source) = code_or_source {
            line.push_str(" [");
            line.push_str(code_or_source.as_str());
            line.push(']');
        }
        lines.push(line);
    }

    if errors.len() > MAX_VALIDATION_ERRORS_SHOWN {
        let remaining = errors.len() - MAX_VALIDATION_ERRORS_SHOWN;
        lines.push(format!(
            "- ... {} more error{}",
            remaining,
            if remaining == 1 { "" } else { "s" }
        ));
    }

    lines.push("Fix these validation errors before continuing with unrelated changes.".to_string());
    Some(lines.join("\n"))
}

fn format_location(diagnostic: &ValidationDiagnostic) -> Option<String> {
    match (diagnostic.line, diagnostic.column) {
        (Some(line), Some(column)) => Some(format!("line {}, column {}", line, column)),
        (Some(line), None) => Some(format!("line {}", line)),
        (None, Some(column)) => Some(format!("column {}", column)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        candidate_languages_for_path, derive_pre_edit_content, diff_newly_introduced_diagnostics,
        format_validation_feedback, parse_capabilities_supports_validation,
        parse_validation_response, ValidationDiagnostic,
    };
    use crate::protocol::{ToolCall, ToolFunction};
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn language_detection_returns_broad_candidate_sets() {
        assert_eq!(
            candidate_languages_for_path(Path::new("src/app/page.tsx")),
            vec!["tsx", "typescript", "ts"]
        );
        assert_eq!(
            candidate_languages_for_path(Path::new("src/index.js")),
            vec!["javascript", "js"]
        );
        assert_eq!(
            candidate_languages_for_path(Path::new("src/main.cpp")),
            vec!["cpp", "c++"]
        );
        assert_eq!(
            candidate_languages_for_path(Path::new("README.md")),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn parse_capabilities_supports_both_documented_shapes() {
        let legacy_shape = json!({
            "result": {
                "support": {
                    "validate": true
                }
            }
        });
        let current_shape = json!({
            "supports": {
                "validate_fast": true
            }
        });

        assert!(parse_capabilities_supports_validation(&legacy_shape));
        assert!(parse_capabilities_supports_validation(&current_shape));
    }

    #[test]
    fn diff_newly_introduced_diagnostics_filters_existing_errors() {
        let existing = ValidationDiagnostic {
            severity: "error".to_string(),
            message: "Cannot find name 'foo'".to_string(),
            code: Some("TS2304".to_string()),
            source: Some("tsc".to_string()),
            line: Some(12),
            column: Some(3),
        };
        let new_error = ValidationDiagnostic {
            severity: "error".to_string(),
            message: "Missing semicolon".to_string(),
            code: Some("TS1005".to_string()),
            source: Some("tsc".to_string()),
            line: Some(18),
            column: Some(9),
        };

        let after = vec![existing.clone(), new_error.clone()];
        let introduced =
            diff_newly_introduced_diagnostics(std::slice::from_ref(&existing), after.as_slice());

        assert_eq!(introduced, vec![new_error]);
    }

    #[test]
    fn derive_pre_edit_content_reverses_multi_patch_calls() {
        let call = ToolCall {
            id: "call-1".to_string(),
            typ: "function".to_string(),
            function: ToolFunction {
                name: "apply_patch".to_string(),
                arguments: json!({
                    "path": "src/app.ts",
                    "patches": [
                        {
                            "old_text": "alpha",
                            "new_text": "beta"
                        },
                        {
                            "old_text": "beta",
                            "new_text": "gamma"
                        }
                    ]
                })
                .to_string(),
            },
            status: None,
            result: None,
        };

        let pre_edit = derive_pre_edit_content(&call, "gamma")
            .expect("should reverse patches")
            .expect("should produce prior content");

        assert_eq!(pre_edit, "alpha");
    }

    #[test]
    fn derive_pre_edit_content_reverses_apply_patch_validated_calls() {
        let call = ToolCall {
            id: "call-validated".to_string(),
            typ: "function".to_string(),
            function: ToolFunction {
                name: "apply_patch_validated".to_string(),
                arguments: json!({
                    "path": "src/app.ts",
                    "old_text": "before",
                    "new_text": "after"
                })
                .to_string(),
            },
            status: None,
            result: None,
        };

        let pre_edit = derive_pre_edit_content(&call, "after")
            .expect("should reverse validated patch")
            .expect("should produce prior content");

        assert_eq!(pre_edit, "before");
    }

    #[test]
    fn parse_validation_response_reads_nested_error_payloads() {
        let response = json!({
            "result": {
                "tier": "syntax",
                "errors": [
                    {
                        "message": "Unexpected token",
                        "severity": "error",
                        "code": "TS1005",
                        "source": "tsc",
                        "range": {
                            "start": { "line": 3, "column": 7 },
                            "end": { "line": 3, "column": 8 }
                        }
                    }
                ]
            }
        });

        let (tier, diagnostics) = parse_validation_response(&response);
        assert_eq!(tier.as_deref(), Some("syntax"));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].line, Some(4));
        assert_eq!(diagnostics[0].column, Some(8));
        assert_eq!(diagnostics[0].code.as_deref(), Some("TS1005"));
    }

    #[test]
    fn format_validation_feedback_omits_non_errors() {
        let diagnostics = vec![ValidationDiagnostic {
            severity: "warning".to_string(),
            message: "Unused import".to_string(),
            code: None,
            source: Some("tsc".to_string()),
            line: Some(2),
            column: Some(1),
        }];

        let feedback = format_validation_feedback(
            Path::new("/workspace"),
            Path::new("/workspace/src/app/page.tsx"),
            Some("lint"),
            &diagnostics,
        );
        assert!(feedback.is_none());
    }

    #[test]
    fn format_validation_feedback_is_compact_and_actionable() {
        let diagnostics = vec![ValidationDiagnostic {
            severity: "error".to_string(),
            message: "Cannot find name 'foo'".to_string(),
            code: Some("TS2304".to_string()),
            source: Some("tsc".to_string()),
            line: Some(12),
            column: Some(3),
        }];

        let feedback = format_validation_feedback(
            Path::new("/workspace"),
            Path::new("/workspace/src/app/page.tsx"),
            Some("compile"),
            &diagnostics,
        )
        .expect("expected feedback");

        assert!(feedback.contains(
            "POST-EDIT VALIDATION: 1 newly introduced error found in src/app/page.tsx (compile)"
        ));
        assert!(feedback.contains("line 12, column 3: Cannot find name 'foo' [tsc:TS2304]"));
        assert!(feedback
            .contains("Fix these validation errors before continuing with unrelated changes."));
    }
}
