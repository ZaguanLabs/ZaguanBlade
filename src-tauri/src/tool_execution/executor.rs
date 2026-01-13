use crate::tools::{self, ToolResult};
use std::path::Path;
use tauri::AppHandle;

use tauri::Runtime;

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

/// Execute a tool with IDE context
pub fn execute_tool_with_context<R: Runtime>(
    context: &ToolExecutionContext<R>,
    tool_name: &str,
    args: &str,
) -> ToolResult {
    // Get workspace root or use current directory
    let workspace_path = context
        .workspace_root
        .as_ref()
        .map(|s| Path::new(s))
        .unwrap_or_else(|| Path::new("."));

    // DEBUG: Log workspace path
    eprintln!(
        "[TOOL EXEC] tool={}, workspace={:?}, args={}",
        tool_name, workspace_path, args
    );

    // Build EditorState for IDE-specific tools
    let editor_state = tools::EditorState {
        active_file: context.active_file.clone(),
        open_files: context.open_files.clone(),
        active_tab_index: context.active_tab_index,
        cursor_line: context.cursor_line,
        cursor_column: context.cursor_column,
        selection_start_line: context.selection_start_line,
        selection_end_line: context.selection_end_line,
    };

    // Execute tool with editor state
    tools::execute_tool_with_editor(
        workspace_path,
        tool_name,
        args,
        Some(&editor_state),
        context.app_handle.as_ref(),
    )
}
