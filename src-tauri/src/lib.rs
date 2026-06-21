#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

pub mod agent_instructions;
pub mod agent_skills;
pub mod agentic_loop;
pub mod ai_workflow;
pub mod app_state;
pub mod blade_client;
pub mod blade_endpoint;
pub mod blade_event_scheduler;
pub mod blade_protocol;
pub mod blade_ws_client;
pub mod buffer_snapshot;
pub mod chat_manager;
pub mod chat_orchestrator;
pub mod commands;
pub mod config;
pub mod context_assembly;
pub mod context_pack;
pub mod conversation;
pub mod conversation_store;
pub mod core_state;
pub mod environment;
pub mod ephemeral_commands;
pub mod ephemeral_documents;
pub mod events;
pub mod explorer;
pub mod feature_flags;
pub mod file_state_sync;
pub mod fs_watcher;
pub mod git;
pub mod gitignore_filter;
pub mod history;
pub mod idempotency;
pub mod indexer;
pub mod language_service;
pub mod linux_webkit_workaround;
pub mod local_artifacts;
pub mod local_index;

pub mod models;
pub mod post_edit_validation;
pub mod project;
pub mod project_settings;
pub mod project_state;
pub mod protocol;
pub mod protocol_dispatcher;
pub mod providers;
pub mod reasoning_parser;
pub mod remote_control;
pub mod screenshot;
pub mod semantic_patch;
pub mod startup;
pub mod symbol_index;
pub mod telegram_service;
pub mod terminal;
pub mod tool_execution;
pub mod tools;
pub mod tree_sitter;
pub mod uncommitted_changes;
pub mod utils;
pub mod warmup;
pub mod workflow_controller;
pub mod workspace_manager;
pub mod worktree;
pub mod ws_connection_manager;
pub mod xml_parser;

pub use app_state::AppState;
use clap::Parser;

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
    crate::linux_webkit_workaround::apply();

    let cli = Cli::parse();

    // Resolve relative paths (like "." or "..") to absolute paths
    let resolved_path = cli.path.map(|p| {
        let path = std::path::PathBuf::from(&p);
        if path.is_relative() {
            // For AppImage: Use OWD (Original Working Directory) if available
            // AppImages change CWD to their mount point, but preserve the original in OWD
            let cwd = std::env::var("OWD")
                .ok()
                .map(std::path::PathBuf::from)
                .or_else(|| std::env::current_dir().ok());

            cwd.map(|dir| dir.join(&path))
                .and_then(|full| std::fs::canonicalize(&full).ok())
                .map(|abs| abs.to_string_lossy().to_string())
                .unwrap_or_else(|| {
                    eprintln!("[WARN] Failed to resolve relative path: {}", p);
                    p
                })
        } else {
            // Already absolute, just canonicalize if possible
            std::fs::canonicalize(&path)
                .map(|abs| abs.to_string_lossy().to_string())
                .unwrap_or(p)
        }
    });

    tauri::Builder::default()
        .manage(AppState::new(resolved_path))
        .manage(terminal::TerminalManager::new())
        .setup(|app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri::{Emitter, Manager};
                let state = handle.state::<AppState>();
                let config_path = crate::config::default_api_config_path();
                let config = crate::config::load_api_config(&config_path);

                let default_ollama_url = crate::config::default_ollama_url();
                let should_detect_local_ollama = {
                    let config = state.config.lock().unwrap();
                    !config.ollama_enabled
                        && (config.ollama_url.trim().is_empty()
                            || config.ollama_url.trim_end_matches('/')
                                == default_ollama_url.trim_end_matches('/'))
                };

                if should_detect_local_ollama
                    && crate::models::ollama::test_connection(&default_ollama_url)
                        .await
                        .is_ok()
                {
                    let updated_config = {
                        let mut config = state.config.lock().unwrap();
                        config.ollama_enabled = true;
                        config.ollama_url = default_ollama_url.clone();
                        config.clone()
                    };

                    crate::models::ollama::clear_cache();
                    let save_path = config_path.clone();
                    if let Err(e) = tokio::task::spawn_blocking(move || {
                        crate::config::save_api_config(&save_path, &updated_config)
                    })
                    .await
                    .map_err(|e| format!("auto-save Ollama settings task failed: {}", e))
                    .and_then(|result| result)
                    {
                        eprintln!("[CONFIG] Failed to persist detected Ollama settings: {}", e);
                    } else {
                        eprintln!(
                            "[CONFIG] Detected local Ollama at {}; enabled local AI settings",
                            default_ollama_url
                        );
                        let _ = handle.emit("local-ai-settings-changed", ());
                    }
                }

                if !config.telegram_bot_token.is_empty() {
                    // Start polling
                    let trimmed_token = config.telegram_bot_token.trim().to_string();
                    let client = reqwest::Client::new();
                    if let Ok(res) = client
                        .get(&format!(
                            "https://api.telegram.org/bot{}/getMe",
                            trimmed_token
                        ))
                        .send()
                        .await
                    {
                        if let Ok(get_me) = res.json::<serde_json::Value>().await {
                            if get_me["ok"].as_bool() == Some(true) {
                                if let Some(bot_username) = get_me["result"]["username"].as_str() {
                                    state
                                        .remote_control
                                        .set_configured(bot_username.to_string())
                                        .await;
                                    crate::telegram_service::start_polling(
                                        handle.clone(),
                                        trimmed_token,
                                        bot_username.to_string(),
                                    )
                                    .await;
                                }
                            }
                        }
                    }
                }
            });

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            // Misc
            commands::misc::greet,
            commands::misc::toggle_devtools,
            commands::misc::log_frontend,
            commands::misc::frontend_shell_ready,
            commands::misc::set_window_title,
            commands::misc::get_blade_event_metrics,
            commands::misc::get_runtime_debug_flags,
            // commands::misc::set_virtual_buffer,
            // commands::misc::clear_virtual_buffer,
            // commands::misc::has_virtual_buffer,
            // commands::misc::get_virtual_files,
            // Files
            commands::files::open_workspace,
            commands::files::list_files,
            commands::files::search_workspace_paths,
            commands::files::read_file_content,
            commands::files::write_file_content,
            commands::files::open_file_in_editor,
            // Project
            commands::project::read_binary_file,
            commands::project::get_recent_workspaces,
            commands::project::get_current_workspace,
            commands::project::load_project_state,
            commands::project::save_project_state,
            commands::project::graceful_shutdown_with_state,
            commands::project::get_project_state_path,
            commands::project::get_user_id,
            commands::project::get_project_id,
            commands::project::load_project_settings,
            commands::project::save_project_settings,
            commands::project::init_zblade_directory,
            commands::project::has_zblade_directory,
            // Screenshot
            commands::screenshot::list_capturable_windows,
            commands::screenshot::capture_window,
            commands::screenshot::capture_full_screen,
            commands::screenshot::capture_window_region,
            commands::screenshot::capture_screen_region,
            // Settings
            commands::settings_remote::get_remote_ai_settings,
            commands::settings_remote::save_remote_ai_settings,
            commands::settings_local_ai::get_local_ai_settings,
            commands::settings_local_ai::save_local_ai_settings,
            commands::settings_local_ai::test_local_ollama_connection,
            commands::settings_local_ai::refresh_local_ollama_models,
            commands::settings_local_ai::test_local_openai_compat_connection,
            commands::settings_local_ai::refresh_local_openai_compat_models,
            // Chat
            commands::chat::send_message,
            commands::chat::list_models,
            commands::chat::get_conversation,
            commands::chat::truncate_conversation,
            commands::chat::list_conversations,
            commands::chat::load_conversation,
            commands::chat::new_conversation,
            commands::chat::delete_conversation,
            commands::chat::save_conversation,
            commands::chat::set_selected_model,
            commands::chat::get_selected_model,
            commands::chat::get_chat_status,
            commands::chat::is_local_model_active,
            // Tools & Changes
            commands::tools::submit_command_result,
            commands::tools::approve_tool_decision,
            commands::tools::send_approval_response,
            commands::tools::approve_single_command,
            crate::terminal::execute_terminal_command,
            crate::terminal::execute_native_command,
            crate::terminal::cancel_command_execution,
            // History
            commands::history::get_file_history,
            commands::history::revert_file_to_snapshot,
            commands::history::undo_batch,
            // Uncommitted Changes (Accept/Reject)
            commands::uncommitted::get_uncommitted_changes,
            commands::uncommitted::get_uncommitted_change,
            commands::uncommitted::get_uncommitted_change_for_file,
            commands::uncommitted::accept_change,
            commands::uncommitted::accept_file_changes,
            commands::uncommitted::accept_all_changes,
            commands::uncommitted::reject_change,
            commands::uncommitted::reject_file_changes,
            commands::uncommitted::reject_all_changes,
            commands::uncommitted::get_uncommitted_changes_count,
            // Cache
            commands::cache::warmup_cache,
            commands::cache::should_rewarm_cache,
            // Local Context
            commands::local_context::list_local_conversations,
            commands::local_context::load_local_conversation,
            commands::local_context::search_local_moments,
            commands::local_context::get_file_context,
            commands::local_context::delete_local_conversation,
            // State (Headless Core)
            commands::state::bootstrap_state,
            commands::state::get_core_state,
            commands::state::get_feature_flags,
            commands::state::set_feature_flag,
            // Git commands
            git::git_status,
            git::git_status_summary,
            git::git_status_files,
            git::git_stage_file,
            git::git_unstage_file,
            git::git_stage_all,
            git::git_unstage_all,
            git::git_commit,
            git::git_commit_preflight,
            git::git_push,
            git::git_diff,
            git::git_generate_commit_message,
            git::git_generate_commit_message_ai,
            git::git_log,
            git::git_commit_stats,
            git::git_remote_url,
            // Remote Control
            commands::remote_control::get_remote_control_status,
            commands::remote_control::set_telegram_bot_token,
            commands::remote_control::disconnect_remote_control,
            // Ephemeral
            ephemeral_commands::create_ephemeral_document,
            ephemeral_commands::get_ephemeral_document,
            ephemeral_commands::update_ephemeral_document,
            ephemeral_commands::close_ephemeral_document,
            ephemeral_commands::list_ephemeral_documents,
            ephemeral_commands::save_ephemeral_document,
            ephemeral_commands::save_ephemeral_document_to_workspace,
            // Protocol Dispatcher
            protocol_dispatcher::dispatch,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
