//! Workspace-owned MCP process lifetimes and serialized, approved tool calls.
use super::{
    call_result::{CallFailure, CallOutcome, McpCallResult},
    catalog::McpCatalog,
    config::{ConnectionConfig, IntegrationDefinition, McpTransport, MAX_CONFIG_BYTES},
    identity::WorkspaceIdentity,
    mcp_session::McpSession,
    permissions::{ApprovedCall, CallTarget},
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
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};
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
    commands: mpsc::Sender<Command>,
    operation: Arc<Semaphore>,
    fingerprint: String,
    snapshot: Mutex<Snapshot>,
    worker: Mutex<Option<tokio::task::AbortHandle>>,
}

enum Command {
    Refresh(OwnedSemaphorePermit),
    Call(CallRequest),
}
struct CallRequest {
    approved: ApprovedCall,
    cancel: CancellationToken,
    sent: Arc<AtomicBool>,
    response: oneshot::Sender<Result<McpCallResult, CallFailure>>,
    _slot: OwnedSemaphorePermit,
}
pub(super) struct CallBinding {
    pub definition: IntegrationDefinition,
    pub fingerprint: String,
    pub tool_name: String,
    pub cancel: CancellationToken,
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
        let (commands, receiver) = mpsc::channel(1);
        let connection = Arc::new(Connection {
            definition,
            documents,
            cancel,
            commands,
            operation: Arc::new(Semaphore::new(1)),
            fingerprint: prepared.fingerprint.clone(),
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
        let slot = entry
            .operation
            .clone()
            .try_acquire_owned()
            .map_err(|_| RuntimeError::Busy)?;
        {
            let mut snapshot = entry.snapshot.lock().map_err(|_| RuntimeError::Busy)?;
            if snapshot.status.phase != ConnectionPhase::Connected || entry.cancel.is_cancelled() {
                return Err(RuntimeError::ConnectionNotReady);
            }
            snapshot.status.phase = ConnectionPhase::Refreshing;
            snapshot.status.catalog_revision = None;
            snapshot.catalog = None;
        }
        if entry.commands.try_send(Command::Refresh(slot)).is_err() {
            entry.stop(Some(RuntimeError::ConnectionNotReady));
            return Err(RuntimeError::ConnectionNotReady);
        }
        Ok(())
    }

    pub(super) fn call_binding(
        &self,
        target: &CallTarget,
        documents: &DocumentService,
    ) -> Result<CallBinding, RuntimeError> {
        let entry = self.owned(target.connection_id, documents)?;
        let tool_name = current_tool(&entry, target)?;
        Ok(CallBinding {
            definition: entry.definition.clone(),
            fingerprint: entry.fingerprint.clone(),
            tool_name,
            cancel: entry.cancel.clone(),
        })
    }

    pub(super) async fn call(
        &self,
        approved: ApprovedCall,
        documents: &DocumentService,
    ) -> Result<McpCallResult, CallFailure> {
        approved.check()?;
        let entry = self.owned(approved.review().target.connection_id, documents)?;
        let slot = entry
            .operation
            .clone()
            .try_acquire_owned()
            .map_err(|_| RuntimeError::Busy)?;
        current_tool(&entry, &approved.review().target)?;
        let cancel = approved.context().cancel.child_token();
        // Aborting/dropping the caller cancels actor-owned work too.
        let _guard = cancel.clone().drop_guard();
        let sent = Arc::new(AtomicBool::new(false));
        let (response, result) = oneshot::channel();
        entry
            .commands
            .try_send(Command::Call(CallRequest {
                approved,
                cancel,
                sent: sent.clone(),
                response,
                _slot: slot,
            }))
            .map_err(|_| RuntimeError::ConnectionNotReady)?;
        result.await.unwrap_or_else(|_| {
            Err(CallFailure {
                code: RuntimeError::ConnectionNotReady,
                outcome: if sent.load(Ordering::Acquire) {
                    CallOutcome::Unknown
                } else {
                    CallOutcome::NotStarted
                },
            })
        })
    }
}

fn current_tool(entry: &Connection, target: &CallTarget) -> Result<String, RuntimeError> {
    if entry.cancel.is_cancelled() {
        return Err(RuntimeError::ConnectionNotReady);
    }
    let snapshot = entry.snapshot.lock().map_err(|_| RuntimeError::Busy)?;
    if snapshot.status.phase != ConnectionPhase::Connected {
        return Err(RuntimeError::ConnectionNotReady);
    }
    let catalog = snapshot
        .catalog
        .as_ref()
        .ok_or(RuntimeError::ConnectionNotReady)?;
    Ok(catalog
        .resolve(&target.alias, &target.catalog_revision)?
        .definition
        .name
        .to_string())
}

async fn run_call(
    connection: &Connection,
    directory: &Path,
    session: &McpSession,
    request: CallRequest,
) -> Result<(), RuntimeError> {
    let validation = async {
        verify(directory, connection).await?;
        request.approved.check()?;
        let review = request.approved.review();
        let name = current_tool(connection, &review.target)?;
        if name != review.tool_name {
            return Err(RuntimeError::CatalogChanged);
        }
        // Arguments were validated before permission review, and cannot change.
        let arguments = review
            .arguments
            .as_object()
            .ok_or(RuntimeError::InvalidArguments)?
            .clone();
        Ok((name, arguments))
    };
    let result = tokio::select! {
        biased;
        _ = request.cancel.cancelled() => Err(RuntimeError::Cancelled),
        result = tokio::time::timeout(DISCOVERY_TIMEOUT, validation) => result.unwrap_or(Err(RuntimeError::TimedOut)),
    };
    let result = match result {
        Ok((name, arguments)) => {
            let policy = async {
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    match tokio::time::timeout(DISCOVERY_TIMEOUT, verify(directory, connection))
                        .await
                    {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => return error,
                        Err(_) => return RuntimeError::TimedOut,
                    }
                }
            };
            tokio::select! {
                result = session.call(name, arguments, &request.cancel, &request.sent) =>
                    result.and_then(|result| McpCallResult::normalize(request.approved.review(), result)),
                error = policy => Err(error),
            }
        }
        Err(error) => Err(error),
    };
    let sent = request.sent.load(Ordering::Acquire);
    let stop = result.as_ref().err().copied().filter(|_| sent);
    // Deny further dispatch before exposing an uncertain outcome to the caller.
    if let Some(error) = stop {
        connection.stop(Some(error));
    }
    let _ = request.response.send(result.map_err(|code| CallFailure {
        code,
        outcome: if sent {
            CallOutcome::Unknown
        } else {
            CallOutcome::NotStarted
        },
    }));
    stop.map_or(Ok(()), Err)
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
    mut commands: mpsc::Receiver<Command>,
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
                        request = commands.recv() => {
                            let Some(request) = request else { return Ok(()); };
                            let _slot = match request {
                                Command::Call(request) => {
                                    run_call(&connection, &directory, active, request).await?;
                                    continue;
                                }
                                Command::Refresh(slot) => slot,
                            };
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
        call_result::CallOutcome,
        permissions::{CallContext, CallReview},
    };
    use crate::integrations::{
        config::{IntegrationConfig, ProcessConfig},
        credentials::SecretStore,
        runtime::IntegrationRuntime,
    };
    use serde_json::json;
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
    if method=='notifications/cancelled':
        Path('cancel.json').write_text(json.dumps(req))
        continue
    if method=='server/discover' and mode=='modern':
        result={'resultType':'complete','supportedVersions':['2026-07-28'],'ttlMs':0,'cacheScope':'private','capabilities':{'tools':{}}}
        print(json.dumps({'jsonrpc':'2.0','id':req['id'],'result':result}),flush=True)
        continue
    if method=='server/discover':
        print(json.dumps({'jsonrpc':'2.0','id':req['id'],'error':{'code':-32601,'message':'legacy'}}),flush=True)
        continue
    if method=='initialize':
        result={'protocolVersion':'2025-11-25','capabilities':{'tools':{}},'serverInfo':{'name':'fixture','version':'1'}}
    elif method=='tools/list':
        pages+=1
        if mode=='slow_refresh' and pages>1: time.sleep(60)
        result={'tools':[{'name':'search','description':str(pages),'inputSchema':{'type':'object'},'annotations':{'readOnlyHint':True}}]}
    elif method=='tools/call':
        with open('calls.jsonl','a') as output: output.write(json.dumps(req)+'\n')
        if mode=='slow_call': continue
        if mode=='exit_call': os._exit(0)
        if mode=='modern': assert req['params']['_meta']['io.modelcontextprotocol/protocolVersion']=='2026-07-28'
        result={'resultType':'complete','structuredContent':{'name':req['params']['name'],'arguments':req['params']['arguments']},'isError':mode=='tool_error'}
        if mode=='large_call': result={'content':[{'type':'text','text':'x'*(1024*1024+1)}]}
        if mode=='input_call': result={'resultType':'input_required','inputRequests':{}}
    else:
        raise RuntimeError('Unexpected request')
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

    impl Fixture {
        fn call_context(&self) -> CallContext {
            CallContext::new(
                &self.documents,
                Uuid::new_v4(),
                Uuid::new_v4(),
                "call-1".into(),
                CancellationToken::new(),
            )
            .unwrap()
        }
        fn review(&self, context: &CallContext) -> CallReview {
            let status = self
                .runtime
                .connections
                .status(self.id, &self.documents)
                .unwrap()
                .unwrap();
            let catalog = self
                .runtime
                .connections
                .catalog(status.connection_id, &self.documents)
                .unwrap();
            self.runtime
                .prepare_tool_call(
                    &self.documents,
                    context,
                    CallTarget {
                        connection_id: status.connection_id,
                        alias: catalog.tools()[0].alias.clone(),
                        catalog_revision: catalog.revision().into(),
                    },
                    json!({"query":"approved"}),
                )
                .unwrap()
        }
        async fn called(&self) {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !self.root.path().join("calls.jsonl").exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
        }
        fn call_count(&self) -> usize {
            std::fs::read_to_string(self.root.path().join("calls.jsonl"))
                .unwrap_or_default()
                .lines()
                .count()
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
        let tool = before
            .tools()
            .iter()
            .find(|tool| tool.definition.name == "symbol_search")
            .unwrap();
        let context = fixture.call_context();
        let review = fixture
            .runtime
            .prepare_tool_call(
                &fixture.documents,
                &context,
                CallTarget {
                    connection_id: status.connection_id,
                    alias: tool.alias.clone(),
                    catalog_revision: before.revision().into(),
                },
                serde_json::json!({"query":"mode"}),
            )
            .unwrap();
        fixture
            .runtime
            .decide_tool_call(&context, review.request_id, true)
            .unwrap();
        let result = fixture
            .runtime
            .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
            .await
            .unwrap();
        assert!(!result.is_error);
        assert!(result.result.get("structuredContent").is_some());
        assert!(!result.text.is_empty());
        let turn = super::super::native_turn::NativeMcpTurn::new(
            fixture.runtime.clone(),
            fixture.documents.clone(),
            Uuid::new_v4(),
            false,
        )
        .unwrap();
        assert!(turn
            .schemas()
            .iter()
            .any(|schema| schema["function"]["name"] == tool.alias));
        let call: crate::protocol::ToolCall = serde_json::from_value(json!({"id":"native-atlas","type":"function","function":{"name":tool.alias,"arguments":"{\"query\":\"mode\"}"}})).unwrap();
        let task = tokio::spawn({
            let turn = turn.clone();
            async move { turn.execute(&call).await }
        });
        let request = native_pending(&turn).await;
        turn.respond(request, true).unwrap();
        let result = task.await.unwrap();
        assert!(result.success);
        let result: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert!(result["result"]["artifact"].is_object());
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
    #[tokio::test]
    async fn calls_require_exact_scope_and_one_use_consent_in_both_protocols() {
        for mode in ["normal", "modern", "tool_error"] {
            let fixture = Fixture::new(mode);
            fixture.connect().await;
            fixture.phase(ConnectionPhase::Connected).await;
            let context = fixture.call_context();
            let mut review = fixture.review(&context);
            let fail = fixture
                .runtime
                .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
                .await
                .unwrap_err();
            assert_eq!(fail.code, RuntimeError::PermissionRequired);
            assert_eq!(fail.outcome, CallOutcome::NotStarted);
            let wrong = fixture.call_context();
            assert_eq!(
                fixture
                    .runtime
                    .decide_tool_call(&wrong, review.request_id, true),
                Err(RuntimeError::PermissionDenied)
            );
            assert_eq!(fixture.call_count(), 0);
            fixture
                .runtime
                .decide_tool_call(&context, review.request_id, true)
                .unwrap();
            assert_eq!(
                fixture
                    .runtime
                    .decide_tool_call(&context, review.request_id, false),
                Err(RuntimeError::ApprovalExpired)
            );
            // UI copies cannot mutate the stored invocation.
            review.arguments = json!({"query":"tampered"});
            let result = fixture
                .runtime
                .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
                .await
                .unwrap();
            assert_eq!(
                result.result["structuredContent"]["arguments"]["query"],
                "approved"
            );
            assert_eq!(result.tool_name, "search");
            assert_eq!(result.scope, context.scope);
            assert_eq!(result.target.alias, review.target.alias);
            assert_eq!(result.is_error, mode == "tool_error");
            assert!(result.text.contains("approved"));
            assert_eq!(
                fixture
                    .runtime
                    .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
                    .await
                    .unwrap_err()
                    .code,
                RuntimeError::ApprovalExpired
            );
            assert_eq!(fixture.call_count(), 1);
            fixture.runtime.shutdown().await;
        }
    }

    #[tokio::test]
    async fn denial_turn_cancellation_and_stale_catalog_never_reach_the_peer() {
        let fixture = Fixture::new("normal");
        let connection = fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let context = fixture.call_context();
        let review = fixture.review(&context);
        fixture
            .runtime
            .decide_tool_call(&context, review.request_id, false)
            .unwrap();
        assert_eq!(
            fixture
                .runtime
                .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
                .await
                .unwrap_err()
                .code,
            RuntimeError::PermissionDenied
        );
        let context = fixture.call_context();
        let review = fixture.review(&context);
        fixture
            .runtime
            .decide_tool_call(&context, review.request_id, true)
            .unwrap();
        context.cancel.cancel();
        assert_eq!(
            fixture
                .runtime
                .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
                .await
                .unwrap_err()
                .code,
            RuntimeError::Cancelled
        );
        let context = fixture.call_context();
        let review = fixture.review(&context);
        fixture
            .runtime
            .decide_tool_call(&context, review.request_id, true)
            .unwrap();
        fixture
            .runtime
            .connections
            .refresh(connection.connection_id, &fixture.documents)
            .unwrap();
        fixture.phase(ConnectionPhase::Connected).await;
        assert_eq!(
            fixture
                .runtime
                .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
                .await
                .unwrap_err()
                .code,
            RuntimeError::CatalogChanged
        );
        assert_eq!(fixture.call_count(), 0);
        fixture.runtime.shutdown().await;
    }

    #[tokio::test]
    async fn dispatch_rechecks_policy_and_workspace_without_waiting_for_status_poll() {
        let fixture = Fixture::new("normal");
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let context = fixture.call_context();
        let review = fixture.review(&context);
        fixture
            .runtime
            .decide_tool_call(&context, review.request_id, true)
            .unwrap();
        let mut settings = crate::project_settings::ProjectSettings::default();
        settings.integrations.disabled_ids.push(fixture.id);
        crate::project_settings::save_project_settings(fixture.root.path(), &settings).unwrap();
        assert!(fixture
            .runtime
            .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
            .await
            .is_err());
        assert_eq!(fixture.call_count(), 0);
        fixture.documents.retire();
        assert_eq!(
            fixture
                .runtime
                .decide_tool_call(&context, review.request_id, true),
            Err(RuntimeError::WorkspaceChanged)
        );
        fixture.runtime.shutdown().await;
    }

    #[tokio::test]
    async fn cancelled_and_abandoned_calls_close_the_connection_without_replay() {
        for abandon in [false, true] {
            let fixture = Fixture::new("slow_call");
            let status = fixture.connect().await;
            fixture.phase(ConnectionPhase::Connected).await;
            let context = fixture.call_context();
            let review = fixture.review(&context);
            fixture
                .runtime
                .decide_tool_call(&context, review.request_id, true)
                .unwrap();
            let runtime = fixture.runtime.clone();
            let documents = fixture.documents.clone();
            let caller = context.clone();
            let task = tokio::spawn(async move {
                runtime
                    .execute_tool_call(documents, &caller, review.request_id)
                    .await
            });
            fixture.called().await;
            assert_eq!(
                fixture
                    .runtime
                    .connections
                    .refresh(status.connection_id, &fixture.documents),
                Err(RuntimeError::Busy)
            );
            let second = fixture.call_context();
            let second_review = fixture.review(&second);
            fixture
                .runtime
                .decide_tool_call(&second, second_review.request_id, true)
                .unwrap();
            let failed = fixture
                .runtime
                .execute_tool_call(fixture.documents.clone(), &second, second_review.request_id)
                .await
                .unwrap_err();
            assert_eq!(failed.code, RuntimeError::Busy);
            assert_eq!(failed.outcome, CallOutcome::NotStarted);
            if abandon {
                task.abort();
                let _ = task.await;
            } else {
                context.cancel.cancel();
                let failed = task.await.unwrap().unwrap_err();
                assert_eq!(failed.code, RuntimeError::Cancelled);
                assert_eq!(failed.outcome, CallOutcome::Unknown);
            }
            fixture.phase(ConnectionPhase::Failed).await;
            assert_eq!(fixture.call_count(), 1);
            assert!(fixture.root.path().join("cancel.json").exists());
            fixture.runtime.shutdown().await;
        }
    }

    #[tokio::test]
    async fn crash_oversize_and_unsupported_input_results_are_uncertain_and_never_retried() {
        for mode in ["exit_call", "large_call", "input_call"] {
            let fixture = Fixture::new(mode);
            fixture.connect().await;
            fixture.phase(ConnectionPhase::Connected).await;
            let context = fixture.call_context();
            let review = fixture.review(&context);
            fixture
                .runtime
                .decide_tool_call(&context, review.request_id, true)
                .unwrap();
            let failed = fixture
                .runtime
                .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
                .await
                .unwrap_err();
            assert_eq!(failed.outcome, CallOutcome::Unknown);
            assert_eq!(
                failed.code,
                match mode {
                    "large_call" => RuntimeError::OutputLimit,
                    "input_call" => RuntimeError::UnsupportedResult,
                    _ => RuntimeError::ProtocolFailed,
                }
            );
            fixture.phase(ConnectionPhase::Failed).await;
            assert_eq!(fixture.call_count(), 1);
            fixture.runtime.shutdown().await;
        }
    }

    #[tokio::test]
    async fn call_deadline_is_fixed_and_does_not_report_side_effects_as_stopped() {
        let fixture = Fixture::new("slow_call");
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let context = fixture.call_context();
        let review = fixture.review(&context);
        fixture
            .runtime
            .decide_tool_call(&context, review.request_id, true)
            .unwrap();
        let failed = tokio::time::timeout(
            Duration::from_secs(50),
            fixture.runtime.execute_tool_call(
                fixture.documents.clone(),
                &context,
                review.request_id,
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert_eq!(failed.code, RuntimeError::TimedOut);
        assert_eq!(failed.outcome, CallOutcome::Unknown);
        fixture.phase(ConnectionPhase::Failed).await;
        assert_eq!(fixture.call_count(), 1);
        fixture.runtime.shutdown().await;
    }
    #[tokio::test]
    async fn changed_credentials_are_rechecked_even_without_a_ui_write() {
        struct MutableSecret(Mutex<String>);
        impl SecretStore for MutableSecret {
            fn get(&self, _: Uuid, _: &str) -> Result<Option<String>, RuntimeError> {
                Ok(Some(self.0.lock().unwrap().clone()))
            }
        }
        let mut fixture = Fixture::new("normal");
        let secret = Arc::new(MutableSecret(Mutex::new("before".into())));
        fixture.runtime = Arc::new(IntegrationRuntime::new(
            fixture.root.path().into(),
            secret.clone(),
        ));
        fixture.configure(|config| {
            let ConnectionConfig::Mcp {
                transport: McpTransport::Stdio { process },
            } = &mut config.entries[0].connection
            else {
                panic!()
            };
            process.env.insert(
                "TEST_TOKEN".into(),
                crate::integrations::config::ConfigValue::Secret {
                    name: "token".into(),
                },
            );
        });
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let context = fixture.call_context();
        let review = fixture.review(&context);
        fixture
            .runtime
            .decide_tool_call(&context, review.request_id, true)
            .unwrap();
        *secret.0.lock().unwrap() = "after".into();
        let error = fixture
            .runtime
            .execute_tool_call(fixture.documents.clone(), &context, review.request_id)
            .await
            .unwrap_err();
        assert_eq!(error.code, RuntimeError::ConfigChanged);
        assert_eq!(error.outcome, CallOutcome::NotStarted);
        assert_eq!(fixture.call_count(), 0);
        fixture.runtime.shutdown().await;
    }

    #[tokio::test]
    async fn policy_changes_during_a_call_stop_it_without_waiting_for_the_call_deadline() {
        let mut fixture = Fixture::new("slow_call");
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let context = fixture.call_context();
        let review = fixture.review(&context);
        fixture
            .runtime
            .decide_tool_call(&context, review.request_id, true)
            .unwrap();
        let runtime = fixture.runtime.clone();
        let documents = fixture.documents.clone();
        let task = tokio::spawn(async move {
            runtime
                .execute_tool_call(documents, &context, review.request_id)
                .await
        });
        fixture.called().await;
        fixture.configure(|config| config.entries[0].enabled = false);
        let error = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap_err();
        assert_eq!(error.outcome, CallOutcome::Unknown);
        assert_eq!(error.code, RuntimeError::IntegrationDisabled);
        fixture.phase(ConnectionPhase::Failed).await;
        assert_eq!(fixture.call_count(), 1);
        fixture.runtime.shutdown().await;
    }

    #[tokio::test]
    async fn aliases_arguments_and_workspace_identity_are_checked_before_permission() {
        let fixture = Fixture::new("normal");
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let context = fixture.call_context();
        let review = fixture.review(&context);
        let target = review.target;
        let mut original_name = target.clone();
        original_name.alias = "search".into();
        assert!(matches!(
            fixture.runtime.prepare_tool_call(
                &fixture.documents,
                &context,
                original_name,
                json!({})
            ),
            Err(RuntimeError::InvalidCatalog)
        ));
        assert!(matches!(
            fixture.runtime.prepare_tool_call(
                &fixture.documents,
                &context,
                target.clone(),
                json!([])
            ),
            Err(RuntimeError::InvalidArguments)
        ));
        assert!(matches!(
            fixture.runtime.prepare_tool_call(
                &fixture.documents,
                &context,
                target.clone(),
                json!({"data":"x".repeat(65536)})
            ),
            Err(RuntimeError::OutputLimit)
        ));
        let reopened = DocumentService::new(fixture.root.path().into());
        assert!(matches!(
            fixture
                .runtime
                .prepare_tool_call(&reopened, &context, target, json!({})),
            Err(RuntimeError::WorkspaceChanged)
        ));
        assert_eq!(fixture.call_count(), 0);
        fixture.runtime.shutdown().await;
    }

    fn native_call(
        turn: &super::super::native_turn::NativeMcpTurn,
        id: &str,
    ) -> crate::protocol::ToolCall {
        serde_json::from_value(json!({"id":id,"type":"function","function":{"name":turn.schemas()[0]["function"]["name"],"arguments":"{\"query\":\"native\"}"}})).unwrap()
    }
    async fn native_pending(turn: &super::super::native_turn::NativeMcpTurn) -> Uuid {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(review) = turn.state().unwrap().pending.first() {
                    return review.request_id;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn native_conversation_consent_artifacts_and_remote_gate() {
        use super::super::{native_turn::NativeMcpTurn, result_artifact::ArtifactRef};
        for mode in ["normal", "modern", "tool_error"] {
            let fixture = Fixture::new(mode);
            fixture.connect().await;
            fixture.phase(ConnectionPhase::Connected).await;
            let conversation = Uuid::new_v4();
            let turn = NativeMcpTurn::new(
                fixture.runtime.clone(),
                fixture.documents.clone(),
                conversation,
                false,
            )
            .unwrap();
            assert_eq!(turn.schemas().len(), 1);
            let call = native_call(&turn, "native-1");
            let task = tokio::spawn({
                let turn = turn.clone();
                let call = call.clone();
                async move { turn.execute(&call).await }
            });
            let request = native_pending(&turn).await;
            assert_eq!(fixture.call_count(), 0);
            turn.respond(request, false).unwrap();
            let denied = task.await.unwrap();
            let denied: serde_json::Value = serde_json::from_str(&denied.content).unwrap();
            assert_eq!(denied["result"]["outcome"], "not_started");
            assert_eq!(fixture.call_count(), 0);
            let mut call = call;
            call.id = "native-2".into();
            let task = tokio::spawn({
                let turn = turn.clone();
                let call = call.clone();
                async move { turn.execute(&call).await }
            });
            let request = native_pending(&turn).await;
            turn.respond(request, true).unwrap();
            assert_eq!(
                turn.respond(request, true),
                Err(RuntimeError::ApprovalExpired)
            );
            let result = task.await.unwrap();
            assert_eq!(result.success, mode != "tool_error");
            let envelope: serde_json::Value = serde_json::from_str(&result.content).unwrap();
            assert_eq!(envelope["tool"], "search");
            let reference: ArtifactRef =
                serde_json::from_value(envelope["result"]["artifact"].clone()).unwrap();
            let stored = fixture
                .runtime
                .read_result_artifact(&fixture.documents.identity().workspace_id, &reference)
                .unwrap();
            assert_eq!(
                stored["result"]["structuredContent"]["arguments"]["query"],
                "native"
            );
            assert_eq!(stored["scope"]["conversation_id"], conversation.to_string());
            assert!(fixture
                .runtime
                .read_result_artifact("wrong-workspace", &reference)
                .is_err());
            assert!(!turn.execute(&call).await.success);
            assert_eq!(fixture.call_count(), 1);
            let remote = NativeMcpTurn::new(
                fixture.runtime.clone(),
                fixture.documents.clone(),
                conversation,
                true,
            )
            .unwrap();
            assert!(remote.schemas().is_empty());
            assert_eq!(remote.state().unwrap().blocked, Some("remote"));
            assert!(!remote.execute(&call).await.success);
            fixture.runtime.shutdown().await;
        }
    }
    #[tokio::test]
    async fn native_cancel_pending_and_unknown_calls_do_not_repeat() {
        use super::super::native_turn::NativeMcpTurn;
        let fixture = Fixture::new("slow_call");
        fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let pending_turn = NativeMcpTurn::new(
            fixture.runtime.clone(),
            fixture.documents.clone(),
            Uuid::new_v4(),
            false,
        )
        .unwrap();
        let call = native_call(&pending_turn, "cancel-before");
        let task = tokio::spawn({
            let turn = pending_turn.clone();
            async move { turn.execute(&call).await }
        });
        let request = native_pending(&pending_turn).await;
        pending_turn.cancel.cancel();
        assert_eq!(
            pending_turn.respond(request, true),
            Err(RuntimeError::Cancelled)
        );
        let result: serde_json::Value = serde_json::from_str(&task.await.unwrap().content).unwrap();
        assert_eq!(result["result"]["outcome"], "not_started");
        assert_eq!(fixture.call_count(), 0);
        let turn = NativeMcpTurn::new(
            fixture.runtime.clone(),
            fixture.documents.clone(),
            Uuid::new_v4(),
            false,
        )
        .unwrap();
        let mut call = native_call(&turn, "cancel-after");
        let task = tokio::spawn({
            let turn = turn.clone();
            let call = call.clone();
            async move { turn.execute(&call).await }
        });
        let request = native_pending(&turn).await;
        turn.respond(request, true).unwrap();
        fixture.called().await;
        turn.cancel.cancel();
        let result: serde_json::Value = serde_json::from_str(&task.await.unwrap().content).unwrap();
        assert_eq!(result["result"]["outcome"], "unknown");
        assert_eq!(turn.state().unwrap().blocked, Some("uncertain"));
        assert!(turn.state().unwrap().pending.is_empty());
        call.id = "model-retry".into();
        assert!(!turn.execute(&call).await.success);
        assert_eq!(fixture.call_count(), 1);
        fixture.runtime.shutdown().await;
    }

    #[tokio::test]
    async fn native_provider_wire_conversation_uses_approved_mcp_with_index_disabled() {
        use super::super::native_turn::NativeMcpTurn;
        use crate::{
            ai_workflow::PendingToolBatch,
            chat_manager::{ChatManager, DrainResult},
            config::ApiConfig,
            conversation::ConversationHistory,
            models::registry::ModelInfo,
            protocol::{ChatMessage, ChatRole},
        };
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        for provider in ["ollama", "openai-compat"] {
            let fixture = Fixture::new("normal");
            let mut settings = crate::project_settings::ProjectSettings::default();
            settings.integrations.symbols_index_enabled = Some(false);
            crate::project_settings::save_project_settings(fixture.root.path(), &settings).unwrap();
            fixture.connect().await;
            fixture.phase(ConnectionPhase::Connected).await;
            let mut history = ConversationHistory::new();
            let turn = NativeMcpTurn::new(
                fixture.runtime.clone(),
                fixture.documents.clone(),
                Uuid::parse_str(&history.metadata.id).unwrap(),
                false,
            )
            .unwrap();
            let alias = turn.schemas()[0]["function"]["name"]
                .as_str()
                .unwrap()
                .to_string();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server_alias = alias.clone();
            let peer = tokio::spawn(async move {
                let mut requests = Vec::new();
                for round in 0..2 {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut bytes = Vec::new();
                    let (header_end, length) = loop {
                        let mut buffer = [0; 4096];
                        let count = socket.read(&mut buffer).await.unwrap();
                        assert!(count > 0);
                        bytes.extend_from_slice(&buffer[..count]);
                        if let Some(start) = bytes.windows(4).position(|slice| slice == b"\r\n\r\n")
                        {
                            let header = String::from_utf8_lossy(&bytes[..start]).to_lowercase();
                            let length = header
                                .lines()
                                .find_map(|line| line.strip_prefix("content-length:"))
                                .unwrap()
                                .trim()
                                .parse::<usize>()
                                .unwrap();
                            break (start + 4, length);
                        }
                    };
                    while bytes.len() < header_end + length {
                        let mut buffer = [0; 4096];
                        let count = socket.read(&mut buffer).await.unwrap();
                        assert!(count > 0);
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    let request: serde_json::Value =
                        serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
                    requests.push(request);
                    let response = if provider == "ollama" {
                        if round == 0 {
                            json!({"model":"mcp-fixture","message":{"role":"assistant","content":"","tool_calls":[{"id":"native-wire","type":"function","function":{"name":server_alias,"arguments":{"query":"wire"}}}]},"done":true,"done_reason":"stop"}).to_string() + "\n"
                        } else {
                            json!({"model":"mcp-fixture","message":{"role":"assistant","content":"Read the MCP result."},"done":true,"done_reason":"stop"}).to_string() + "\n"
                        }
                    } else {
                        let delta = if round == 0 {
                            json!({"tool_calls":[{"index":0,"id":"native-wire","type":"function","function":{"name":server_alias,"arguments":"{\"query\":\"wire\"}"}}]})
                        } else {
                            json!({"content":"Read the MCP result."})
                        };
                        format!(
                            "data: {}\n\ndata: [DONE]\n\n",
                            json!({"choices":[{"index":0,"delta":delta,"finish_reason":if round==0 {"tool_calls"} else {"stop"}}]})
                        )
                    };
                    let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", if provider=="ollama" {"application/x-ndjson"} else {"text/event-stream"}, response.len(), response);
                    socket.write_all(reply.as_bytes()).await.unwrap();
                }
                requests
            });
            let config = ApiConfig {
                ollama_url: url.clone(),
                openai_compat_url: url,
                ..Default::default()
            };
            let models = vec![ModelInfo {
                id: format!("{provider}/mcp-fixture"),
                name: "Fixture".into(),
                description: String::new(),
                provider: Some(provider.into()),
                reasoning_effort: None,
                api_id: None,
            }];
            let workspace = fixture.root.path().to_path_buf();
            let mut manager = ChatManager::new(50);
            manager.native_mcp = Some(turn.clone());
            history.push(ChatMessage::new(
                ChatRole::User,
                "Use the connected tool".into(),
            ));
            manager
                .start_stream(
                    "Use the connected tool".into(),
                    &mut history,
                    &config,
                    &models,
                    0,
                    Some(&workspace),
                    None,
                    None,
                    None,
                    None,
                    reqwest::Client::new(),
                    Some("local".into()),
                    None,
                    true,
                )
                .unwrap();
            let calls = tokio::time::timeout(Duration::from_secs(10), async {
                loop {
                    match manager.drain_events(&mut history) {
                        DrainResult::ToolCalls(calls, _) => break calls,
                        DrainResult::Error(error) => panic!("provider error: {error}"),
                        _ => tokio::time::sleep(Duration::from_millis(10)).await,
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].function.name, alias);
            let task = tokio::spawn({
                let turn = turn.clone();
                let call = calls[0].clone();
                async move { turn.execute(&call).await }
            });
            let review = native_pending(&turn).await;
            assert_eq!(fixture.call_count(), 0);
            turn.respond(review, true).unwrap();
            let result = task.await.unwrap();
            assert!(result.success);
            let results = vec![(calls[0].clone(), result)];
            manager.record_tool_results(&results, &mut history, true);
            let batch = PendingToolBatch {
                calls,
                file_results: results,
                commands: Vec::new(),
                changes: Vec::new(),
                confirms: Vec::new(),
                loop_detected: false,
            };
            manager
                .continue_tool_batch(
                    batch,
                    &mut history,
                    &config,
                    &models,
                    0,
                    Some(&workspace),
                    true,
                    reqwest::Client::new(),
                )
                .unwrap();
            let requests = tokio::time::timeout(Duration::from_secs(10), peer)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(requests.len(), 2);
            let assistant = requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|message| {
                    message
                        .get("tool_calls")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|calls| !calls.is_empty())
                })
                .unwrap();
            assert!(assistant["tool_calls"][0].get("result").is_none());
            assert!(assistant["tool_calls"][0].get("status").is_none());
            for request in &requests {
                let tools = request["tools"].as_array().unwrap();
                assert!(tools.iter().any(|tool| tool["function"]["name"] == alias));
                assert!(!tools
                    .iter()
                    .any(|tool| tool["function"]["name"] == "symbol_search"));
            }
            let results: Vec<_> = requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|message| message["role"] == "tool")
                .collect();
            assert_eq!(
                results.len(),
                1,
                "immediate persistence must not duplicate continuation results"
            );
            let content: serde_json::Value =
                serde_json::from_str(results[0]["content"].as_str().unwrap()).unwrap();
            assert_eq!(content["mcp_result_version"], 1);
            assert_eq!(content["result"]["outcome"], "completed");
            assert!(content["result"]["projection"]["text"]
                .as_str()
                .unwrap()
                .contains("wire"));
            assert!(!fixture
                .root
                .path()
                .join(".zblade/index/symbols.db")
                .exists());
            manager.request_stop();
            fixture.runtime.shutdown().await;
        }
    }

    #[tokio::test]
    async fn native_catalog_snapshot_and_pending_requests_retire_safely() {
        use super::super::native_turn::NativeMcpTurn;
        let fixture = Fixture::new("normal");
        let connection = fixture.connect().await;
        fixture.phase(ConnectionPhase::Connected).await;
        let turn = NativeMcpTurn::new(
            fixture.runtime.clone(),
            fixture.documents.clone(),
            Uuid::new_v4(),
            false,
        )
        .unwrap();
        let call = native_call(&turn, "abandoned");
        let task = tokio::spawn({
            let turn = turn.clone();
            async move { turn.execute(&call).await }
        });
        let request = native_pending(&turn).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(turn.state().unwrap().pending.is_empty());
        assert_eq!(
            turn.respond(request, true),
            Err(RuntimeError::ApprovalExpired)
        );
        let before = turn.schemas();
        fixture
            .runtime
            .connections
            .refresh(connection.connection_id, &fixture.documents)
            .unwrap();
        fixture.phase(ConnectionPhase::Connected).await;
        assert_eq!(
            turn.schemas(),
            before,
            "a live refresh cannot remap an in-flight catalog"
        );
        let call = native_call(&turn, "stale");
        let result: serde_json::Value =
            serde_json::from_str(&turn.execute(&call).await.content).unwrap();
        assert_eq!(result["result"]["error"], "catalog_changed");
        assert_eq!(fixture.call_count(), 0);
        let turn = NativeMcpTurn::new(
            fixture.runtime.clone(),
            fixture.documents.clone(),
            Uuid::new_v4(),
            false,
        )
        .unwrap();
        let call = native_call(&turn, "disconnected");
        let task = tokio::spawn({
            let turn = turn.clone();
            async move { turn.execute(&call).await }
        });
        native_pending(&turn).await;
        fixture
            .runtime
            .connections
            .disconnect(connection.connection_id, &fixture.documents)
            .unwrap();
        let result = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(result["result"]["outcome"], "not_started");
        assert!(turn.state().unwrap().pending.is_empty());
        fixture.runtime.shutdown().await;
    }
}
