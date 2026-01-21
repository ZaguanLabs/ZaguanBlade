use crate::app_state::AppState;

pub fn check_batch_completion(state: &AppState) {
    let mut batch_guard = state.pending_batch.lock().unwrap();
    if let Some(batch) = batch_guard.as_mut() {
        // If nothing is pending, consider the batch complete
        let no_pending_items =
            batch.commands.is_empty() && batch.changes.is_empty() && batch.confirms.is_empty();

        // A batch is complete if all calls have a corresponding result in file_results
        let all_addressed = batch.calls.iter().all(|call| {
            batch
                .file_results
                .iter()
                .any(|(res_call, _)| res_call.id == call.id)
        });

        if no_pending_items || all_addressed {
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
