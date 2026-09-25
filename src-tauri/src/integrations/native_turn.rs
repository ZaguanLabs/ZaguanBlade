//! Native conversation owner for catalog snapshots, desktop permissions and calls.
use super::{
    call_result::{CallFailure, CallOutcome},
    connections::ConnectionPhase,
    native_catalog::{ExcludedTool, NativeCatalog, MAX_NATIVE_TOOLS},
    permissions::{CallContext, CallReview},
    result_artifact,
    runtime::IntegrationRuntime,
    RuntimeError,
};
use crate::{document_service::DocumentService, protocol::ToolCall, tools::ToolResult};
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct Decision {
    review: CallReview,
    context: CallContext,
    sender: oneshot::Sender<()>,
}
// Dropping a waiting invocation must remove its UI request and revoke its
// context even if its future is abandoned before normal cleanup runs.
struct PendingReviewGuard<'a> {
    turn: &'a NativeMcpTurn,
    id: Uuid,
    cancel: CancellationToken,
}
impl Drop for PendingReviewGuard<'_> {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Ok(mut pending) = self.turn.pending.lock() {
            pending.remove(&self.id);
        }
    }
}
#[derive(Serialize)]
pub struct TurnState {
    pub turn_id: Uuid,
    pub conversation_id: Uuid,
    pub available: usize,
    pub limit: usize,
    pub excluded: Vec<ExcludedTool>,
    pub blocked: Option<&'static str>,
    pub pending: Vec<CallReview>,
}
pub struct NativeMcpTurn {
    pub id: Uuid,
    pub conversation: Uuid,
    documents: Arc<DocumentService>,
    runtime: Arc<IntegrationRuntime>,
    pub cancel: CancellationToken,
    catalog: NativeCatalog,
    pending: Mutex<HashMap<Uuid, Decision>>,
    seen: Mutex<HashSet<String>>,
    blocked: Mutex<Option<&'static str>>,
}
impl NativeMcpTurn {
    pub fn new(
        runtime: Arc<IntegrationRuntime>,
        documents: Arc<DocumentService>,
        conversation: Uuid,
        remote: bool,
    ) -> Result<Arc<Self>, RuntimeError> {
        let mut catalog = NativeCatalog::default();
        let statuses = runtime.connections.statuses(&documents)?;
        let remote = remote
            && statuses
                .iter()
                .any(|status| status.phase == ConnectionPhase::Connected && status.tools > 0);
        if !remote {
            for status in statuses {
                if status.phase != ConnectionPhase::Connected {
                    continue;
                }
                if let Ok(snapshot) = runtime
                    .connections
                    .catalog(status.connection_id, &documents)
                {
                    let binding = snapshot.tools().first().and_then(|tool| {
                        runtime
                            .connections
                            .call_binding(
                                &super::permissions::CallTarget {
                                    connection_id: status.connection_id,
                                    alias: tool.alias.clone(),
                                    catalog_revision: snapshot.revision().into(),
                                },
                                &documents,
                            )
                            .ok()
                    });
                    if let Some(binding) = binding {
                        catalog.add(status.connection_id, &binding.definition.name, &snapshot);
                    }
                }
            }
        }
        Ok(Arc::new(Self {
            id: Uuid::new_v4(),
            conversation,
            documents,
            runtime,
            cancel: CancellationToken::new(),
            catalog,
            pending: Mutex::new(HashMap::new()),
            seen: Mutex::new(HashSet::new()),
            blocked: Mutex::new(remote.then_some("remote")),
        }))
    }
    pub fn has_tools(&self) -> bool {
        !self.catalog.tools.is_empty()
    }
    pub fn is_current(&self) -> bool {
        !self.cancel.is_cancelled() && !self.documents.cancellation().is_cancelled()
    }
    pub fn matches(&self, documents: &DocumentService, conversation: &str) -> bool {
        self.documents.identity() == documents.identity()
            && self.conversation.to_string() == conversation
    }
    pub fn state(&self) -> Result<TurnState, RuntimeError> {
        let pending = if self.cancel.is_cancelled() || self.documents.cancellation().is_cancelled()
        {
            Vec::new()
        } else {
            self.pending
                .lock()
                .map_err(|_| RuntimeError::Busy)?
                .values()
                .map(|entry| entry.review.clone())
                .collect()
        };
        Ok(TurnState {
            turn_id: self.id,
            conversation_id: self.conversation,
            available: self.catalog.tools.len(),
            limit: MAX_NATIVE_TOOLS,
            excluded: self.catalog.excluded.clone(),
            blocked: *self.blocked.lock().map_err(|_| RuntimeError::Busy)?,
            pending,
        })
    }
    pub fn schemas(&self) -> Vec<Value> {
        if self.cancel.is_cancelled() || self.documents.cancellation().is_cancelled() {
            return Vec::new();
        }
        if self
            .blocked
            .lock()
            .map(|reason| reason.is_some())
            .unwrap_or(true)
        {
            return Vec::new();
        }
        self.catalog
            .tools
            .values()
            .map(|tool| tool.schema.clone())
            .collect()
    }
    pub fn block(&self, reason: &'static str) {
        if let Ok(mut blocked) = self.blocked.lock() {
            *blocked = Some(reason);
        }
    }
    pub fn respond(&self, request: Uuid, allow: bool) -> Result<(), RuntimeError> {
        if self.cancel.is_cancelled() || self.documents.cancellation().is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        let mut pending = self.pending.lock().map_err(|_| RuntimeError::Busy)?;
        let decision = pending.get(&request).ok_or(RuntimeError::ApprovalExpired)?;
        self.runtime
            .decide_tool_call(&decision.context, request, allow)?;
        if let Some(decision) = pending.remove(&request) {
            let _ = decision.sender.send(());
        }
        Ok(())
    }
    pub async fn execute(&self, call: &ToolCall) -> ToolResult {
        let tool = self.catalog.tools.get(&call.function.name);
        let outcome = self.execute_inner(call).await;
        let (success, payload) = match outcome {
            Ok(result) => {
                let directory = self.runtime.directory.clone();
                let projection = json!({"text":result.text,"truncated":result.text_truncated,"non_text_blocks":result.non_text_blocks,"is_error":result.is_error});
                let success = !result.is_error;
                let artifact =
                    tokio::task::spawn_blocking(move || result_artifact::save(&directory, &result))
                        .await
                        .ok()
                        .and_then(Result::ok);
                (
                    success,
                    json!({"projection":projection,"artifact":artifact,"artifact_unavailable":artifact.is_none(),"outcome":"completed"}),
                )
            }
            Err(error) if error.outcome == CallOutcome::NotStarted => (
                false,
                json!({"error":error.code,"outcome":error.outcome,"retry":false}),
            ),
            Err(error) => {
                // Retain uncertain outcomes even if the owning conversation or
                // workspace closes before the orchestrator can update history.
                let directory = self.runtime.directory.clone();
                let scope = super::permissions::CallScope {
                    workspace: self.documents.identity().clone(),
                    conversation_id: self.conversation,
                    turn_id: self.id,
                    call_id: call.id.clone(),
                };
                let value = json!({"schema_version":1,"scope":scope,"target":tool.map(|tool| &tool.target),"error":error.code,"outcome":error.outcome});
                let artifact = tokio::task::spawn_blocking(move || {
                    result_artifact::save_value(&directory, &scope, &value)
                })
                .await
                .ok()
                .and_then(Result::ok);
                (
                    false,
                    json!({"error":error.code,"outcome":error.outcome,"retry":false,"artifact":artifact,"artifact_unavailable":artifact.is_none()}),
                )
            }
        };
        let content = json!({"mcp_result_version":1,"server":tool.map(|t| t.server_name.as_str()),"tool":tool.map(|t| t.original_name.as_str()),
            "alias":call.function.name,"turn_id":self.id,"conversation_id":self.conversation,"result":payload}).to_string();
        ToolResult {
            success,
            content,
            error: None,
            skipped: false,
        }
    }
    async fn execute_inner(
        &self,
        call: &ToolCall,
    ) -> Result<super::call_result::McpCallResult, CallFailure> {
        if self
            .blocked
            .lock()
            .map_err(|_| RuntimeError::Busy)?
            .is_some()
        {
            return Err(RuntimeError::PermissionDenied.into());
        }
        let tool = self
            .catalog
            .tools
            .get(&call.function.name)
            .ok_or(RuntimeError::InvalidCatalog)?;
        if call.id.is_empty() || call.id.len() > 256 || call.id.chars().any(char::is_control) {
            return Err(RuntimeError::InvalidArguments.into());
        }
        {
            let mut seen = self.seen.lock().map_err(|_| RuntimeError::Busy)?;
            if seen.len() >= 32 || !seen.insert(call.id.clone()) {
                return Err(RuntimeError::CallLimit.into());
            }
        }
        if call.function.arguments.len() > super::call_result::MAX_ARGUMENT_BYTES {
            return Err(RuntimeError::OutputLimit.into());
        }
        let arguments = serde_json::from_str(&call.function.arguments)
            .map_err(|_| RuntimeError::InvalidArguments)?;
        let context = CallContext::new(
            &self.documents,
            self.conversation,
            self.id,
            call.id.clone(),
            self.cancel.child_token(),
        )?;
        let connection_cancel = self
            .runtime
            .connections
            .call_binding(&tool.target, &self.documents)?
            .cancel;
        let review = self.runtime.prepare_tool_call(
            &self.documents,
            &context,
            tool.target.clone(),
            arguments,
        )?;
        let id = review.request_id;
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().map_err(|_| RuntimeError::Busy)?.insert(
            id,
            Decision {
                review,
                context: context.clone(),
                sender,
            },
        );
        let _pending_guard = PendingReviewGuard {
            turn: self,
            id,
            cancel: context.cancel.clone(),
        };
        let workspace_cancel = self.documents.cancellation();
        let decision = tokio::select! {
            biased;
            _ = self.cancel.cancelled() => Err(RuntimeError::Cancelled),
            _ = workspace_cancel.cancelled() => Err(RuntimeError::WorkspaceChanged),
            _ = connection_cancel.cancelled() => Err(RuntimeError::ConnectionNotReady),
            result = tokio::time::timeout(Duration::from_secs(60), receiver) => result.map_err(|_| RuntimeError::ApprovalExpired).and_then(|result| result.map_err(|_| RuntimeError::Cancelled)),
        };
        self.pending
            .lock()
            .map_err(|_| RuntimeError::Busy)?
            .remove(&id);
        decision?;
        let result = self
            .runtime
            .execute_tool_call(self.documents.clone(), &context, id)
            .await;
        // Do not let the next model round retry an uncertain operation under a
        // fresh call ID. A new user turn is required before any more MCP work.
        if result
            .as_ref()
            .err()
            .is_some_and(|error| error.outcome == CallOutcome::Unknown)
        {
            self.block("uncertain");
        }
        result
    }
}
impl Drop for NativeMcpTurn {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
