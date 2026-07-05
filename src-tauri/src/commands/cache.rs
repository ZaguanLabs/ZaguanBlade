use crate::app_state::AppState;
use crate::{warmup, warmup_bundle};
use tauri::{command, State};

#[command]
pub async fn warmup_cache(
    session_id: String,
    model: String,
    trigger: String,
    editor_snapshot: Option<warmup_bundle::EditorWarmupSnapshot>,
    state: State<'_, AppState>,
) -> Result<warmup::WarmupResponse, String> {
    let trigger = match trigger.as_str() {
        "launch" => warmup::WarmupTrigger::Launch,
        "model_change" => warmup::WarmupTrigger::ModelChange,
        "workspace_change" => warmup::WarmupTrigger::WorkspaceChange,
        "session_resume" => warmup::WarmupTrigger::SessionResume,
        _ => warmup::WarmupTrigger::Launch,
    };

    // Bundle assembly reads files and shells out to git — keep it off the
    // async runtime. A failed build degrades to a legacy context-free warmup.
    let context = match (editor_snapshot, state.workspace_root()) {
        (Some(snapshot), Some(root)) => {
            tokio::task::spawn_blocking(move || warmup_bundle::build_bundle(&root, snapshot))
                .await
                .ok()
        }
        _ => None,
    };

    state
        .warmup_client
        .warmup(&session_id, &model, trigger, context)
        .await
}

#[command]
pub fn should_rewarm_cache(state: State<'_, AppState>) -> bool {
    state.warmup_client.should_rewarm()
}
