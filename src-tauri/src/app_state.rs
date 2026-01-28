use crate::ai_workflow::AiWorkflow;
use crate::chat_manager::ChatManager;
use crate::config::{self, ApiConfig};
use crate::conversation::ConversationHistory;
use crate::conversation_store;
use crate::ephemeral_documents;
use crate::feature_flags::FeatureFlags;
use crate::uncommitted_changes::UncommittedChangeTracker;
use crate::warmup;
use crate::workspace_manager::WorkspaceManager;
use crate::ws_connection_manager::WsConnectionManager;
use dotenvy::dotenv;
use notify::RecommendedWatcher;
use std::sync::{Arc, Mutex};

pub struct AppState {
    pub chat_manager: Mutex<ChatManager>,
    pub conversation: Mutex<ConversationHistory>,
    pub conversation_store: Mutex<conversation_store::ConversationStore>,
    pub workspace: Mutex<WorkspaceManager>,
    pub config: Mutex<ApiConfig>,
    pub workflow: Mutex<AiWorkflow>,

    pub pending_approval: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
    pub pending_batch: Mutex<Option<crate::ai_workflow::PendingToolBatch>>,
    pub selected_model_index: Mutex<usize>,
    pub ephemeral_docs: ephemeral_documents::EphemeralDocumentStore,
    pub active_file: Mutex<Option<String>>,
    pub open_files: Mutex<Vec<String>>,
    pub cursor_line: Mutex<Option<usize>>,
    pub cursor_column: Mutex<Option<usize>>,
    pub selection_start_line: Mutex<Option<usize>>,
    pub selection_end_line: Mutex<Option<usize>>,
    // virtual_buffers removed
    pub approved_command_roots: Mutex<std::collections::HashSet<String>>,
    pub executing_commands: std::sync::Arc<
        Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    >,
    pub idempotency_cache: crate::idempotency::IdempotencyCache, // v1.1: Idempotency support
    pub warmup_client: warmup::WarmupClient,                     // v2.1: Cache warmup
    pub user_id: Mutex<Option<String>>, // Authenticated user ID from WebSocket
    pub fs_watcher: Mutex<Option<RecommendedWatcher>>, // Workspace file watcher
    pub history_service: std::sync::Arc<crate::history::HistoryService>, // File history service
    pub language_service: std::sync::Arc<crate::language_service::LanguageService>, // v1.3: Unified Language Service
    pub language_handler: crate::language_service::LanguageHandler, // v1.3: Language Intent Handler
    pub uncommitted_changes: UncommittedChangeTracker, // Track AI changes pending accept/reject
    pub indexer_manager: Mutex<Option<crate::indexer::IndexerManager>>, // Project indexer
    pub feature_flags: FeatureFlags, // Headless migration feature flags
    pub tabs: Mutex<Vec<crate::core_state::TabInfo>>, // Headless: tab state
    pub active_tab_id: Mutex<Option<String>>, // Headless: active tab ID
    pub ws_connection: Arc<WsConnectionManager>, // Persistent WebSocket connection to zcoderd
}

impl AppState {
    pub fn new(initial_path: Option<String>) -> Self {
        // Load environment variables from .env file
        dotenv().ok();

        // Load config from disk
        let config_path = config::default_api_config_path();
        let mut config = config::load_api_config(&config_path);

        if let Err(e) = config::ensure_global_prompts_dir() {
            eprintln!("[CONFIG] Failed to ensure global prompts directory: {}", e);
        }

        // Fallback or override logic:
        // If config.blade_url is empty, use default or check environment variable.
        if config.blade_url.trim().is_empty() {
            if let Ok(url) = std::env::var("BLADE_URL") {
                config.blade_url = url;
            } else {
                config.blade_url = "https://coder.zaguanai.com".to_string();
            }
        }

        // Fallback for api_key from environment variable
        if config.api_key.trim().is_empty() {
            if let Ok(key) = std::env::var("BLADE_API_KEY") {
                config.api_key = key;
            }
        }

        // Initialize selected model index from config
        // We can't fetch models synchronously here, so we default to 0
        // The actual index will be corrected when models are fetched or when set_selected_model is called
        let initial_model_index = 0;

        // Initialize conversation store
        let storage_path = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("zaguan")
            .join("conversations");

        let conversation_store = conversation_store::ConversationStore::new(storage_path.clone())
            .unwrap_or_else(|e| {
                eprintln!("Failed to initialize conversation store: {}", e);
                // Fallback to temp directory
                conversation_store::ConversationStore::new(
                    std::env::temp_dir().join("zaguan_conversations"),
                )
                .expect("Failed to create conversation store in temp directory")
            });

        // Initialize History Service
        // Use zaguan/history in data dir
        let history_service = std::sync::Arc::new(crate::history::HistoryService::new(
            &storage_path.parent().unwrap(),
        ));

        let mut workspace_manager = WorkspaceManager::new();
        // Override workspace if provided via CLI
        if let Some(path_str) = &initial_path {
            workspace_manager.set_workspace(std::path::PathBuf::from(path_str));
        }

        // Get or create user_id
        let user_id = config::get_or_create_user_id(&config_path);

        // Initialize warmup client with config values
        let warmup_client = warmup::WarmupClient::new(
            config.blade_url.clone(),
            config.api_key.clone(),
            user_id.clone(),
        );

        // Initialize Language Service
        let db_path = dirs::data_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("zaguan")
            .join("symbols.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let ls_root = initial_path.as_deref().unwrap_or(".");

        let symbol_store = std::sync::Arc::new(
            crate::symbol_index::store::SymbolStore::new(&db_path)
                .expect("Failed to create SymbolStore"),
        );

        let language_service = std::sync::Arc::new(
            crate::language_service::LanguageService::new(
                std::path::PathBuf::from(ls_root),
                symbol_store,
            )
            .expect("Failed to initialize Language Service"),
        );

        // Initialize Language Handler

        let language_handler =
            crate::language_service::LanguageHandler::new(language_service.clone());

        // IndexerManager will be initialized asynchronously after AppState is created
        // This ensures GUI launches immediately without blocking on indexing
        let indexer_manager = None;

        // Initialize WebSocket connection manager before config is moved
        let ws_connection = Arc::new(WsConnectionManager::new(
            config.blade_url.clone(),
            config.api_key.clone(),
        ));

        Self {
            chat_manager: Mutex::new(ChatManager::new(10)),
            conversation: Mutex::new(ConversationHistory::new()),
            conversation_store: Mutex::new(conversation_store),
            workspace: Mutex::new(workspace_manager),
            config: Mutex::new(config),
            workflow: Mutex::new(AiWorkflow::new()),
            pending_approval: Mutex::new(None),
            pending_batch: Mutex::new(None),
            selected_model_index: Mutex::new(initial_model_index),
            ephemeral_docs: ephemeral_documents::EphemeralDocumentStore::new(),
            active_file: Mutex::new(None),
            open_files: Mutex::new(Vec::new()),
            cursor_line: Mutex::new(None),
            cursor_column: Mutex::new(None),
            user_id: Mutex::new(Some(user_id)),
            selection_start_line: Mutex::new(None),
            selection_end_line: Mutex::new(None),
            // virtual_buffers removed
            approved_command_roots: Mutex::new(std::collections::HashSet::new()),
            executing_commands: std::sync::Arc::new(Mutex::new(std::collections::HashMap::new())),
            idempotency_cache: crate::idempotency::IdempotencyCache::default(), // 24h TTL
            warmup_client, // v2.1: Cache warmup
            fs_watcher: Mutex::new(None),
            history_service,
            language_service,
            language_handler,
            uncommitted_changes: UncommittedChangeTracker::new(),
            indexer_manager: Mutex::new(indexer_manager),
            feature_flags: FeatureFlags::new(),
            tabs: Mutex::new(Vec::new()),
            active_tab_id: Mutex::new(None),
            ws_connection,
        }
    }
}
