//! Internal invocation boundary for the forthcoming native conversation adapter.
//! No Tauri command, model tool, or remote intent exposes permission decisions.
use super::{
    call_result::{bounded_json, CallFailure, McpCallResult, MAX_ARGUMENT_BYTES},
    config::{ConnectionConfig, McpTransport},
    connections::read_policy,
    permissions::{CallContext, CallReview, CallTarget},
    process::PreparedProcess,
    runtime::IntegrationRuntime,
    RuntimeError,
};
use crate::document_service::DocumentService;
use serde_json::Value;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use uuid::Uuid;

impl IntegrationRuntime {
    pub(crate) fn prepare_tool_call(
        &self,
        documents: &DocumentService,
        context: &CallContext,
        target: CallTarget,
        arguments: Value,
    ) -> Result<CallReview, RuntimeError> {
        self.check_call_context(documents, context)?;
        if !arguments.is_object() {
            return Err(RuntimeError::InvalidArguments);
        }
        bounded_json(&arguments, MAX_ARGUMENT_BYTES)?;
        let binding = self.connections.call_binding(&target, documents)?;
        let review = CallReview {
            request_id: Uuid::new_v4(),
            scope: context.scope.clone(),
            target,
            integration_id: binding.definition.id,
            server_name: binding.definition.name,
            tool_name: binding.tool_name,
            arguments,
        };
        self.permissions
            .request(review, context.clone(), binding.cancel)
    }

    pub(crate) fn decide_tool_call(
        &self,
        context: &CallContext,
        request_id: Uuid,
        allow: bool,
    ) -> Result<(), RuntimeError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(RuntimeError::Cancelled);
        }
        self.permissions.decide(context, request_id, allow)
    }

    pub(crate) async fn execute_tool_call(
        self: &Arc<Self>,
        documents: Arc<DocumentService>,
        context: &CallContext,
        request_id: Uuid,
    ) -> Result<McpCallResult, CallFailure> {
        self.check_call_context(&documents, context)?;
        // Consume before any await. Failed dispatch never restores consent.
        let approved = self.permissions.take(context, request_id)?;
        let slot = self
            .invoking
            .clone()
            .try_acquire_owned()
            .map_err(|_| RuntimeError::Busy)?;
        let binding = self
            .connections
            .call_binding(&approved.review().target, &documents)?;
        let runtime = self.clone();
        let validation_documents = documents.clone();
        let validation = tokio::task::spawn_blocking(move || {
            // Keep the bound even if the async caller is dropped while a slow
            // filesystem/keyring operation is still running on this worker.
            let slot = slot;
            let current = read_policy(
                &runtime.directory,
                &validation_documents,
                binding.definition.id,
            )?;
            if current != binding.definition {
                return Err(RuntimeError::ConfigChanged);
            }
            let ConnectionConfig::Mcp {
                transport: McpTransport::Stdio { process },
            } = &current.connection
            else {
                return Err(RuntimeError::UnsupportedTransport);
            };
            let resolved = PreparedProcess::resolve(
                process,
                current.id,
                validation_documents.workspace_root(),
                runtime.secrets.as_ref(),
            )?;
            if resolved.fingerprint != binding.fingerprint {
                return Err(RuntimeError::ConfigChanged);
            }
            Ok(slot)
        });
        let _slot = tokio::select! {
            biased;
            _ = approved.context().workspace_cancel.cancelled() => return Err(RuntimeError::WorkspaceChanged.into()),
            _ = approved.context().cancel.cancelled() => return Err(RuntimeError::Cancelled.into()),
            result = tokio::time::timeout(Duration::from_secs(20), validation) => {
                result.map_err(|_| RuntimeError::TimedOut)?.map_err(|_| RuntimeError::PolicyUnavailable)??
            }
        };
        self.check_call_context(&documents, context)?;
        approved.check()?;
        self.connections.call(approved, &documents).await
    }

    fn check_call_context(
        &self,
        documents: &DocumentService,
        context: &CallContext,
    ) -> Result<(), RuntimeError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(RuntimeError::Cancelled);
        }
        context.check()?;
        if documents.identity() != &context.scope.workspace
            || documents.cancellation().is_cancelled()
        {
            return Err(RuntimeError::WorkspaceChanged);
        }
        Ok(())
    }
}
