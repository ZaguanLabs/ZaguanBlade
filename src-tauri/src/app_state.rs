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
use crate::worktree::WorktreeStore;
use crate::ws_connection_manager::WsConnectionManager;
use dotenvy::dotenv;
use notify::RecommendedWatcher;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64},
    Arc, Mutex, RwLock,
};

fn project_data_root(workspace_root: Option<&PathBuf>) -> PathBuf {
    workspace_root
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".zblade")
}

pub struct FrontendWatchdogState {
    pub last_ping_ms: AtomicU64,
    pub last_recovery_ms: AtomicU64,
    pub recovery_attempts: AtomicU64,
}

impl FrontendWatchdogState {
    fn new() -> Self {
        Self {
            last_ping_ms: AtomicU64::new(0),
            last_recovery_ms: AtomicU64::new(0),
            recovery_attempts: AtomicU64::new(0),
        }
    }
}

pub struct AppState {
    pub chat_manager: Mutex<ChatManager>,
    pub conversation: Mutex<ConversationHistory>,
    pub conversation_store: Mutex<Option<conversation_store::ConversationStore>>,
    pub workspace: Mutex<WorkspaceManager>,
    pub config: Mutex<ApiConfig>,
    pub workflow: Mutex<AiWorkflow>,

    pub pending_approval: Mutex<Option<tokio::sync::oneshot::Sender<bool>>>,
    pub sso_cancel: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    pub pending_batch: Mutex<Option<crate::ai_workflow::PendingToolBatch>>,
    pub selected_model_index: Mutex<usize>,
    pub ephemeral_docs: ephemeral_documents::EphemeralDocumentStore,
    pub active_file: Mutex<Option<String>>,
    pub open_files: Mutex<Vec<String>>,
    pub cursor_line: Mutex<Option<usize>>,
    pub cursor_column: Mutex<Option<usize>>,
    pub selection_start_line: Mutex<Option<usize>>,
    pub selection_end_line: Mutex<Option<usize>>,
    pub approved_command_roots: Mutex<std::collections::HashSet<String>>,
    pub executing_commands: std::sync::Arc<
        Mutex<std::collections::HashMap<String, std::sync::Arc<std::sync::atomic::AtomicBool>>>,
    >,
    pub idempotency_cache: crate::idempotency::IdempotencyCache, // v1.1: Idempotency support
    pub warmup_client: warmup::WarmupClient,                     // v2.1: Cache warmup
    pub user_id: Mutex<Option<String>>, // Authenticated user ID from WebSocket
    pub protocol_version_emitted: AtomicBool, // Track protocol version broadcast
    pub startup_services_started: AtomicBool, // Delay heavy startup work until frontend is ready
    pub fs_watcher: Mutex<Option<RecommendedWatcher>>, // Workspace file watcher
    pub history_service: RwLock<Option<std::sync::Arc<crate::history::HistoryService>>>, // File history service
    pub language_service: RwLock<Option<std::sync::Arc<crate::language_service::LanguageService>>>, // v1.3: Unified Language Service
    pub worktree: RwLock<Option<std::sync::Arc<WorktreeStore>>>, // Shared workspace snapshot/index
    pub uncommitted_changes: UncommittedChangeTracker, // Track AI changes pending accept/reject
    pub feature_flags: FeatureFlags,                   // Headless migration feature flags
    pub tabs: Mutex<Vec<crate::core_state::TabInfo>>,  // Headless: tab state
    pub active_tab_id: Mutex<Option<String>>,          // Headless: active tab ID
    pub ws_connection: Arc<WsConnectionManager>,       // Persistent WebSocket connection to zcoderd
    pub pending_error_feedback: Mutex<Option<String>>, // Recovery hint to prepend to next user message
    pub git_dir: RwLock<Option<PathBuf>>,              // Path to .git directory for gix::open()
    pub frontend_watchdog: FrontendWatchdogState,
    pub remote_control: crate::remote_control::RemoteControlService,
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

        // Blade URL is never read from config: the hardcoded endpoints in
        // blade_endpoint.rs are the only source of truth.
        let blade_url = config::default_blade_url();

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

        let mut workspace_manager = WorkspaceManager::new();
        // Override workspace if provided via CLI
        if let Some(path_str) = &initial_path {
            workspace_manager.set_workspace(std::path::PathBuf::from(path_str));
        }

        // Get or create user_id
        let user_id = config::get_or_create_user_id(&config_path);
        let user_id_state = if user_id.trim().is_empty() {
            None
        } else {
            Some(user_id.clone())
        };

        // Initialize warmup client with config values
        let warmup_client = warmup::WarmupClient::new(
            blade_url.clone(),
            config.api_key.clone(),
            user_id.clone(),
        );

        // Initialize WebSocket connection manager before config is moved
        let ws_connection = Arc::new(WsConnectionManager::new(
            blade_url,
            config.api_key.clone(),
        ));

        Self {
            chat_manager: Mutex::new(ChatManager::new(50)),
            conversation: Mutex::new(ConversationHistory::new()),
            conversation_store: Mutex::new(None),
            workspace: Mutex::new(workspace_manager),
            config: Mutex::new(config),
            workflow: Mutex::new(AiWorkflow::new()),
            pending_approval: Mutex::new(None),
            sso_cancel: Mutex::new(None),
            pending_batch: Mutex::new(None),
            selected_model_index: Mutex::new(initial_model_index),
            ephemeral_docs: ephemeral_documents::EphemeralDocumentStore::new(),
            active_file: Mutex::new(None),
            open_files: Mutex::new(Vec::new()),
            cursor_line: Mutex::new(None),
            cursor_column: Mutex::new(None),
            user_id: Mutex::new(user_id_state),
            selection_start_line: Mutex::new(None),
            selection_end_line: Mutex::new(None),
            approved_command_roots: Mutex::new(std::collections::HashSet::new()),
            executing_commands: std::sync::Arc::new(Mutex::new(std::collections::HashMap::new())),
            idempotency_cache: crate::idempotency::IdempotencyCache::default(), // 24h TTL
            warmup_client, // v2.1: Cache warmup
            fs_watcher: Mutex::new(None),
            protocol_version_emitted: AtomicBool::new(false),
            startup_services_started: AtomicBool::new(false),
            history_service: RwLock::new(None),
            language_service: RwLock::new(None),
            worktree: RwLock::new(None),
            uncommitted_changes: UncommittedChangeTracker::new(),
            feature_flags: FeatureFlags::new(),
            tabs: Mutex::new(Vec::new()),
            active_tab_id: Mutex::new(None),
            ws_connection,
            pending_error_feedback: Mutex::new(None),
            git_dir: RwLock::new(None),
            frontend_watchdog: FrontendWatchdogState::new(),
            remote_control: crate::remote_control::RemoteControlService::new(),
        }
    }

    pub fn workspace_root(&self) -> Option<PathBuf> {
        self.workspace
            .lock()
            .ok()
            .and_then(|workspace| workspace.workspace.clone())
    }

    fn ensure_project_data_dir(&self) -> Result<PathBuf, String> {
        let workspace_root = self.workspace_root();
        if let Some(ws_root) = workspace_root.as_ref() {
            crate::project_settings::init_zblade_dir(ws_root)?;
        } else {
            std::fs::create_dir_all(project_data_root(None)).map_err(|e| e.to_string())?;
        }
        Ok(project_data_root(workspace_root.as_ref()))
    }

    fn create_conversation_store(
        project_data_dir: &Path,
    ) -> Result<conversation_store::ConversationStore, String> {
        let storage_path = project_data_dir.join("artifacts").join("conversations");
        conversation_store::ConversationStore::new(storage_path).or_else(|e| {
            eprintln!("Failed to initialize conversation store: {}", e);
            conversation_store::ConversationStore::new(
                std::env::temp_dir().join("zblade_conversations"),
            )
        })
    }

    pub fn with_conversation_store<T, F>(&self, f: F) -> Result<T, String>
    where
        F: FnOnce(&mut conversation_store::ConversationStore) -> Result<T, String>,
    {
        let mut guard = self
            .conversation_store
            .lock()
            .map_err(|e| format!("Failed to lock conversation store: {}", e))?;
        if guard.is_none() {
            let project_data_dir = self.ensure_project_data_dir()?;
            *guard = Some(Self::create_conversation_store(&project_data_dir)?);
        }

        let store = guard
            .as_mut()
            .ok_or_else(|| "Conversation store failed to initialize".to_string())?;
        f(store)
    }

    pub fn history_service(&self) -> Result<Arc<crate::history::HistoryService>, String> {
        if let Some(service) = self
            .history_service
            .read()
            .map_err(|e| format!("Failed to lock history service: {}", e))?
            .clone()
        {
            return Ok(service);
        }

        let project_data_dir = self.ensure_project_data_dir()?;
        let service = Arc::new(crate::history::HistoryService::new(&project_data_dir));

        let mut guard = self
            .history_service
            .write()
            .map_err(|e| format!("Failed to write history service: {}", e))?;
        Ok(guard.get_or_insert_with(|| service).clone())
    }

    pub fn worktree(&self) -> Result<Arc<WorktreeStore>, String> {
        if let Some(store) = self
            .worktree
            .read()
            .map_err(|e| format!("Failed to lock worktree store: {}", e))?
            .clone()
        {
            return Ok(store);
        }

        let workspace_root = self
            .workspace_root()
            .ok_or_else(|| "No workspace open".to_string())?;
        let store = Arc::new(WorktreeStore::new(workspace_root)?);

        let mut guard = self
            .worktree
            .write()
            .map_err(|e| format!("Failed to write worktree store: {}", e))?;
        Ok(guard.get_or_insert_with(|| store).clone())
    }

    pub fn language_service(
        &self,
    ) -> Result<Arc<crate::language_service::LanguageService>, String> {
        if let Some(service) = self
            .language_service
            .read()
            .map_err(|e| format!("Failed to lock language service: {}", e))?
            .clone()
        {
            return Ok(service);
        }

        let project_data_dir = self.ensure_project_data_dir()?;
        let db_path = project_data_dir.join("index").join("symbols.db");
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }

        let workspace_root = self.workspace_root().unwrap_or_else(|| PathBuf::from("."));
        let symbol_store = Arc::new(
            crate::symbol_index::store::SymbolStore::new(&db_path)
                .map_err(|e| format!("Failed to create SymbolStore: {}", e))?,
        );
        let service = Arc::new(
            crate::language_service::LanguageService::new(workspace_root, symbol_store)
                .map_err(|e| format!("Failed to initialize LanguageService: {}", e))?,
        );
        if let Ok(worktree) = self.worktree() {
            service.set_worktree_store(worktree);
        }

        let mut guard = self
            .language_service
            .write()
            .map_err(|e| format!("Failed to write language service: {}", e))?;
        Ok(guard.get_or_insert_with(|| service).clone())
    }

    pub fn language_handler(&self) -> Result<crate::language_service::LanguageHandler, String> {
        Ok(crate::language_service::LanguageHandler::new(
            self.language_service()?,
        ))
    }

    pub fn reset_project_services(&self) -> Result<(), String> {
        *self
            .conversation_store
            .lock()
            .map_err(|e| format!("Failed to lock conversation store: {}", e))? = None;
        *self
            .history_service
            .write()
            .map_err(|e| format!("Failed to write history service: {}", e))? = None;
        *self
            .language_service
            .write()
            .map_err(|e| format!("Failed to write language service: {}", e))? = None;
        *self
            .worktree
            .write()
            .map_err(|e| format!("Failed to write worktree store: {}", e))? = None;
        Ok(())
    }

    pub async fn is_local_model_active(&self) -> bool {
        let config = { self.config.lock().unwrap().clone() };
        let models = crate::models::catalog::list_all_models(&config).await;
        let selected_model = *self.selected_model_index.lock().unwrap();
        if !models.is_empty() && selected_model < models.len() {
            if let Some(m) = models.get(selected_model) {
                if let Some(ref provider) = m.provider {
                    let provider_id = crate::providers::ProviderId::from_provider_str(provider);
                    return matches!(
                        provider_id,
                        crate::providers::ProviderId::Ollama
                            | crate::providers::ProviderId::OpenAiCompat
                    );
                }
            }
        }
        false
    }
}
