//! Workspace-owned MCP process lifetimes. No tool execution is exposed here.
use super::{
    catalog::McpCatalog,
    config::{ConnectionConfig, IntegrationDefinition, McpTransport, MAX_CONFIG_BYTES},
    identity::WorkspaceIdentity,
    mcp_session::McpSession,
    process::{PreparedProcess, SupervisedProcess},
    store::IntegrationStore,
    RuntimeError,
};
use crate::document_service::DocumentService;
use serde::Serialize;
use std::{
    collections::HashMap,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_CONNECTIONS: usize = 4;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionPhase {
    Connecting,
    Connected,
    Refreshing,
    Stopping,
    Disconnected,
    Failed,
}
impl ConnectionPhase {
    fn active(self) -> bool {
        !matches!(self, Self::Disconnected | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionStatus {
    pub connection_id: Uuid,
    pub integration_id: Uuid,
    pub workspace: WorkspaceIdentity,
    pub phase: ConnectionPhase,
    pub protocol_version: Option<String>,
    pub catalog_revision: Option<String>,
    pub tools: usize,
    pub error: Option<RuntimeError>,
}
struct Snapshot {
    status: ConnectionStatus,
    catalog: Option<McpCatalog>,
}
struct Connection {
    definition: IntegrationDefinition,
    documents: Arc<DocumentService>,
    cancel: CancellationToken,
    refresh: mpsc::Sender<()>,
    snapshot: Mutex<Snapshot>,
    worker: Mutex<Option<tokio::task::AbortHandle>>,
}
impl Connection {
    fn stop(&self, error: Option<RuntimeError>) {
        if let Ok(mut snapshot) = self.snapshot.lock() {
            if snapshot.status.phase.active() {
                snapshot.status.phase = ConnectionPhase::Stopping;
                snapshot.status.error = error;
                snapshot.status.catalog_revision = None;
                snapshot.status.tools = 0;
                snapshot.catalog = None;
            }
        }
        self.cancel.cancel();
    }
    fn status(&self) -> Result<ConnectionStatus, RuntimeError> {
        self.snapshot
            .lock()
            .map(|snapshot| snapshot.status.clone())
            .map_err(|_| RuntimeError::Busy)
    }
    fn publish(&self, session: &McpSession, catalog: McpCatalog) -> Result<(), RuntimeError> {
        let mut snapshot = self.snapshot.lock().map_err(|_| RuntimeError::Busy)?;
        if self.cancel.is_cancelled() || self.documents.cancellation().is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        snapshot.status.phase = ConnectionPhase::Connected;
        snapshot.status.protocol_version = Some(session.protocol_version.clone());
        snapshot.status.catalog_revision = Some(catalog.revision().to_owned());
        snapshot.status.tools = catalog.tools().len();
        snapshot.catalog = Some(catalog);
        Ok(())
    }
}

#[derive(Default)]
pub struct ConnectionRegistry {
    entries: Mutex<HashMap<Uuid, Arc<Connection>>>,
}
impl Drop for ConnectionRegistry {
    fn drop(&mut self) {
        if let Ok(entries) = self.entries.lock() {
            for connection in entries.values() {
                connection.stop(None);
            }
        }
    }
}
impl ConnectionRegistry {
    pub(super) fn start(
        &self,
        id: Uuid,
        directory: std::path::PathBuf,
        definition: IntegrationDefinition,
        documents: Arc<DocumentService>,
        cancel: CancellationToken,
        prepared: PreparedProcess,
        expires: Instant,
    ) -> Result<ConnectionStatus, RuntimeError> {
        let mut entries = self.entries.lock().map_err(|_| RuntimeError::Busy)?;
        if entries.get(&definition.id).is_some_and(|entry| {
            entry
                .status()
                .map(|status| status.phase.active())
                .unwrap_or(true)
        }) {
            return Err(RuntimeError::Busy);
        }
        if entries
            .values()
            .filter(|entry| {
                entry
                    .status()
                    .map(|status| status.phase.active())
                    .unwrap_or(true)
            })
            .count()
            >= MAX_CONNECTIONS
        {
            return Err(RuntimeError::Busy);
        }
        if entries.len() >= 128 {
            entries.retain(|_, entry| {
                entry
                    .status()
                    .map(|status| status.phase.active())
                    .unwrap_or(true)
            });
        }
        if cancel.is_cancelled() || documents.cancellation().is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        let status = ConnectionStatus {
            connection_id: id,
            integration_id: definition.id,
            workspace: documents.identity().clone(),
            phase: ConnectionPhase::Connecting,
            protocol_version: None,
            catalog_revision: None,
            tools: 0,
            error: None,
        };
        let (refresh, receiver) = mpsc::channel(1);
        let connection = Arc::new(Connection {
            definition,
            documents,
            cancel,
            refresh,
            worker: Mutex::new(None),
            snapshot: Mutex::new(Snapshot {
                status: status.clone(),
                catalog: None,
            }),
        });
        entries.insert(connection.definition.id, connection.clone());
        let worker = tokio::spawn(run(
            connection.clone(),
            directory,
            prepared,
            receiver,
            expires,
        ));
        *connection
            .worker
            .lock()
            .expect("new connection worker slot") = Some(worker.abort_handle());
        Ok(status)
    }

    pub fn stop_all(&self) {
        if let Ok(entries) = self.entries.lock() {
            for entry in entries.values() {
                entry.stop(None);
            }
        }
    }
    pub fn is_quiet(&self) -> bool {
        self.entries
            .lock()
            .map(|entries| {
                entries
                    .values()
                    .all(|entry| entry.status().is_ok_and(|status| !status.phase.active()))
            })
            .unwrap_or(false)
    }
    pub fn abort_remaining(&self) {
        if let Ok(entries) = self.entries.lock() {
            for entry in entries.values() {
                if entry.status().is_ok_and(|status| status.phase.active()) {
                    entry.stop(None);
                    if let Ok(worker) = entry.worker.lock() {
                        if let Some(worker) = worker.as_ref() {
                            worker.abort();
                        }
                    }
                }
            }
        }
    }

    pub fn statuses(
        &self,
        documents: &DocumentService,
    ) -> Result<Vec<ConnectionStatus>, RuntimeError> {
        if documents.cancellation().is_cancelled() {
            return Err(RuntimeError::WorkspaceChanged);
        }
        let entries = self.entries.lock().map_err(|_| RuntimeError::Busy)?;
        let mut statuses = entries
            .values()
            .filter(|entry| entry.documents.identity() == documents.identity())
            .map(|entry| entry.status())
            .collect::<Result<Vec<_>, _>>()?;
        statuses.sort_by_key(|status| status.integration_id);
        Ok(statuses)
    }

    pub fn status(
        &self,
        integration: Uuid,
        documents: &DocumentService,
    ) -> Result<Option<ConnectionStatus>, RuntimeError> {
        let entries = self.entries.lock().map_err(|_| RuntimeError::Busy)?;
        entries
            .get(&integration)
            .filter(|entry| entry.documents.identity() == documents.identity())
            .map(|entry| entry.status())
            .transpose()
    }

    fn owned(
        &self,
        id: Uuid,
        documents: &DocumentService,
    ) -> Result<Arc<Connection>, RuntimeError> {
        if documents.cancellation().is_cancelled() {
            return Err(RuntimeError::WorkspaceChanged);
        }
        let entries = self.entries.lock().map_err(|_| RuntimeError::Busy)?;
        entries
            .values()
            .find(|entry| {
                entry
                    .status()
                    .is_ok_and(|status| status.connection_id == id)
                    && entry.documents.identity() == documents.identity()
            })
            .cloned()
            .ok_or(RuntimeError::ConnectionNotReady)
    }
    pub fn disconnect(&self, id: Uuid, documents: &DocumentService) -> Result<(), RuntimeError> {
        self.owned(id, documents)?.stop(None);
        Ok(())
    }
    pub fn cancel_request(&self, id: Uuid) {
        if let Ok(entries) = self.entries.lock() {
            for entry in entries.values() {
                if entry
                    .status()
                    .is_ok_and(|status| status.connection_id == id)
                {
                    entry.stop(None);
                }
            }
        }
    }
    pub fn invalidate_secret(&self, integration: Uuid) {
        if let Ok(entries) = self.entries.lock() {
            if let Some(entry) = entries.get(&integration) {
                entry.stop(Some(RuntimeError::ConfigChanged));
            }
        }
    }
    /// Run on a blocking worker after durable settings writes. The actor also
    /// checks independently, so external edits don't depend on an open Settings UI.
    pub fn reconcile(&self, directory: &Path) {
        let entries: Vec<_> = match self.entries.lock() {
            Ok(entries) => entries.values().cloned().collect(),
            Err(_) => return,
        };
        for entry in entries {
            if let Err(error) = check_policy(directory, &entry.documents, &entry.definition) {
                entry.stop(Some(error));
            }
        }
    }
    pub fn catalog(
        &self,
        id: Uuid,
        documents: &DocumentService,
    ) -> Result<McpCatalog, RuntimeError> {
        let entry = self.owned(id, documents)?;
        let snapshot = entry.snapshot.lock().map_err(|_| RuntimeError::Busy)?;
        if snapshot.status.phase != ConnectionPhase::Connected || entry.cancel.is_cancelled() {
            return Err(RuntimeError::ConnectionNotReady);
        }
        snapshot
            .catalog
            .clone()
            .ok_or(RuntimeError::ConnectionNotReady)
    }
    pub fn refresh(&self, id: Uuid, documents: &DocumentService) -> Result<(), RuntimeError> {
        let entry = self.owned(id, documents)?;
        {
            let mut snapshot = entry.snapshot.lock().map_err(|_| RuntimeError::Busy)?;
            if snapshot.status.phase != ConnectionPhase::Connected || entry.cancel.is_cancelled() {
                return Err(RuntimeError::ConnectionNotReady);
            }
            snapshot.status.phase = ConnectionPhase::Refreshing;
            snapshot.status.catalog_revision = None;
            snapshot.catalog = None;
        }
        if entry.refresh.try_send(()).is_err() {
            entry.stop(Some(RuntimeError::ConnectionNotReady));
            return Err(RuntimeError::ConnectionNotReady);
        }
        Ok(())
    }
}

pub(super) fn read_policy(
    directory: &Path,
    documents: &DocumentService,
    integration: Uuid,
) -> Result<IntegrationDefinition, RuntimeError> {
    if documents.cancellation().is_cancelled() {
        return Err(RuntimeError::WorkspaceChanged);
    }
    let snapshot = IntegrationStore::new(directory.into())
        .load()
        .map_err(|_| RuntimeError::PolicyUnavailable)?;
    let entry = snapshot
        .config
        .entries
        .into_iter()
        .find(|entry| entry.id == integration)
        .ok_or(RuntimeError::ConfigChanged)?;
    let path = crate::project_settings::get_settings_path(documents.workspace_root());
    let settings = match std::fs::File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_CONFIG_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| RuntimeError::PolicyUnavailable)?;
            if bytes.len() > MAX_CONFIG_BYTES {
                return Err(RuntimeError::PolicyUnavailable);
            }
            serde_json::from_slice::<crate::project_settings::ProjectSettings>(&bytes)
                .map_err(|_| RuntimeError::PolicyUnavailable)?
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(_) => return Err(RuntimeError::PolicyUnavailable),
    };
    if !settings.integrations.allows(&entry) {
        return Err(RuntimeError::IntegrationDisabled);
    }
    if !matches!(
        entry.connection,
        ConnectionConfig::Mcp {
            transport: McpTransport::Stdio { .. }
        }
    ) {
        return Err(RuntimeError::UnsupportedTransport);
    }
    Ok(entry)
}
fn check_policy(
    directory: &Path,
    documents: &DocumentService,
    expected: &IntegrationDefinition,
) -> Result<(), RuntimeError> {
    if read_policy(directory, documents, expected.id)? != *expected {
        return Err(RuntimeError::ConfigChanged);
    }
    Ok(())
}

async fn verify(directory: &Path, connection: &Connection) -> Result<(), RuntimeError> {
    let directory = directory.to_path_buf();
    let documents = connection.documents.clone();
    let definition = connection.definition.clone();
    tokio::task::spawn_blocking(move || check_policy(&directory, &documents, &definition))
        .await
        .map_err(|_| RuntimeError::PolicyUnavailable)?
}

struct FinishGuard(Arc<Connection>);
impl Drop for FinishGuard {
    fn drop(&mut self) {
        if self.0.status().is_ok_and(|status| status.phase.active()) {
            self.0.stop(Some(RuntimeError::ProtocolFailed));
            if let Ok(mut snapshot) = self.0.snapshot.lock() {
                snapshot.status.phase = ConnectionPhase::Failed;
            }
        }
    }
}

async fn run(
    connection: Arc<Connection>,
    directory: std::path::PathBuf,
    prepared: PreparedProcess,
    mut refresh: mpsc::Receiver<()>,
    expires: Instant,
) {
    let _finished = FinishGuard(connection.clone());
    let workspace_cancel = connection.documents.cancellation();
    let mut process = None;
    let mut session = None;
    let work = async {
        verify(&directory, &connection).await?;
        if connection.cancel.is_cancelled() || workspace_cancel.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        if expires <= Instant::now() {
            return Err(RuntimeError::ApprovalExpired);
        }
        let (child, read, write) = SupervisedProcess::spawn(&prepared)?;
        process = Some(child);
        let opened = McpSession::open(read, write).await?;
        session = Some(opened);
        let active = session.as_ref().ok_or(RuntimeError::ProtocolFailed)?;
        let catalog = active.discover(connection.definition.id).await?;
        verify(&directory, &connection).await?;
        connection.publish(active, catalog)
    };
    let started = tokio::select! {
        biased;
        _ = workspace_cancel.cancelled() => Err(RuntimeError::WorkspaceChanged),
        _ = connection.cancel.cancelled() => Err(RuntimeError::Cancelled),
        result = tokio::time::timeout(DISCOVERY_TIMEOUT, work) => result.unwrap_or(Err(RuntimeError::TimedOut)),
    };
    let result = match started {
        Err(error) => Err(error),
        Ok(()) => {
            let active = session
                .as_ref()
                .expect("successful initialization owns the MCP session");
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let monitor = async {
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if active.is_closed() { return Err(active.close_error()); }
                            tokio::time::timeout(DISCOVERY_TIMEOUT, verify(&directory, &connection)).await.map_err(|_| RuntimeError::TimedOut)??;
                        }
                        request = refresh.recv() => {
                            if request.is_none() { return Ok(()); }
                            let discover = async {
                                verify(&directory, &connection).await?;
                                let catalog = active.discover(connection.definition.id).await?;
                                verify(&directory, &connection).await?;
                                connection.publish(active, catalog)
                            };
                            tokio::time::timeout(DISCOVERY_TIMEOUT, discover).await.map_err(|_| RuntimeError::TimedOut)??;
                        }
                    }
                }
            };
            tokio::select! {
                biased;
                _ = workspace_cancel.cancelled() => Err(RuntimeError::WorkspaceChanged),
                _ = connection.cancel.cancelled() => Ok(()),
                result = monitor => result,
            }
        }
    };
    // Clear discovery before closing transport; never present stale tools while stopping.
    let error = result
        .err()
        .filter(|error| *error != RuntimeError::Cancelled)
        .or_else(|| connection.status().ok().and_then(|status| status.error));
    connection.stop(error);
    if let Some(session) = session {
        session.shutdown().await;
    }
    if let Some(process) = process {
        process.shutdown_gracefully().await;
    }
    if let Ok(mut snapshot) = connection.snapshot.lock() {
        snapshot.status.phase = if error.is_some() {
            ConnectionPhase::Failed
        } else {
            ConnectionPhase::Disconnected
        };
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::integrations::{
        config::{IntegrationConfig, ProcessConfig},
        credentials::SecretStore,
        runtime::IntegrationRuntime,
    };
    struct NoSecrets;
    impl SecretStore for NoSecrets {
        fn get(&self, _: Uuid, _: &str) -> Result<Option<String>, RuntimeError> {
            Ok(None)
        }
    }
    struct Fixture {
        root: tempfile::TempDir,
        runtime: Arc<IntegrationRuntime>,
        documents: Arc<DocumentService>,
        revision: String,
        id: Uuid,
    }
    impl Fixture {
        fn new(mode: &str) -> Self {
            let root = tempfile::tempdir().unwrap();
            let script = root.path().join("peer.py");
            std::fs::write(&script, r#"
import sys,json,time,os
from pathlib import Path
Path('peer.pid').write_text(str(os.getpid()))
mode=sys.argv[1]
pages=0
for line in sys.stdin:
    req=json.loads(line)
    if mode=='hang':
        time.sleep(60)
        continue
    method=req['method']
    if method=='notifications/initialized': continue
    if method=='server/discover':
        print(json.dumps({'jsonrpc':'2.0','id':req['id'],'error':{'code':-32601,'message':'legacy'}}),flush=True)
        continue
    if method=='initialize':
        result={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}}
    elif method=='tools/list':
        pages+=1
        if mode=='slow_refresh' and pages>1: time.sleep(60)
        result={'tools':[{'name':'search','description':str(pages),'inputSchema':{'type':'object'}}]}
    else:
        raise RuntimeError('Only discovery is authorized')
    print(json.dumps({'jsonrpc':'2.0','id':req['id'],'result':result}),flush=True)
    if mode=='exit' and pages: break
"#).unwrap();
            let id = Uuid::new_v4();
            let config = IntegrationConfig {
                entries: vec![IntegrationDefinition {
                    id,
                    name: "Fixture".into(),
                    enabled: true,
                    connection: ConnectionConfig::Mcp {
                        transport: McpTransport::Stdio {
                            process: ProcessConfig {
                                command: "python3".into(),
                                args: vec![
                                    "-u".into(),
                                    script.to_string_lossy().into(),
                                    mode.into(),
                                ],
                                cwd: None,
                                env: Default::default(),
                            },
                        },
                    },
                }],
                ..Default::default()
            };
            let revision = IntegrationStore::new(root.path().into())
                .save("missing", config)
                .unwrap()
                .revision;
            Self {
                runtime: Arc::new(IntegrationRuntime::new(
                    root.path().into(),
                    Arc::new(NoSecrets),
                )),
                documents: Arc::new(DocumentService::new(root.path().into())),
                root,
                revision,
                id,
            }
        }
        async fn connect(&self) -> ConnectionStatus {
            let review = self
                .runtime
                .prepare_connection(self.id, self.revision.clone(), &self.documents)
                .unwrap();
            self.runtime
                .connect(review.ticket_id, self.documents.clone())
                .await
                .unwrap()
        }
        async fn phase(&self, phase: ConnectionPhase) -> ConnectionStatus {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let status = self
                        .runtime
                        .connections
                        .status(self.id, &self.documents)
                        .unwrap()
                        .unwrap();
                    if status.phase == phase {
                        return status;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("connection reached expected phase")
        }
        fn configure(&mut self, edit: impl FnOnce(&mut IntegrationConfig)) {
            let store = IntegrationStore::new(self.root.path().into());
            let mut snapshot = store.load().unwrap();
            edit(&mut snapshot.config);
            self.revision = store
                .save(&snapshot.revision, snapshot.config)
                .unwrap()
                .revision;
        }
    }

    #[tokio::test]
    async fn approvals_cannot_cross_purposes_and_live_start_is_single_use() {
        let fixture = Fixture::new("normal");
        let probe = fixture
            .runtime
            .prepare(fixture.id, fixture.revision.clone(), &fixture.documents)
            .unwrap();
        assert!(matches!(
            fixture
                .runtime
                .connect(probe.ticket_id, fixture.documents.clone())
                .await,
            Err(RuntimeError::ApprovalExpired)
        ));
        fixture.runtime.cancel(probe.ticket_id);
        let live = fixture
            .runtime
            .prepare_connection(fixture.id, fixture.revision.clone(), &fixture.documents)
            .unwrap();
        assert!(matches!(
            fixture
                .runtime
                .run(live.ticket_id, fixture.documents.clone())
                .await,
            Err(RuntimeError::ApprovalExpired)
        ));
        fixture
            .runtime
            .connect(live.ticket_id, fixture.documents.clone())
            .await
            .unwrap();
        assert!(matches!(
            fixture
                .runtime
                .connect(live.ticket_id, fixture.documents.clone())
                .await,
            Err(RuntimeError::ApprovalExpired)
        ));
        fixture.phase(ConnectionPhase::Connected).await;
        let duplicate = fixture
            .runtime
            .prepare_connection(fixture.id, fixture.revision.clone(), &fixture.documents)
            .unwrap();
        assert!(matches!(
            fixture
                .runtime
                .connect(duplicate.ticket_id, fixture.documents.clone())
                .await,
            Err(RuntimeError::Busy)
        ));
        fixture
            .runtime
            .connections
            .disconnect(live.ticket_id, &fixture.documents)
            .unwrap();
        fixture.phase(ConnectionPhase::Disconnected).await;
    }

    #[tokio::test]
    async fn refresh_keeps_the_process_and_stale_disconnect_cannot_stop_a_replacement() {
        let fixture = Fixture::new("normal");
        let initial = fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let pid = std::fs::read_to_string(fixture.root.path().join("peer.pid")).unwrap();
        let before = fixture
            .runtime
            .connections
            .catalog(initial.connection_id, &fixture.documents)
            .unwrap();
        fixture
            .runtime
            .connections
            .refresh(initial.connection_id, &fixture.documents)
            .unwrap();
        assert!(fixture
            .runtime
            .connections
            .catalog(initial.connection_id, &fixture.documents)
            .is_err());
        fixture.phase(ConnectionPhase::Connected).await;
        let after = fixture
            .runtime
            .connections
            .catalog(initial.connection_id, &fixture.documents)
            .unwrap();
        assert_ne!(before.revision(), after.revision());
        assert_eq!(before.tools()[0].alias, after.tools()[0].alias);
        assert_eq!(
            pid,
            std::fs::read_to_string(fixture.root.path().join("peer.pid")).unwrap()
        );
        fixture
            .runtime
            .connections
            .disconnect(initial.connection_id, &fixture.documents)
            .unwrap();
        fixture.phase(ConnectionPhase::Disconnected).await;
        let next = fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        fixture.runtime.cancel(initial.connection_id);
        assert!(fixture
            .runtime
            .connections
            .disconnect(initial.connection_id, &fixture.documents)
            .is_err());
        assert_ne!(initial.connection_id, next.connection_id);
        assert!(fixture
            .runtime
            .connections
            .catalog(next.connection_id, &fixture.documents)
            .is_ok());
        fixture
            .runtime
            .connections
            .disconnect(next.connection_id, &fixture.documents)
            .unwrap();
        fixture.phase(ConnectionPhase::Disconnected).await;
    }

    #[tokio::test]
    async fn policy_fails_closed_and_unrelated_index_preference_does_not_disconnect() {
        let mut fixture = Fixture::new("normal");
        fixture.configure(|config| config.entries[0].enabled = false);
        assert!(matches!(
            fixture.runtime.prepare_connection(
                fixture.id,
                fixture.revision.clone(),
                &fixture.documents
            ),
            Err(RuntimeError::IntegrationDisabled)
        ));
        assert!(!fixture.root.path().join("peer.pid").exists());
        fixture.configure(|config| config.entries[0].enabled = true);
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        fixture.configure(|config| config.symbols_index_enabled = false);
        fixture.runtime.reconcile_connections();
        assert_eq!(
            fixture
                .runtime
                .connections
                .status(fixture.id, &fixture.documents)
                .unwrap()
                .unwrap()
                .phase,
            ConnectionPhase::Connected
        );
        let mut settings = crate::project_settings::ProjectSettings::default();
        settings.integrations.disabled_ids.push(fixture.id);
        crate::project_settings::save_project_settings(fixture.root.path(), &settings).unwrap();
        fixture.runtime.reconcile_connections();
        assert_eq!(
            fixture.phase(ConnectionPhase::Failed).await.error,
            Some(RuntimeError::IntegrationDisabled)
        );
        std::fs::write(
            crate::project_settings::get_settings_path(fixture.root.path()),
            "invalid",
        )
        .unwrap();
        assert!(matches!(
            fixture.runtime.prepare_connection(
                fixture.id,
                fixture.revision.clone(),
                &fixture.documents
            ),
            Err(RuntimeError::PolicyUnavailable)
        ));
    }

    #[tokio::test]
    async fn external_config_edits_are_detected_without_settings_polling() {
        let mut fixture = Fixture::new("normal");
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        fixture.configure(|config| config.entries[0].name = "Changed".into());
        assert_eq!(
            fixture.phase(ConnectionPhase::Failed).await.error,
            Some(RuntimeError::ConfigChanged)
        );
    }

    #[tokio::test]
    async fn credential_revocation_and_workspace_retirement_clear_catalogs() {
        let fixture = Fixture::new("normal");
        let first = fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        fixture.runtime.credentials_changed(fixture.id);
        fixture.phase(ConnectionPhase::Failed).await;
        assert!(fixture
            .runtime
            .connections
            .catalog(first.connection_id, &fixture.documents)
            .is_err());
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let reopened = DocumentService::new(fixture.root.path().into());
        assert!(fixture
            .runtime
            .connections
            .statuses(&reopened)
            .unwrap()
            .is_empty());
        fixture.documents.retire();
        assert_eq!(
            fixture.phase(ConnectionPhase::Failed).await.error,
            Some(RuntimeError::WorkspaceChanged)
        );
    }

    #[tokio::test]
    async fn cancellation_stops_initialization_and_a_stalled_refresh() {
        for mode in ["hang", "slow_refresh"] {
            let fixture = Fixture::new(mode);
            let status = fixture.connect().await;
            if mode == "slow_refresh" {
                fixture.phase(ConnectionPhase::Connected).await;
                fixture
                    .runtime
                    .connections
                    .refresh(status.connection_id, &fixture.documents)
                    .unwrap();
            }
            fixture.runtime.cancel(status.connection_id);
            fixture.phase(ConnectionPhase::Disconnected).await;
            assert!(fixture
                .runtime
                .connections
                .catalog(status.connection_id, &fixture.documents)
                .is_err());
        }
    }

    #[tokio::test]
    async fn unexpected_peer_exit_never_leaves_a_live_catalog() {
        let fixture = Fixture::new("exit");
        let status = fixture.connect().await;
        assert_eq!(
            fixture.phase(ConnectionPhase::Failed).await.error,
            Some(RuntimeError::ProtocolFailed)
        );
        assert!(fixture
            .runtime
            .connections
            .catalog(status.connection_id, &fixture.documents)
            .is_err());
    }

    #[tokio::test]
    async fn startup_deadline_closes_a_peer_that_never_answers() {
        let fixture = Fixture::new("hang");
        fixture.connect().await;
        let status = tokio::time::timeout(Duration::from_secs(25), async {
            loop {
                let status = fixture
                    .runtime
                    .connections
                    .status(fixture.id, &fixture.documents)
                    .unwrap()
                    .unwrap();
                if status.phase == ConnectionPhase::Failed {
                    return status;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(status.error, Some(RuntimeError::TimedOut));
    }
    #[tokio::test]
    async fn credential_changes_revoke_unconsumed_connection_approvals() {
        let fixture = Fixture::new("normal");
        let review = fixture
            .runtime
            .prepare_connection(fixture.id, fixture.revision.clone(), &fixture.documents)
            .unwrap();
        fixture.runtime.credentials_changed(fixture.id);
        assert!(matches!(
            fixture
                .runtime
                .connect(review.ticket_id, fixture.documents.clone())
                .await,
            Err(RuntimeError::ApprovalExpired)
        ));
        assert!(!fixture.root.path().join("peer.pid").exists());
    }

    #[tokio::test]
    #[ignore = "requires ZBLADE_ATLAS_SCOUT_BINARY pointing to an installed Atlas Scout"]
    async fn atlas_scout_live_workspace_smoke() {
        let executable = std::env::var("ZBLADE_ATLAS_SCOUT_BINARY")
            .expect("explicit Atlas Scout executable required");
        let mut fixture = Fixture::new("normal");
        let root = fixture.root.path().to_string_lossy().to_string();
        let cache = fixture
            .root
            .path()
            .join("scout-cache")
            .to_string_lossy()
            .to_string();
        fixture.configure(|config| {
            config.entries[0].connection = ConnectionConfig::Mcp {
                transport: McpTransport::Stdio {
                    process: ProcessConfig {
                        command: executable,
                        args: vec![
                            "mcp".into(),
                            "--workspace".into(),
                            root,
                            "--cache-dir".into(),
                            cache,
                        ],
                        cwd: None,
                        env: Default::default(),
                    },
                },
            };
        });
        let mut settings = crate::project_settings::ProjectSettings::default();
        settings.integrations.symbols_index_enabled = Some(false);
        crate::project_settings::save_project_settings(fixture.root.path(), &settings).unwrap();
        let status = fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let before = fixture
            .runtime
            .connections
            .catalog(status.connection_id, &fixture.documents)
            .unwrap();
        assert!(before
            .tools()
            .iter()
            .any(|tool| tool.definition.name == "symbol_search"));
        fixture
            .runtime
            .connections
            .refresh(status.connection_id, &fixture.documents)
            .unwrap();
        fixture.phase(ConnectionPhase::Connected).await;
        let after = fixture
            .runtime
            .connections
            .catalog(status.connection_id, &fixture.documents)
            .unwrap();
        assert_eq!(before.revision(), after.revision());
        fixture
            .runtime
            .connections
            .disconnect(status.connection_id, &fixture.documents)
            .unwrap();
        fixture.phase(ConnectionPhase::Disconnected).await;
        assert!(!fixture
            .root
            .path()
            .join(".zblade/index/symbols.db")
            .exists());
    }
    #[tokio::test]
    async fn application_shutdown_drains_connections_and_revokes_future_launches() {
        let fixture = Fixture::new("normal");
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        fixture.runtime.shutdown().await;
        assert!(fixture.runtime.connections.is_quiet());
        assert!(matches!(
            fixture.runtime.prepare_connection(
                fixture.id,
                fixture.revision.clone(),
                &fixture.documents
            ),
            Err(RuntimeError::Cancelled)
        ));
        assert!(matches!(
            fixture
                .runtime
                .prepare(fixture.id, fixture.revision.clone(), &fixture.documents),
            Err(RuntimeError::Cancelled)
        ));
    }
}
