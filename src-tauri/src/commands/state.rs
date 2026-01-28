//! State Commands
//!
//! Tauri commands for querying core state and feature flags.

use tauri::State;

use crate::app_state::AppState;
use crate::blade_protocol::Version;
use crate::core_state::{
    ChatStateSnapshot, CoreStateSnapshot, EditorStateSnapshot, PendingApproval, ProtocolInfo,
    TerminalStateSnapshot, WorkspaceStateSnapshot,
};
use crate::feature_flags::FeatureFlagsSnapshot;

/// Returns a complete snapshot of the core application state.
/// Used for UI initialization, reload recovery, and debugging.
#[tauri::command]
pub fn get_core_state(state: State<'_, AppState>) -> CoreStateSnapshot {
    // Workspace
    let workspace_snapshot = {
        let ws = state.workspace.lock().unwrap();
        WorkspaceStateSnapshot {
            path: ws.workspace.as_ref().map(|p| p.to_string_lossy().to_string()),
            project_id: None, // TODO: Add project_id tracking
        }
    };

    // Editor
    let editor_snapshot = {
        let active_file = state.active_file.lock().unwrap();
        let open_files = state.open_files.lock().unwrap();
        let cursor_line = state.cursor_line.lock().unwrap();
        let cursor_column = state.cursor_column.lock().unwrap();
        let selection_start = state.selection_start_line.lock().unwrap();
        let selection_end = state.selection_end_line.lock().unwrap();

        EditorStateSnapshot {
            active_file: active_file.clone(),
            open_files: open_files.clone(),
            cursor_line: cursor_line.map(|l| l as u32),
            cursor_column: cursor_column.map(|c| c as u32),
            selection_start: selection_start.map(|l| l as u32),
            selection_end: selection_end.map(|l| l as u32),
        }
    };

    // Chat
    let chat_snapshot = {
        let chat_manager = state.chat_manager.lock().unwrap();
        let conversation = state.conversation.lock().unwrap();

        ChatStateSnapshot {
            session_id: chat_manager.session_id.clone(),
            message_count: conversation.len(),
            is_generating: chat_manager.streaming,
            // Model selection is managed by frontend/project state
            selected_model: None,
        }
    };

    // Terminals - basic snapshot for now
    // TODO: Get actual terminal state from TerminalManager
    let terminals: Vec<TerminalStateSnapshot> = vec![];

    // Pending approvals
    // TODO: Get from workflow state
    let pending_approvals: Vec<PendingApproval> = vec![];

    CoreStateSnapshot {
        protocol: ProtocolInfo {
            version: Version::CURRENT,
            capabilities: CoreStateSnapshot::default_capabilities(),
        },
        workspace: workspace_snapshot,
        editor: editor_snapshot,
        chat: chat_snapshot,
        terminals,
        pending_approvals,
    }
}

/// Returns the current feature flags configuration.
#[tauri::command]
pub fn get_feature_flags(state: State<'_, AppState>) -> FeatureFlagsSnapshot {
    state.feature_flags.snapshot()
}

/// Sets a feature flag value. Used for testing and gradual rollout.
#[tauri::command]
pub fn set_feature_flag(
    flag: String,
    value: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    match flag.as_str() {
        "editor_backend_authority" => {
            state.feature_flags.set_editor_backend_authority(value);
            Ok(())
        }
        "tabs_backend_authority" => {
            state.feature_flags.set_tabs_backend_authority(value);
            Ok(())
        }
        _ => Err(format!("Unknown feature flag: {}", flag)),
    }
}
