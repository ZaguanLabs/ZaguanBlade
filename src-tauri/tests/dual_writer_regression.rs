//! Regression coverage for the former incompatible dual-writer path.
//!
//! See: docs/internal/2026-07-24-performance-optimization-plan.md §P0.5

use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use zblade_lib::conversation_store::{ConversationMetadata, ConversationStore, StoredConversation};
use zblade_lib::local_artifacts::{ConversationArtifact, LocalArtifactStore};

fn temp_project_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "zblade_dual_writer_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn stores_resolve_to_different_conversation_paths() {
    let project_dir = temp_project_dir();

    // ConversationStore path
    let conv_store_path = project_dir.join("conversations");
    let _conv_store = ConversationStore::new(conv_store_path.clone()).unwrap();

    // LocalArtifactStore path (now separated to avoid dual-writer collision)
    let artifact_store = LocalArtifactStore::new(&project_dir);
    let artifact_dir = artifact_store.conversations_dir_test();

    // After the P1 fix, the two stores must NOT resolve to the same directory.
    assert_ne!(
        conv_store_path, artifact_dir,
        "ConversationStore and LocalArtifactStore must use separate directories \
         to avoid the dual-writer schema collision bug"
    );

    // Clean up
    let _ = fs::remove_dir_all(&project_dir);
}

#[test]
fn stores_no_longer_clobber_each_other() {
    let project_dir = temp_project_dir();
    let conv_id = "test-conv-456";
    let now = Utc::now();

    // 1. Write via ConversationStore (StoredConversation schema)
    let conv_store_path = project_dir.join("conversations");
    let mut conv_store = ConversationStore::new(conv_store_path.clone()).unwrap();

    let stored = StoredConversation {
        metadata: ConversationMetadata {
            id: conv_id.to_string(),
            title: "Test Conversation".to_string(),
            created_at: now,
            updated_at: now,
            model_id: "test-model".to_string(),
            message_count: 1,
            session_id: Some("session-1".to_string()),
            planning_mode: Some(false),
            runtime_mode: Some("code".to_string()),
            mode_source: Some("manual".to_string()),
            format_version: None,
        },
        messages: vec![],
    };
    conv_store.save_conversation(&stored).unwrap();

    let conv_file = conv_store_path.join(conv_id).join("metadata.json");
    assert!(
        conv_file.exists(),
        "ConversationStore wrote to {conv_file:?}"
    );

    // 2. Write via LocalArtifactStore (ConversationArtifact schema)
    let artifact_store = LocalArtifactStore::new(&project_dir);
    let artifact = ConversationArtifact::new(
        conv_id.to_string(),
        "test-project".to_string(),
        "Test Artifact".to_string(),
    );
    artifact_store.save_conversation(&artifact).unwrap();

    // The LocalArtifactStore file should be in a separate directory
    let artifact_file = artifact_store
        .conversations_dir_test()
        .join(format!("{}.json", conv_id));
    assert!(
        artifact_file.exists(),
        "LocalArtifactStore wrote to {artifact_file:?}"
    );
    assert_ne!(
        conv_file, artifact_file,
        "The two stores must write to separate files"
    );

    // 3. ConversationStore can still load its own data (not clobbered)
    let load_result = conv_store.load_conversation(conv_id);
    assert!(
        load_result.is_ok(),
        "ConversationStore should still load its file after LocalArtifactStore writes to a separate path"
    );

    // Clean up
    let _ = fs::remove_dir_all(&project_dir);
}
