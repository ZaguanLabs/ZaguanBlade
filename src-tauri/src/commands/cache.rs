use crate::app_state::AppState;
use crate::warmup;
use tauri::{command, State};

#[command]
pub async fn warmup_cache(
    session_id: String,
    model: String,
    trigger: String,
    state: State<'_, AppState>,
) -> Result<warmup::WarmupResponse, String> {
    let trigger = match trigger.as_str() {
        "launch" => warmup::WarmupTrigger::Launch,
        "model_change" => warmup::WarmupTrigger::ModelChange,
        "workspace_change" => warmup::WarmupTrigger::WorkspaceChange,
        "session_resume" => warmup::WarmupTrigger::SessionResume,
        _ => warmup::WarmupTrigger::Launch,
    };

    state
        .warmup_client
        .warmup(&session_id, &model, trigger)
        .await
}

#[command]
pub fn should_rewarm_cache(state: State<'_, AppState>) -> bool {
    state.warmup_client.should_rewarm()
}
