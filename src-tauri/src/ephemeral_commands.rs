use crate::AppState;
use tauri::State;
use tokio::fs;

#[tauri::command]
pub fn close_ephemeral_document(id: String, state: State<'_, AppState>) -> Result<(), String> {
    if state.ephemeral_docs.remove(&id) {
        Ok(())
    } else {
        Err("Document not found".to_string())
    }
}
#[tauri::command]
pub async fn save_ephemeral_document_to_workspace(
    id: String,
    filename: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let doc = state
        .ephemeral_docs
        .get(&id)
        .ok_or_else(|| "Document not found".to_string())?;

    // Get workspace root
    let workspace_root = {
        let workspace = state.workspace.lock().unwrap();
        workspace
            .workspace
            .clone()
            .ok_or_else(|| "No workspace open".to_string())?
    };

    // Add timestamp to filename
    // Extract base name and extension
    let timestamp = chrono::Local::now().format("%Y-%m-%d-%H.%M.%S");
    let timestamped_filename = if let Some(dot_pos) = filename.rfind('.') {
        let (base, ext) = filename.split_at(dot_pos);
        format!("{}-{}{}", base, timestamp, ext)
    } else {
        format!("{}-{}", filename, timestamp)
    };

    // Create full path in workspace root
    let file_path = workspace_root.join(&timestamped_filename);
    let file_path_str = file_path.to_string_lossy().to_string();

    // Write file
    fs::write(&file_path, &doc.content)
        .await
        .map_err(|e| e.to_string())?;

    println!("[EPHEMERAL] Saved document to workspace: {}", file_path_str);

    // Remove from ephemeral storage after successful save
    state.ephemeral_docs.remove(&id);

    // Return the relative path (just the filename since it's in root)
    Ok(timestamped_filename)
}
