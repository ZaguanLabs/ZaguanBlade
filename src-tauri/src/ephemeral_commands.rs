use crate::ephemeral_documents::EphemeralDocument;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn create_ephemeral_document(
    content: String,
    suggested_name: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    println!(
        "[EPHEMERAL RUST] Creating ephemeral document: {} ({} bytes)",
        suggested_name,
        content.len()
    );
    let id = state.ephemeral_docs.create(content, suggested_name);
    println!("[EPHEMERAL RUST] Document created with ID: {}", id);
    Ok(id)
}

#[tauri::command]
pub fn get_ephemeral_document(
    id: String,
    state: State<'_, AppState>,
) -> Result<EphemeralDocument, String> {
    state
        .ephemeral_docs
        .get(&id)
        .ok_or_else(|| "Document not found".to_string())
}

#[tauri::command]
pub fn update_ephemeral_document(
    id: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if state.ephemeral_docs.update_content(&id, content) {
        Ok(())
    } else {
        Err("Document not found".to_string())
    }
}

#[tauri::command]
pub fn close_ephemeral_document(id: String, state: State<'_, AppState>) -> Result<(), String> {
    if state.ephemeral_docs.remove(&id) {
        Ok(())
    } else {
        Err("Document not found".to_string())
    }
}

#[tauri::command]
pub fn list_ephemeral_documents(
    state: State<'_, AppState>,
) -> Result<Vec<EphemeralDocument>, String> {
    Ok(state.ephemeral_docs.list())
}

#[tauri::command]
pub async fn save_ephemeral_document(
    id: String,
    path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let doc = state
        .ephemeral_docs
        .get(&id)
        .ok_or_else(|| "Document not found".to_string())?;

    std::fs::write(&path, doc.content).map_err(|e| e.to_string())?;

    // Remove from ephemeral storage after successful save
    state.ephemeral_docs.remove(&id);

    Ok(())
}
