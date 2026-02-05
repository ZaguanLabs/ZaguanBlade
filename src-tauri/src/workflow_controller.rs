use crate::app_state::AppState;

pub fn check_batch_completion(state: &AppState) {
    let mut batch_guard = state.pending_batch.lock().unwrap();
    if let Some(batch) = batch_guard.as_mut() {
        // Check if all COMMANDS have results (not all calls - some calls are auto-executed)
        let all_commands_done = batch.commands.iter().all(|cmd| {
            batch.file_results.iter().any(|(res_call, _)| res_call.id == cmd.call.id)
        });
        
        // Check if all CONFIRMS have results
        let all_confirms_done = batch.confirms.iter().all(|conf| {
            batch.file_results.iter().any(|(res_call, _)| res_call.id == conf.call.id)
        });

        // Batch is complete when all pending items (commands + confirms) have results
        let is_complete = all_commands_done && all_confirms_done;

        if is_complete {
            let mut approval_guard = state.pending_approval.lock().unwrap();
            if let Some(tx) = approval_guard.take() {
                let _ = tx.send(true);
            }
        }
    } else {
        // No batch tracked; unblock any pending approval
        let mut approval_guard = state.pending_approval.lock().unwrap();
        if let Some(tx) = approval_guard.take() {
            let _ = tx.send(true);
        }
    }
}
