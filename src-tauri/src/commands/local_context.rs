use crate::local_artifacts;
use crate::local_index;
use tauri::command;

#[command]
pub fn list_local_conversations(
    project_path: String,
) -> Result<Vec<local_index::ConversationIndex>, String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.list_conversations()
}

#[command]
pub fn load_local_conversation(
    project_path: String,
    conversation_id: String,
) -> Result<local_artifacts::ConversationArtifact, String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.load_conversation(&conversation_id)
}

#[command]
pub fn search_local_moments(
    project_path: String,
    query: String,
    limit: i32,
) -> Result<Vec<local_index::MomentIndex>, String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.search_moments(&query, limit)
}

#[command]
pub fn get_file_context(
    project_path: String,
    file_path: String,
) -> Result<Vec<local_index::CodeReferenceIndex>, String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.get_file_references(&file_path)
}

#[command]
pub fn delete_local_conversation(
    project_path: String,
    conversation_id: String,
) -> Result<(), String> {
    let path = std::path::PathBuf::from(project_path);
    let store = local_artifacts::LocalArtifactStore::new(&path);
    store.delete_conversation(&conversation_id)
}
