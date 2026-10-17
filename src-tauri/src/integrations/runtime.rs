//! Request-scoped, one-use launch approvals. Saving/enabling a definition never
//! grants trust. Probe and live-connection tickets have distinct purposes.
use super::{
    config::ConnectionConfig,
    connections::{self, ConnectionRegistry, ConnectionStatus},
    credentials::SecretStore,
    identity::WorkspaceIdentity,
    launch::PreparedTransport,
    probe::{self, ProbeResult},
    process::SupervisedProcess,
    store::IntegrationStore,
    RuntimeError,
};
use crate::document_service::DocumentService;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const APPROVAL_TTL: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Serialize)]
pub struct LaunchReview {
    pub ticket_id: Uuid,
    #[serde(flatten)]
    pub target: ReviewTarget,
}

/// What the user approves. Header values and environment values are never shown.
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewTarget {
    Process {
        executable: PathBuf,
        cwd: PathBuf,
        args: Vec<String>,
    },
    Http {
        url: String,
        header_names: Vec<String>,
        insecure: bool,
        /// Requests carry the user's OAuth sign-in for this server.
        oauth: bool,
    },
}

impl From<&PreparedTransport> for ReviewTarget {
    fn from(prepared: &PreparedTransport) -> Self {
        match prepared {
            PreparedTransport::Process(process) => Self::Process {
                executable: process.executable.clone(),
                cwd: process.cwd.clone(),
                args: process.args.clone(),
            },
            PreparedTransport::Http(http) => Self::Http {
                url: http.url.clone(),
                header_names: http.header_names.clone(),
                insecure: http.insecure,
                oauth: http.oauth.is_some(),
            },
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Probe,
    Connection,
    Agent,
}

struct Ticket {
    purpose: Purpose,
    integration: Uuid,
    revision: String,
    workspace: WorkspaceIdentity,
    fingerprint: String,
    expires: Instant,
    cancel: CancellationToken,
    workspace_cancel: CancellationToken,
}

#[derive(Default)]
struct Requests {
    pending: HashMap<Uuid, Ticket>,
    running: HashMap<Uuid, (Uuid, CancellationToken)>,
}

pub struct IntegrationRuntime {
    pub(super) directory: PathBuf,
    pub(super) secrets: Arc<dyn SecretStore>,
    requests: Mutex<Requests>,
    preparing: Arc<tokio::sync::Semaphore>,
    pub connections: ConnectionRegistry,
    pub(super) shutting_down: AtomicBool,
    pub(super) permissions: super::permissions::PermissionRegistry,
    pub(super) invoking: Arc<tokio::sync::Semaphore>,
    /// The one interactive OAuth sign-in that may run at a time.
    signing_in: Mutex<Option<(Uuid, CancellationToken)>>,
    pub agents: Arc<super::agent_sessions::AgentSessions>,
}

impl IntegrationRuntime {
    pub fn new(directory: PathBuf, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            directory,
            secrets,
            requests: Mutex::new(Requests::default()),
            preparing: Arc::new(tokio::sync::Semaphore::new(4)),
            connections: ConnectionRegistry::default(),
            shutting_down: AtomicBool::new(false),
            permissions: Default::default(),
            invoking: Arc::new(tokio::sync::Semaphore::new(4)),
            signing_in: Mutex::new(None),
            agents: Default::default(),
        }
    }

    /// The saved URL and OAuth settings of a Streamable HTTP server that signs in.
    fn oauth_target(
        &self,
        integration: Uuid,
        revision: Option<&str>,
    ) -> Result<(String, super::config::OAuthSettings), RuntimeError> {
        let snapshot = IntegrationStore::new(self.directory.clone())
            .load()
            .map_err(|_| RuntimeError::ConfigChanged)?;
        if revision.is_some_and(|revision| revision != snapshot.revision) {
            return Err(RuntimeError::ConfigChanged);
        }
        match snapshot
            .config
            .entries
            .into_iter()
            .find(|entry| entry.id == integration)
            .map(|entry| entry.connection)
        {
            Some(ConnectionConfig::Mcp {
                transport:
                    super::config::McpTransport::StreamableHttp {
                        url,
                        oauth: Some(settings),
                        ..
                    },
            }) => Ok((url, settings)),
            Some(_) => Err(RuntimeError::OAuthUnsupported),
            None => Err(RuntimeError::ConfigChanged),
        }
    }

    /// Interactive sign-in against the saved configuration `revision`. On
    /// success, live connections and pending approvals for this server stop so
    /// the next connection uses the new sign-in.
    pub async fn sign_in(
        &self,
        integration: Uuid,
        revision: String,
        open: impl FnOnce(&str) -> Result<(), RuntimeError>,
        texts: super::oauth::CallbackPage,
    ) -> Result<(), RuntimeError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(RuntimeError::Cancelled);
        }
        let (url, settings) = self.oauth_target(integration, Some(&revision))?;
        let cancel = CancellationToken::new();
        let attempt = Uuid::new_v4();
        {
            let mut active = self.signing_in.lock().map_err(|_| RuntimeError::Busy)?;
            if active
                .as_ref()
                .is_some_and(|(_, active)| !active.is_cancelled())
            {
                return Err(RuntimeError::Busy);
            }
            *active = Some((attempt, cancel.clone()));
        }
        let result = super::oauth::sign_in(
            &url,
            &settings,
            integration,
            self.secrets.clone(),
            open,
            &texts,
            &cancel,
        )
        .await;
        if let Ok(mut active) = self.signing_in.lock() {
            if active.as_ref().is_some_and(|(id, _)| *id == attempt) {
                *active = None;
            }
        }
        if result.is_ok() {
            self.credentials_changed(integration);
        }
        result
    }

    pub fn cancel_sign_in(&self) {
        if let Some((_, cancel)) = self
            .signing_in
            .lock()
            .ok()
            .and_then(|mut active| active.take())
        {
            cancel.cancel();
        }
    }

    pub async fn sign_out(&self, integration: Uuid) -> Result<(), RuntimeError> {
        super::oauth::sign_out(self.secrets.clone(), integration).await?;
        self.credentials_changed(integration);
        Ok(())
    }

    /// Whether the saved server has stored tokens for its current URL.
    pub fn signed_in(&self, integration: Uuid) -> Result<bool, RuntimeError> {
        match self.oauth_target(integration, None) {
            Ok((url, _)) => super::oauth::signed_in(self.secrets.as_ref(), integration, &url),
            Err(RuntimeError::OAuthUnsupported) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn resolve(
        &self,
        integration: Uuid,
        revision: &str,
        documents: &DocumentService,
    ) -> Result<(PreparedTransport, bool), RuntimeError> {
        if documents.cancellation().is_cancelled() {
            return Err(RuntimeError::WorkspaceChanged);
        }
        let snapshot = IntegrationStore::new(self.directory.clone())
            .load()
            .map_err(|_| RuntimeError::ConfigChanged)?;
        if snapshot.revision != revision {
            return Err(RuntimeError::ConfigChanged);
        }
        let entry = snapshot
            .config
            .entries
            .iter()
            .find(|entry| entry.id == integration)
            .ok_or(RuntimeError::ConfigChanged)?;
        Ok((
            PreparedTransport::resolve(
                &entry.connection,
                integration,
                documents.workspace_root(),
                &self.secrets,
            )?,
            matches!(entry.connection, ConnectionConfig::Acp { .. }),
        ))
    }

    pub fn prepare(
        &self,
        integration: Uuid,
        revision: String,
        documents: &DocumentService,
    ) -> Result<LaunchReview, RuntimeError> {
        self.prepare_for(integration, revision, documents, Purpose::Probe)
    }

    /// A launch review for an ACP agent conversation.
    pub fn prepare_agent(
        &self,
        integration: Uuid,
        revision: String,
        documents: &DocumentService,
    ) -> Result<LaunchReview, RuntimeError> {
        self.prepare_for(integration, revision, documents, Purpose::Agent)
    }

    /// Starts the approved agent program. The ticket is used up either way.
    pub async fn launch_agent(
        self: &Arc<Self>,
        ticket_id: Uuid,
        documents: Arc<DocumentService>,
    ) -> Result<LaunchedAgent, RuntimeError> {
        let ticket = self.take_ticket(ticket_id, Purpose::Agent, &documents)?;
        let _running = RunningRequest {
            runtime: self.clone(),
            id: ticket_id,
        };
        let runtime = self.clone();
        let resolve_documents = documents.clone();
        let integration = ticket.integration;
        let revision = ticket.revision.clone();
        let preparation = tokio::task::spawn_blocking(move || {
            let definition = agent_policy(&runtime.directory, &resolve_documents, integration)?;
            let (prepared, _) = runtime.resolve(integration, &revision, &resolve_documents)?;
            Ok::<_, RuntimeError>((prepared, definition))
        });
        let work = async {
            let (prepared, definition) = preparation
                .await
                .map_err(|_| RuntimeError::LaunchFailed)??;
            if prepared.fingerprint() != ticket.fingerprint {
                return Err(RuntimeError::ConfigChanged);
            }
            if ticket.expires <= Instant::now() {
                return Err(RuntimeError::ApprovalExpired);
            }
            if ticket.cancel.is_cancelled() {
                return Err(RuntimeError::Cancelled);
            }
            if ticket.workspace_cancel.is_cancelled() {
                return Err(RuntimeError::WorkspaceChanged);
            }
            let PreparedTransport::Process(prepared) = prepared else {
                return Err(RuntimeError::UnsupportedTransport);
            };
            let (process, read, write) = SupervisedProcess::spawn(&prepared)?;
            Ok(LaunchedAgent {
                integration,
                name: definition.name,
                workspace: ticket.workspace.clone(),
                workspace_cancel: ticket.workspace_cancel.clone(),
                process,
                read: Box::new(read),
                write: Box::new(write),
            })
        };
        tokio::select! {
            biased;
            _ = ticket.workspace_cancel.cancelled() => Err(RuntimeError::WorkspaceChanged),
            _ = ticket.cancel.cancelled() => Err(RuntimeError::Cancelled),
            result = tokio::time::timeout(PROBE_TIMEOUT, work) => result.unwrap_or(Err(RuntimeError::TimedOut)),
        }
    }

    pub fn prepare_connection(
        &self,
        integration: Uuid,
        revision: String,
        documents: &DocumentService,
    ) -> Result<LaunchReview, RuntimeError> {
        self.prepare_for(integration, revision, documents, Purpose::Connection)
    }

    fn prepare_for(
        &self,
        integration: Uuid,
        revision: String,
        documents: &DocumentService,
        purpose: Purpose,
    ) -> Result<LaunchReview, RuntimeError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(RuntimeError::Cancelled);
        }
        let _slot = self
            .preparing
            .clone()
            .try_acquire_owned()
            .map_err(|_| RuntimeError::Busy)?;
        match purpose {
            Purpose::Connection => {
                connections::read_policy(&self.directory, documents, integration)?;
            }
            Purpose::Agent => {
                agent_policy(&self.directory, documents, integration)?;
            }
            Purpose::Probe => {}
        }
        let (prepared, _) = self.resolve(integration, &revision, documents)?;
        let review = LaunchReview {
            ticket_id: Uuid::new_v4(),
            target: ReviewTarget::from(&prepared),
        };
        let mut requests = self.requests.lock().map_err(|_| RuntimeError::Busy)?;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(RuntimeError::Cancelled);
        }
        requests.pending.retain(|_, ticket| {
            ticket.expires > Instant::now() && !ticket.workspace_cancel.is_cancelled()
        });
        if requests.pending.len() + requests.running.len() >= 32 {
            return Err(RuntimeError::Busy);
        }
        requests.pending.insert(
            review.ticket_id,
            Ticket {
                purpose,
                integration,
                revision,
                workspace: documents.identity().clone(),
                fingerprint: prepared.fingerprint().to_owned(),
                expires: Instant::now() + APPROVAL_TTL,
                cancel: CancellationToken::new(),
                workspace_cancel: documents.cancellation(),
            },
        );
        Ok(review)
    }

    /// Called from the final Tauri Exit event while the async runtime is alive.
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::Release);
        self.cancel_sign_in();
        self.agents.close_all();
        self.permissions.clear();
        if let Ok(mut requests) = self.requests.lock() {
            for (_, ticket) in requests.pending.drain() {
                ticket.cancel.cancel();
            }
            for (_, cancel) in requests.running.values() {
                cancel.cancel();
            }
        }
        self.connections.stop_all();
        let drained = async {
            loop {
                let requests_done = self
                    .requests
                    .lock()
                    .map(|requests| requests.running.is_empty())
                    .unwrap_or(false);
                if requests_done && self.connections.is_quiet() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        };
        if tokio::time::timeout(Duration::from_secs(5), drained)
            .await
            .is_err()
        {
            self.connections.abort_remaining();
            let _ = tokio::time::timeout(Duration::from_secs(1), async {
                while !self.connections.is_quiet() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await;
        }
    }

    pub fn credentials_changed(&self, integration: Uuid) {
        if let Ok(mut requests) = self.requests.lock() {
            requests.pending.retain(|_, ticket| {
                if ticket.integration == integration {
                    ticket.cancel.cancel();
                    false
                } else {
                    true
                }
            });
            for (id, cancel) in requests.running.values() {
                if *id == integration {
                    cancel.cancel();
                }
            }
        }
        self.connections.invalidate_secret(integration);
    }

    pub fn reconcile_connections(&self) {
        self.connections.reconcile(&self.directory);
    }

    pub fn cancel(&self, ticket_id: Uuid) {
        if let Ok(mut requests) = self.requests.lock() {
            if let Some(ticket) = requests.pending.remove(&ticket_id) {
                ticket.cancel.cancel();
            }
            if let Some((_, cancel)) = requests.running.get(&ticket_id) {
                cancel.cancel();
            }
        }
        self.connections.cancel_request(ticket_id);
    }

    fn take_ticket(
        &self,
        ticket_id: Uuid,
        purpose: Purpose,
        documents: &DocumentService,
    ) -> Result<Ticket, RuntimeError> {
        let mut requests = self.requests.lock().map_err(|_| RuntimeError::Busy)?;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(RuntimeError::Cancelled);
        }
        if requests
            .pending
            .get(&ticket_id)
            .is_some_and(|ticket| ticket.purpose != purpose)
        {
            return Err(RuntimeError::ApprovalExpired);
        }
        let ticket = requests
            .pending
            .remove(&ticket_id)
            .ok_or(RuntimeError::ApprovalExpired)?;
        if ticket.expires <= Instant::now() {
            return Err(RuntimeError::ApprovalExpired);
        }
        if ticket.workspace != *documents.identity() || ticket.workspace_cancel.is_cancelled() {
            return Err(RuntimeError::WorkspaceChanged);
        }
        if requests.running.len() >= 4 {
            return Err(RuntimeError::Busy);
        }
        requests
            .running
            .insert(ticket_id, (ticket.integration, ticket.cancel.clone()));
        Ok(ticket)
    }

    pub async fn connect(
        self: &Arc<Self>,
        ticket_id: Uuid,
        documents: Arc<DocumentService>,
    ) -> Result<ConnectionStatus, RuntimeError> {
        let ticket = self.take_ticket(ticket_id, Purpose::Connection, &documents)?;
        let _running = RunningRequest {
            runtime: self.clone(),
            id: ticket_id,
        };
        let runtime = self.clone();
        let resolve_documents = documents.clone();
        let integration = ticket.integration;
        let revision = ticket.revision.clone();
        let preparation = tokio::task::spawn_blocking(move || {
            let definition =
                connections::read_policy(&runtime.directory, &resolve_documents, integration)?;
            let (prepared, is_acp) = runtime.resolve(integration, &revision, &resolve_documents)?;
            if is_acp {
                return Err(RuntimeError::UnsupportedTransport);
            }
            Ok::<_, RuntimeError>((prepared, definition))
        });
        let work = async {
            let (prepared, definition) = preparation
                .await
                .map_err(|_| RuntimeError::LaunchFailed)??;
            if prepared.fingerprint() != ticket.fingerprint {
                return Err(RuntimeError::ConfigChanged);
            }
            if ticket.expires <= Instant::now() {
                return Err(RuntimeError::ApprovalExpired);
            }
            if ticket.cancel.is_cancelled() {
                return Err(RuntimeError::Cancelled);
            }
            if ticket.workspace_cancel.is_cancelled() {
                return Err(RuntimeError::WorkspaceChanged);
            }
            self.connections.start(
                ticket_id,
                self.directory.clone(),
                definition,
                documents,
                ticket.cancel.clone(),
                prepared,
                ticket.expires,
            )
        };
        tokio::select! {
            biased;
            _ = ticket.workspace_cancel.cancelled() => Err(RuntimeError::WorkspaceChanged),
            _ = ticket.cancel.cancelled() => Err(RuntimeError::Cancelled),
            result = tokio::time::timeout(PROBE_TIMEOUT, work) => result.unwrap_or(Err(RuntimeError::TimedOut)),
        }
    }

    pub async fn run(
        self: &Arc<Self>,
        ticket_id: Uuid,
        documents: Arc<DocumentService>,
    ) -> Result<ProbeResult, RuntimeError> {
        let ticket = self.take_ticket(ticket_id, Purpose::Probe, &documents)?;
        let _running = RunningRequest {
            runtime: self.clone(),
            id: ticket_id,
        };
        let runtime = self.clone();
        let integration = ticket.integration;
        let revision = ticket.revision.clone();
        let preparation = tokio::task::spawn_blocking(move || {
            runtime.resolve(integration, &revision, &documents)
        });
        let work = async {
            let (prepared, is_acp) = preparation
                .await
                .map_err(|_| RuntimeError::LaunchFailed)??;
            if prepared.fingerprint() != ticket.fingerprint {
                return Err(RuntimeError::ConfigChanged);
            }
            if ticket.expires <= Instant::now() {
                return Err(RuntimeError::ApprovalExpired);
            }
            if ticket.cancel.is_cancelled() {
                return Err(RuntimeError::Cancelled);
            }
            if ticket.workspace_cancel.is_cancelled() {
                return Err(RuntimeError::WorkspaceChanged);
            }
            let prepared = match prepared {
                PreparedTransport::Http(http) => return probe::mcp_http(integration, &http).await,
                PreparedTransport::Process(process) => process,
            };
            let (process, read, write) = SupervisedProcess::spawn(&prepared)?;
            let result = if is_acp {
                probe::acp(read, write).await
            } else {
                probe::mcp(integration, read, write).await
            };
            process.shutdown().await;
            result
        };
        tokio::select! {
            biased;
            _ = ticket.workspace_cancel.cancelled() => Err(RuntimeError::WorkspaceChanged),
            _ = ticket.cancel.cancelled() => Err(RuntimeError::Cancelled),
            result = tokio::time::timeout(PROBE_TIMEOUT, work) => result.unwrap_or(Err(RuntimeError::TimedOut)),
        }
    }
}

/// A started agent program, before the ACP handshake.
pub struct LaunchedAgent {
    pub integration: Uuid,
    pub name: String,
    pub workspace: WorkspaceIdentity,
    pub workspace_cancel: CancellationToken,
    pub process: SupervisedProcess,
    pub read: Box<dyn tokio::io::AsyncRead + Send + Unpin>,
    pub write: Box<dyn tokio::io::AsyncWrite + Send + Unpin>,
}

fn agent_policy(
    directory: &std::path::Path,
    documents: &DocumentService,
    integration: Uuid,
) -> Result<super::config::IntegrationDefinition, RuntimeError> {
    let entry = connections::read_entry_policy(directory, documents, integration)?;
    if !matches!(entry.connection, ConnectionConfig::Acp { .. }) {
        return Err(RuntimeError::UnsupportedTransport);
    }
    Ok(entry)
}

struct RunningRequest {
    runtime: Arc<IntegrationRuntime>,
    id: Uuid,
}
impl Drop for RunningRequest {
    fn drop(&mut self) {
        if let Ok(mut requests) = self.runtime.requests.lock() {
            requests.running.remove(&self.id);
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::integrations::config::{
        ConfigValue, IntegrationConfig, IntegrationDefinition, ProcessConfig,
    };
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct Secrets(Mutex<HashMap<(Uuid, String), String>>);
    impl SecretStore for Secrets {
        fn get(&self, id: Uuid, name: &str) -> Result<Option<String>, RuntimeError> {
            Ok(self.0.lock().unwrap().get(&(id, name.into())).cloned())
        }
    }
    struct Fixture {
        _root: tempfile::TempDir,
        runtime: Arc<IntegrationRuntime>,
        documents: Arc<DocumentService>,
        secrets: Arc<Secrets>,
        revision: String,
        id: Uuid,
    }
    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().unwrap();
            let id = Uuid::new_v4();
            let secrets = Arc::new(Secrets::default());
            secrets
                .0
                .lock()
                .unwrap()
                .insert((id, "key".into()), "fixture-secret".into());
            let config = IntegrationConfig {
                entries: vec![IntegrationDefinition {
                    id,
                    name: "Fixture".into(),
                    enabled: false,
                    connection: ConnectionConfig::Acp {
                        process: ProcessConfig {
                            command: "/bin/sh".into(),
                            args: vec!["-c".into(), "sleep 60".into()],
                            cwd: None,
                            env: BTreeMap::from([(
                                "TOKEN".into(),
                                ConfigValue::Secret { name: "key".into() },
                            )]),
                        },
                        mcp_servers: vec![],
                    },
                }],
                ..Default::default()
            };
            let revision = IntegrationStore::new(root.path().into())
                .save("missing", config)
                .unwrap()
                .revision;
            Self {
                runtime: Arc::new(IntegrationRuntime::new(root.path().into(), secrets.clone())),
                documents: Arc::new(DocumentService::new(root.path().into())),
                _root: root,
                secrets,
                revision,
                id,
            }
        }
        fn prepare(&self) -> LaunchReview {
            self.runtime
                .prepare(self.id, self.revision.clone(), &self.documents)
                .unwrap()
        }
    }

    #[tokio::test]
    async fn approvals_are_single_use_and_bound_to_configuration_and_workspace() {
        let fixture = Fixture::new();
        let review = fixture.prepare();
        let json = serde_json::to_string(&review).unwrap();
        assert!(!json.contains("fixture-secret"));
        fixture.runtime.cancel(review.ticket_id);
        assert!(matches!(
            fixture
                .runtime
                .run(review.ticket_id, fixture.documents.clone())
                .await,
            Err(RuntimeError::ApprovalExpired)
        ));
        let review = fixture.prepare();
        let reopened = Arc::new(DocumentService::new(
            fixture.documents.workspace_root().into(),
        ));
        assert!(matches!(
            fixture.runtime.run(review.ticket_id, reopened).await,
            Err(RuntimeError::WorkspaceChanged)
        ));
        let review = fixture.prepare();
        fixture
            .secrets
            .0
            .lock()
            .unwrap()
            .insert((fixture.id, "key".into()), "changed-secret".into());
        assert!(matches!(
            fixture
                .runtime
                .run(review.ticket_id, fixture.documents.clone())
                .await,
            Err(RuntimeError::ConfigChanged)
        ));
        assert!(fixture.runtime.requests.lock().unwrap().running.is_empty());
        let review = fixture.prepare();
        fixture
            .runtime
            .requests
            .lock()
            .unwrap()
            .pending
            .get_mut(&review.ticket_id)
            .unwrap()
            .expires = Instant::now();
        assert!(matches!(
            fixture
                .runtime
                .run(review.ticket_id, fixture.documents.clone())
                .await,
            Err(RuntimeError::ApprovalExpired)
        ));
    }

    #[tokio::test]
    async fn cancelling_one_request_never_cancels_another_and_retirement_cancels_both() {
        let fixture = Fixture::new();
        let first = fixture.prepare();
        let second = fixture.prepare();
        let mut a = Box::pin(
            fixture
                .runtime
                .run(first.ticket_id, fixture.documents.clone()),
        );
        let mut b = Box::pin(
            fixture
                .runtime
                .run(second.ticket_id, fixture.documents.clone()),
        );
        tokio::select! { _ = &mut a => panic!("hung fixture completed"), _ = &mut b => panic!("hung fixture completed"), _ = tokio::time::sleep(Duration::from_millis(50)) => {} }
        fixture.runtime.cancel(first.ticket_id);
        assert_eq!(a.await.unwrap_err(), RuntimeError::Cancelled);
        assert!(
            !fixture.runtime.requests.lock().unwrap().running[&second.ticket_id]
                .1
                .is_cancelled()
        );
        fixture.documents.retire();
        assert_eq!(b.await.unwrap_err(), RuntimeError::WorkspaceChanged);
        assert!(fixture.runtime.requests.lock().unwrap().running.is_empty());
    }

    #[test]
    fn credentials_are_scoped_by_integration_id() {
        let fixture = Fixture::new();
        assert_eq!(fixture.secrets.get(Uuid::new_v4(), "key").unwrap(), None);
        assert!(fixture.secrets.get(fixture.id, "key").unwrap().is_some());
    }

    #[tokio::test]
    async fn stalled_peer_hits_the_runtime_deadline_and_releases_its_request() {
        let fixture = Fixture::new();
        let review = fixture.prepare();
        let result = tokio::time::timeout(
            Duration::from_secs(25),
            fixture
                .runtime
                .run(review.ticket_id, fixture.documents.clone()),
        )
        .await
        .unwrap();
        assert_eq!(result.unwrap_err(), RuntimeError::TimedOut);
        assert!(fixture.runtime.requests.lock().unwrap().running.is_empty());
    }

    #[tokio::test]
    async fn agents_launch_only_from_an_agent_ticket_under_workspace_policy() {
        let mut fixture = Fixture::new();
        assert_eq!(
            fixture
                .runtime
                .prepare_agent(fixture.id, fixture.revision.clone(), &fixture.documents)
                .err(),
            Some(RuntimeError::IntegrationDisabled)
        );
        let store = IntegrationStore::new(fixture.runtime.directory.clone());
        let mut snapshot = store.load().unwrap();
        snapshot.config.entries[0].enabled = true;
        fixture.revision = store
            .save(&snapshot.revision, snapshot.config)
            .unwrap()
            .revision;
        assert_eq!(
            fixture
                .runtime
                .prepare_connection(fixture.id, fixture.revision.clone(), &fixture.documents)
                .err(),
            Some(RuntimeError::UnsupportedTransport),
            "an agent is not an MCP server"
        );
        let review = fixture
            .runtime
            .prepare_agent(fixture.id, fixture.revision.clone(), &fixture.documents)
            .unwrap();
        assert!(matches!(review.target, ReviewTarget::Process { .. }));
        assert_eq!(
            fixture
                .runtime
                .connect(review.ticket_id, fixture.documents.clone())
                .await
                .err(),
            Some(RuntimeError::ApprovalExpired),
            "an agent ticket does not start a connection"
        );
        let review = fixture
            .runtime
            .prepare_agent(fixture.id, fixture.revision.clone(), &fixture.documents)
            .unwrap();
        let launched = fixture
            .runtime
            .launch_agent(review.ticket_id, fixture.documents.clone())
            .await
            .unwrap();
        assert_eq!(launched.integration, fixture.id);
        assert_eq!(launched.name, "Fixture");
        assert_eq!(launched.workspace, *fixture.documents.identity());
        launched.process.shutdown().await;
        assert_eq!(
            fixture
                .runtime
                .launch_agent(review.ticket_id, fixture.documents.clone())
                .await
                .err(),
            Some(RuntimeError::ApprovalExpired),
            "tickets are single use"
        );
        assert!(fixture.runtime.requests.lock().unwrap().running.is_empty());
    }

    #[tokio::test]
    async fn a_saved_revision_change_revokes_a_prepared_launch() {
        let fixture = Fixture::new();
        let review = fixture.prepare();
        let store = IntegrationStore::new(fixture.runtime.directory.clone());
        let mut snapshot = store.load().unwrap();
        snapshot.config.entries[0].name = "Renamed".into();
        store.save(&snapshot.revision, snapshot.config).unwrap();
        assert_eq!(
            fixture
                .runtime
                .run(review.ticket_id, fixture.documents.clone())
                .await
                .unwrap_err(),
            RuntimeError::ConfigChanged
        );
    }
}
