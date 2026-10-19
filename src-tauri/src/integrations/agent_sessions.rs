//! The live ACP agent session. One at a time, bound to the active
//! conversation: switching conversations, changing workspace or shutting down
//! ends it. Resuming a stored session is phase 6.
use super::{
    acp_config::SessionConfig,
    acp_forward::ForwardingView,
    acp_session::{AcpSession, AgentSnapshot, Reopened, TurnRecorder},
    process::SupervisedProcess,
    runtime::{AgentApproval, AgentCommand},
    RuntimeError,
};
use serde::Serialize;
use std::{
    ffi::OsString,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub struct LiveAgent {
    pub id: Uuid,
    pub conversation_id: String,
    pub integration: Uuid,
    pub name: String,
    pub session: AcpSession,
    pub session_id: Mutex<Option<String>>,
    pub recorder: Arc<Mutex<TurnRecorder>>,
    /// The MCP servers the agent was given, and those it was not.
    pub forwarding: ForwardingView,
    /// How the program was started, to run it for a terminal sign-in and
    /// start it again afterwards. `None` when it cannot be (tests).
    pub launch: Option<AgentLaunch>,
    signing_in: AtomicBool,
    closed: CancellationToken,
}

pub struct AgentLaunch {
    pub command: AgentCommand,
    pub environment: Vec<(OsString, OsString)>,
    pub approval: AgentApproval,
}

/// Held while a terminal sign-in runs; one at a time.
pub struct SigningIn<'a>(&'a AtomicBool);
impl Drop for SigningIn<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct AgentStatus {
    pub conversation_id: String,
    pub integration_id: Uuid,
    pub name: String,
    pub snapshot: AgentSnapshot,
    /// `false` until the agent has created a session (it may need a sign-in).
    pub ready: bool,
    pub forwarding: ForwardingView,
    /// The agent's session settings (model, effort, mode…).
    pub config: SessionConfig,
    /// How the session began; `None` until it exists.
    pub reopened: Option<Reopened>,
}

impl LiveAgent {
    pub fn new(
        conversation_id: String,
        integration: Uuid,
        name: String,
        session: AcpSession,
        session_id: Option<String>,
        recorder: Arc<Mutex<TurnRecorder>>,
        forwarding: ForwardingView,
        launch: Option<AgentLaunch>,
    ) -> Arc<Self> {
        Arc::new(Self {
            id: Uuid::new_v4(),
            conversation_id,
            integration,
            name,
            session,
            session_id: Mutex::new(session_id),
            recorder,
            forwarding,
            launch,
            signing_in: AtomicBool::new(false),
            closed: CancellationToken::new(),
        })
    }

    /// `None` while another sign-in runs.
    pub fn begin_sign_in(&self) -> Option<SigningIn<'_>> {
        self.signing_in
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| SigningIn(&self.signing_in))
    }

    pub fn status(&self) -> AgentStatus {
        AgentStatus {
            conversation_id: self.conversation_id.clone(),
            integration_id: self.integration,
            name: self.name.clone(),
            snapshot: (*self.session.snapshot).clone(),
            ready: self
                .session_id
                .lock()
                .map(|id| id.is_some())
                .unwrap_or(false),
            forwarding: self.forwarding.clone(),
            config: self.session.config(),
            reopened: self.session.reopened(),
        }
    }

    pub fn close(&self) {
        self.closed.cancel();
        self.session.close();
    }
}

#[derive(Default)]
pub struct AgentSessions {
    live: Mutex<Option<Arc<LiveAgent>>>,
}

impl AgentSessions {
    /// Makes `live` the current session and supervises its program until the
    /// session ends, the workspace changes or it is closed.
    pub fn install(
        self: &Arc<Self>,
        live: Arc<LiveAgent>,
        process: SupervisedProcess,
        workspace_cancel: CancellationToken,
    ) -> Option<Arc<LiveAgent>> {
        let previous = self
            .live
            .lock()
            .ok()
            .and_then(|mut slot| slot.replace(live.clone()));
        let sessions = self.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = live.session.finished() => {}
                _ = workspace_cancel.cancelled() => live.close(),
                _ = live.closed.cancelled() => {}
            }
            // Let the actor answer pending requests and notify the agent.
            let _ = tokio::time::timeout(Duration::from_secs(2), live.session.finished()).await;
            process.shutdown().await;
            if let Ok(mut slot) = sessions.live.lock() {
                if slot.as_ref().is_some_and(|current| current.id == live.id) {
                    *slot = None;
                }
            }
        });
        previous
    }

    pub fn current(&self) -> Option<Arc<LiveAgent>> {
        self.live.lock().ok().and_then(|slot| slot.clone())
    }

    /// The live session of `conversation_id`.
    pub fn bound(&self, conversation_id: &str) -> Result<Arc<LiveAgent>, RuntimeError> {
        self.current()
            .filter(|live| live.conversation_id == conversation_id && !live.session.is_finished())
            .ok_or(RuntimeError::ConnectionNotReady)
    }

    /// Removes the live session unless it belongs to `keep`, returning it so
    /// the caller can record an interrupted turn before it closes.
    pub fn take_unless(&self, keep: Option<&str>) -> Option<Arc<LiveAgent>> {
        let mut slot = self.live.lock().ok()?;
        if slot
            .as_ref()
            .is_some_and(|live| Some(live.conversation_id.as_str()) == keep)
        {
            return None;
        }
        slot.take()
    }

    pub fn close_all(&self) {
        if let Some(live) = self.take_unless(None) {
            live.close();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::integrations::{
        acp_session::tests::ready, config::ProcessConfig, credentials::SecretStore,
        process::PreparedProcess,
    };
    use tokio::io::AsyncReadExt;

    struct NoSecrets;
    impl SecretStore for NoSecrets {
        fn get(&self, _: Uuid, _: &str) -> Result<Option<String>, RuntimeError> {
            Ok(None)
        }
    }

    /// A program that stands in for the agent's process; its stdout closes
    /// when the supervisor stops it.
    async fn program() -> (SupervisedProcess, tokio::process::ChildStdout) {
        let root = tempfile::tempdir().unwrap();
        let config = ProcessConfig {
            command: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 60 & printf ready; wait".into()],
            cwd: None,
            env: Default::default(),
        };
        let prepared =
            PreparedProcess::resolve(&config, Uuid::new_v4(), root.path(), &NoSecrets).unwrap();
        let (process, mut stdout, _stdin) = SupervisedProcess::spawn(&prepared).unwrap();
        let mut ready = [0; 5];
        stdout.read_exact(&mut ready).await.unwrap();
        (process, stdout)
    }

    async fn stopped(mut stdout: tokio::process::ChildStdout) {
        let mut rest = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stdout.read_to_end(&mut rest))
            .await
            .expect("the program was not stopped")
            .unwrap();
    }

    async fn cleared(sessions: &AgentSessions) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while sessions.current().is_some() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the slot was not cleared");
    }

    fn live(conversation: &str, session: AcpSession) -> Arc<LiveAgent> {
        LiveAgent::new(
            conversation.into(),
            Uuid::new_v4(),
            "Agent".into(),
            session,
            Some("s1".into()),
            Arc::default(),
            ForwardingView::default(),
            None,
        )
    }

    #[tokio::test]
    async fn the_live_session_is_bound_to_its_conversation_and_stops_its_program() {
        let sessions = Arc::new(AgentSessions::default());
        let (_wire, session, _events) = ready().await;
        let (process, stdout) = program().await;
        let first = live("c1", session);
        assert!(sessions
            .install(first.clone(), process, CancellationToken::new())
            .is_none());
        assert!(sessions.bound("c1").unwrap().status().ready);
        assert_eq!(
            sessions.bound("c2").err(),
            Some(RuntimeError::ConnectionNotReady)
        );
        assert!(sessions.take_unless(Some("c1")).is_none(), "kept");
        let taken = sessions.take_unless(Some("c2")).unwrap();
        assert_eq!(taken.id, first.id);
        taken.close();
        stopped(stdout).await;
        assert!(first.session.is_finished());
        assert!(sessions.current().is_none());
    }

    #[tokio::test]
    async fn an_exited_agent_or_a_workspace_change_ends_the_session() {
        let sessions = Arc::new(AgentSessions::default());
        let (wire, session, _events) = ready().await;
        let (process, stdout) = program().await;
        sessions.install(live("c1", session), process, CancellationToken::new());
        drop(wire);
        stopped(stdout).await;
        cleared(&sessions).await;

        let (_wire, session, _events) = ready().await;
        let (process, stdout) = program().await;
        let workspace = CancellationToken::new();
        let second = live("c2", session);
        sessions.install(second.clone(), process, workspace.clone());
        workspace.cancel();
        stopped(stdout).await;
        cleared(&sessions).await;
        assert!(second.session.is_finished());
    }
}
