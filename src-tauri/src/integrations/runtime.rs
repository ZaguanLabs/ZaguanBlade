//! Request-scoped, one-use launch approvals. Saving/enabling a definition never
//! grants trust. A probe cannot be repurposed for a long-running connection.
use super::{
    config::{ConnectionConfig, McpTransport},
    credentials::SecretStore,
    identity::WorkspaceIdentity,
    probe::{self, ProbeResult},
    process::{PreparedProcess, SupervisedProcess},
    store::IntegrationStore,
    RuntimeError,
};
use crate::document_service::DocumentService;
use serde::Serialize;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const APPROVAL_TTL: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Serialize)]
pub struct LaunchReview {
    pub ticket_id: Uuid,
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<String>,
}

struct Ticket {
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
    running: HashMap<Uuid, CancellationToken>,
}

pub struct IntegrationRuntime {
    directory: PathBuf,
    secrets: Arc<dyn SecretStore>,
    requests: Mutex<Requests>,
    preparing: Arc<tokio::sync::Semaphore>,
}

impl IntegrationRuntime {
    pub fn new(directory: PathBuf, secrets: Arc<dyn SecretStore>) -> Self {
        Self {
            directory,
            secrets,
            requests: Mutex::new(Requests::default()),
            preparing: Arc::new(tokio::sync::Semaphore::new(4)),
        }
    }

    fn resolve(
        &self,
        integration: Uuid,
        revision: &str,
        documents: &DocumentService,
    ) -> Result<(PreparedProcess, bool), RuntimeError> {
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
        let (process, is_acp) = match &entry.connection {
            ConnectionConfig::Mcp {
                transport: McpTransport::Stdio { process },
            } => (process, false),
            ConnectionConfig::Acp { process, .. } => (process, true),
            _ => return Err(RuntimeError::UnsupportedTransport),
        };
        Ok((
            PreparedProcess::resolve(
                process,
                integration,
                documents.workspace_root(),
                self.secrets.as_ref(),
            )?,
            is_acp,
        ))
    }

    pub fn prepare(
        &self,
        integration: Uuid,
        revision: String,
        documents: &DocumentService,
    ) -> Result<LaunchReview, RuntimeError> {
        let _slot = self
            .preparing
            .clone()
            .try_acquire_owned()
            .map_err(|_| RuntimeError::Busy)?;
        let (process, _) = self.resolve(integration, &revision, documents)?;
        let review = LaunchReview {
            ticket_id: Uuid::new_v4(),
            executable: process.executable,
            cwd: process.cwd,
            args: process.args,
        };
        let mut requests = self.requests.lock().map_err(|_| RuntimeError::Busy)?;
        requests.pending.retain(|_, ticket| {
            ticket.expires > Instant::now() && !ticket.workspace_cancel.is_cancelled()
        });
        if requests.pending.len() + requests.running.len() >= 32 {
            return Err(RuntimeError::Busy);
        }
        requests.pending.insert(
            review.ticket_id,
            Ticket {
                integration,
                revision,
                workspace: documents.identity().clone(),
                fingerprint: process.fingerprint,
                expires: Instant::now() + APPROVAL_TTL,
                cancel: CancellationToken::new(),
                workspace_cancel: documents.cancellation(),
            },
        );
        Ok(review)
    }

    pub fn cancel(&self, ticket_id: Uuid) {
        if let Ok(mut requests) = self.requests.lock() {
            if let Some(ticket) = requests.pending.remove(&ticket_id) {
                ticket.cancel.cancel();
            }
            if let Some(cancel) = requests.running.get(&ticket_id) {
                cancel.cancel();
            }
        }
    }

    pub async fn run(
        self: &Arc<Self>,
        ticket_id: Uuid,
        documents: Arc<DocumentService>,
    ) -> Result<ProbeResult, RuntimeError> {
        let ticket = {
            let mut requests = self.requests.lock().map_err(|_| RuntimeError::Busy)?;
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
            requests.running.insert(ticket_id, ticket.cancel.clone());
            ticket
        };
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
            if prepared.fingerprint != ticket.fingerprint {
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
            !fixture.runtime.requests.lock().unwrap().running[&second.ticket_id].is_cancelled()
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
