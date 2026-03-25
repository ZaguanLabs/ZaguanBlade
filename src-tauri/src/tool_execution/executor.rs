use crate::tools::{self, ToolResult};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tauri::AppHandle;

use tauri::Runtime;

const DEFAULT_TOOL_EXECUTION_TIMEOUT: Duration = Duration::from_secs(45);

/// Context for IDE-aware tool execution
pub struct ToolExecutionContext<R: Runtime> {
    pub workspace_root: Option<String>,
    pub active_file: Option<String>,
    pub open_files: Vec<String>,
    pub active_tab_index: usize,
    pub cursor_line: Option<usize>,
    pub cursor_column: Option<usize>,
    pub selection_start_line: Option<usize>,
    pub selection_end_line: Option<usize>,
    pub app_handle: Option<AppHandle<R>>,
}

impl<R: Runtime> ToolExecutionContext<R> {
    pub fn new(
        workspace_root: Option<String>,
        active_file: Option<String>,
        open_files: Vec<String>,
        active_tab_index: usize,
        cursor_line: Option<usize>,
        cursor_column: Option<usize>,
        selection_start_line: Option<usize>,
        selection_end_line: Option<usize>,
        app_handle: Option<tauri::AppHandle<R>>,
    ) -> Self {
        Self {
            workspace_root,
            active_file,
            open_files,
            active_tab_index,
            cursor_line,
            cursor_column,
            selection_start_line,
            selection_end_line,
            app_handle,
        }
    }
}

fn build_editor_state<R: Runtime>(context: &ToolExecutionContext<R>) -> tools::EditorState {
    tools::EditorState {
        active_file: context.active_file.clone(),
        open_files: context.open_files.clone(),
        active_tab_index: context.active_tab_index,
        cursor_line: context.cursor_line,
        cursor_column: context.cursor_column,
        selection_start_line: context.selection_start_line,
        selection_end_line: context.selection_end_line,
    }
}

fn execute_tool_snapshot(
    workspace_root: Option<String>,
    editor_state: tools::EditorState,
    tool_name: String,
    args: String,
) -> ToolResult {
    let workspace_path = workspace_root
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    tools::execute_tool_with_editor::<tauri::Wry>(
        &workspace_path,
        &tool_name,
        &args,
        Some(&editor_state),
        None,
    )
}

fn execute_tool_inner<R: Runtime>(
    context: &ToolExecutionContext<R>,
    tool_name: &str,
    args: &str,
) -> ToolResult {
    let workspace_path = context
        .workspace_root
        .as_ref()
        .map(|s| Path::new(s))
        .unwrap_or_else(|| Path::new("."));

    let editor_state = tools::EditorState {
        active_file: context.active_file.clone(),
        open_files: context.open_files.clone(),
        active_tab_index: context.active_tab_index,
        cursor_line: context.cursor_line,
        cursor_column: context.cursor_column,
        selection_start_line: context.selection_start_line,
        selection_end_line: context.selection_end_line,
    };

    tools::execute_tool_with_editor(
        workspace_path,
        tool_name,
        args,
        Some(&editor_state),
        context.app_handle.as_ref(),
    )
}

fn execute_with_timeout<F>(tool_name: &str, timeout: Duration, f: F) -> ToolResult
where
    F: FnOnce() -> ToolResult + Send + 'static,
{
    let (tx, rx) = mpsc::sync_channel(1);
    let tool_name_owned = tool_name.to_string();
    thread::spawn(move || {
        let _ = tx.send(f());
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => ToolResult::err(format!(
            "tool '{}' timed out after {} seconds. Narrow the scope or try a more targeted query.",
            tool_name_owned,
            timeout.as_secs()
        )),
        Err(mpsc::RecvTimeoutError::Disconnected) => ToolResult::err(format!(
            "tool '{}' execution terminated unexpectedly",
            tool_name_owned
        )),
    }
}

/// Execute a tool with IDE context
pub fn execute_tool_with_context<R: Runtime>(
    context: &ToolExecutionContext<R>,
    tool_name: &str,
    args: &str,
) -> ToolResult {
    execute_tool_inner(context, tool_name, args)
}

pub fn execute_tool_with_default_timeout<R: Runtime>(
    context: &ToolExecutionContext<R>,
    tool_name: &str,
    args: &str,
) -> ToolResult {
    let workspace_root = context.workspace_root.clone();
    let editor_state = build_editor_state(context);
    let tool_name_owned = tool_name.to_string();
    let args_owned = args.to_string();
    execute_with_timeout(tool_name, DEFAULT_TOOL_EXECUTION_TIMEOUT, move || {
        execute_tool_snapshot(workspace_root, editor_state, tool_name_owned, args_owned)
    })
}

#[cfg(test)]
mod tests {
    use super::execute_with_timeout;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn execute_with_timeout_returns_result_before_deadline() {
        let result = execute_with_timeout("test_tool", Duration::from_millis(50), || {
            crate::tools::ToolResult::ok("done")
        });

        assert!(result.success);
        assert_eq!(result.content, "done");
    }

    #[test]
    fn execute_with_timeout_returns_timeout_error() {
        let result = execute_with_timeout("slow_tool", Duration::from_millis(10), || {
            thread::sleep(Duration::from_millis(40));
            crate::tools::ToolResult::ok("late")
        });

        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("timed out"));
    }
}
