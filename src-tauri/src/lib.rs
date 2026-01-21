pub mod agentic_loop;
pub mod ai_workflow;
pub mod app_state;
pub mod blade_client;
pub mod blade_protocol;
pub mod blade_ws_client;
pub mod chat;
pub mod chat_manager;
pub mod chat_orchestrator;
pub mod commands;
pub mod config;
pub mod context_assembly;
pub mod conversation;
pub mod conversation_store;
pub mod ephemeral_commands;
pub mod ephemeral_documents;
pub mod events;
pub mod explorer;
pub mod fs_watcher;
pub mod git;
pub mod idempotency;
pub mod language_service;
pub mod local_artifacts;
pub mod local_index;
pub mod lsp;
pub mod models;
pub mod project;
pub mod project_settings;
pub mod project_state;
pub mod protocol;
pub mod protocol_dispatcher;
pub mod reasoning_parser;
pub mod semantic_patch;
pub mod symbol_index;
pub mod terminal;
pub mod tool_execution;
pub mod tools;
pub mod tree_sitter;
pub mod utils;
pub mod warmup;
pub mod workflow_controller;
pub mod workspace_manager;
pub mod xml_parser;

pub use app_state::AppState;
use clap::Parser;
use tauri::Manager;

/// ZaguanBlade - AI-Native Intelligent Code Editor
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Optional path to open as workspace root
    #[arg(value_name = "PATH")]
    pub path: Option<String>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let cli = Cli::parse();

    // Resolve relative paths (like "." or "..") to absolute paths
    let resolved_path = cli.path.map(|p| {
        let path = std::path::PathBuf::from(&p);
        if path.is_relative() {
            // Resolve relative to current working directory
            std::env::current_dir()
                .ok()
                .map(|cwd| cwd.join(&path))
                .and_then(|full| std::fs::canonicalize(&full).ok())
                .map(|abs| abs.to_string_lossy().to_string())
                .unwrap_or(p)
        } else {
            // Already absolute, just canonicalize if possible
            std::fs::canonicalize(&path)
                .map(|abs| abs.to_string_lossy().to_string())
                .unwrap_or(p)
        }
    });

    tauri::Builder::default()
        .setup(|app| {
            let state = app.state::<AppState>();
            crate::fs_watcher::restart_fs_watcher(&app.handle(), &state);
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::new(resolved_path))
        .manage(terminal::TerminalManager::new())
        .invoke_handler(tauri::generate_handler![
            // Misc
            commands::misc::greet,
            commands::misc::toggle_devtools,
            commands::misc::log_frontend,
            commands::misc::set_virtual_buffer,
            commands::misc::clear_virtual_buffer,
            commands::misc::has_virtual_buffer,
            commands::misc::get_virtual_files,
            // Files
            commands::files::open_workspace,
            commands::files::list_files,
            commands::files::read_file_content,
            commands::files::write_file_content,
            // Project
            commands::project::read_binary_file,
            commands::project::get_recent_workspaces,
            commands::project::get_current_workspace,
            commands::project::load_project_state,
            commands::project::save_project_state,
            commands::project::get_project_state_path,
            commands::project::get_user_id,
            commands::project::get_project_id,
            commands::project::load_project_settings,
            commands::project::save_project_settings,
            commands::project::init_zblade_directory,
            commands::project::has_zblade_directory,
            // Settings
            commands::settings::get_global_settings,
            commands::settings::save_global_settings,
            // Chat
            commands::chat::send_message,
            commands::chat::list_models,
            commands::chat::get_conversation,
            commands::chat::list_conversations,
            commands::chat::load_conversation,
            commands::chat::new_conversation,
            commands::chat::delete_conversation,
            commands::chat::save_conversation,
            commands::chat::set_selected_model,
            commands::chat::get_selected_model,
            // Tools & Changes
            commands::tools::submit_command_result,
            commands::tools::approve_tool_decision,
            commands::changes::approve_change,
            commands::changes::approve_changes_for_file,
            commands::changes::approve_all_changes,
            commands::changes::reject_change,
            // Cache
            commands::cache::warmup_cache,
            commands::cache::should_rewarm_cache,
            // Local Context
            commands::local_context::list_local_conversations,
            commands::local_context::load_local_conversation,
            commands::local_context::search_local_moments,
            commands::local_context::get_file_context,
            commands::local_context::delete_local_conversation,
            // Git commands
            git::git_status_summary,
            git::git_status_files,
            git::git_stage_file,
            git::git_unstage_file,
            git::git_stage_all,
            git::git_unstage_all,
            git::git_commit,
            git::git_push,
            git::git_diff,
            git::git_generate_commit_message,
            git::git_generate_commit_message_ai,
            // Ephemeral
            ephemeral_commands::create_ephemeral_document,
            ephemeral_commands::get_ephemeral_document,
            ephemeral_commands::update_ephemeral_document,
            ephemeral_commands::close_ephemeral_document,
            ephemeral_commands::list_ephemeral_documents,
            ephemeral_commands::save_ephemeral_document,
            // Protocol Dispatcher
            protocol_dispatcher::dispatch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
