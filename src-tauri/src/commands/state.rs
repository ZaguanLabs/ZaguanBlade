//! State Commands
//!
//! Tauri commands for querying core state and feature flags.

use serde::Serialize;
use tauri::State;

use crate::app_state::AppState;
use crate::blade_protocol::Version;
use crate::config::RemoteAiConfig;
use crate::core_state::{
    ChatStateSnapshot, CoreStateSnapshot, EditorStateSnapshot, PendingApproval, ProtocolInfo,
    TerminalStateSnapshot, WorkspaceStateSnapshot,
};
use crate::feature_flags::FeatureFlagsSnapshot;

#[derive(Clone, Serialize)]
pub struct BootstrapState {
    pub core_state: CoreStateSnapshot,
    pub feature_flags: FeatureFlagsSnapshot,
    pub remote_settings: RemoteAiConfig,
    pub user_id: Option<String>,
    pub has_zblade_directory: Option<bool>,
}

fn build_core_state_snapshot(state: &AppState) -> CoreStateSnapshot {
    let workspace_root = state.workspace.lock().unwrap().workspace.clone();

    let project_id = workspace_root
        .as_ref()
        .and_then(|path| crate::project::get_existing_project_id(path));

    let workspace_snapshot = WorkspaceStateSnapshot {
        path: workspace_root
            .as_ref()
            .map(|path| path.to_string_lossy().to_string()),
        project_id,
    };

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
            cursor_line: cursor_line.map(|line| line as u32),
            cursor_column: cursor_column.map(|column| column as u32),
            selection_start: selection_start.map(|line| line as u32),
            selection_end: selection_end.map(|line| line as u32),
        }
    };

    let chat_snapshot = {
        let chat_manager = state.chat_manager.lock().unwrap();
        let conversation = state.conversation.lock().unwrap();

        ChatStateSnapshot {
            session_id: chat_manager.session_id.clone(),
            message_count: conversation.len(),
            is_generating: chat_manager.streaming,
            selected_model: None,
        }
    };

    let terminals: Vec<TerminalStateSnapshot> = vec![];
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

/// Returns a complete snapshot of the core application state.
/// Used for UI initialization, reload recovery, and debugging.
#[tauri::command]
pub fn get_core_state(state: State<'_, AppState>) -> CoreStateSnapshot {
    build_core_state_snapshot(&state)
}

/// Returns the startup bootstrap payload used to hydrate the frontend shell.
#[tauri::command]
pub fn bootstrap_state(state: State<'_, AppState>) -> BootstrapState {
    let workspace_root = state.workspace.lock().unwrap().workspace.clone();
    let has_zblade_directory = workspace_root
        .as_ref()
        .map(|path| crate::project_settings::has_zblade_dir(path));

    BootstrapState {
        core_state: build_core_state_snapshot(&state),
        feature_flags: state.feature_flags.snapshot(),
        remote_settings: state.config.lock().unwrap().remote_config(),
        user_id: state.user_id.lock().unwrap().clone(),
        has_zblade_directory,
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
        "composite_tools_enabled" => {
            state.feature_flags.set_composite_tools_enabled(value);
            Ok(())
        }
        "grep_timeout_enforced" => {
            state.feature_flags.set_grep_timeout_enforced(value);
            Ok(())
        }
        _ => Err(format!("Unknown feature flag: {}", flag)),
    }
}
