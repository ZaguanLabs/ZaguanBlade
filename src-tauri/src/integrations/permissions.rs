//! Request-scoped MCP consent. Launch consent and server annotations grant no
//! invocation rights. Only a future trusted conversation controller may decide.
use super::{identity::WorkspaceIdentity, RuntimeError};
use crate::document_service::DocumentService;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_PENDING: usize = 64;
const APPROVAL_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallScope {
    pub workspace: WorkspaceIdentity,
    pub conversation_id: Uuid,
    pub turn_id: Uuid,
    pub call_id: String,
}

/// Constructed by the conversation owner, never deserialized from model input.
#[derive(Clone)]
pub struct CallContext {
    pub scope: CallScope,
    pub(super) cancel: CancellationToken,
    pub(super) workspace_cancel: CancellationToken,
}
impl CallContext {
    pub fn new(
        documents: &DocumentService,
        conversation_id: Uuid,
        turn_id: Uuid,
        call_id: String,
        cancel: CancellationToken,
    ) -> Result<Self, RuntimeError> {
        if call_id.is_empty() || call_id.len() > 256 || call_id.chars().any(char::is_control) {
            return Err(RuntimeError::InvalidArguments);
        }
        Ok(Self {
            scope: CallScope {
                workspace: documents.identity().clone(),
                conversation_id,
                turn_id,
                call_id,
            },
            cancel,
            workspace_cancel: documents.cancellation(),
        })
    }
    pub(super) fn check(&self) -> Result<(), RuntimeError> {
        if self.workspace_cancel.is_cancelled() {
            return Err(RuntimeError::WorkspaceChanged);
        }
        if self.cancel.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CallTarget {
    pub connection_id: Uuid,
    pub alias: String,
    pub catalog_revision: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallReview {
    pub request_id: Uuid,
    pub scope: CallScope,
    pub target: CallTarget,
    pub integration_id: Uuid,
    pub server_name: String,
    pub tool_name: String,
    pub arguments: Value,
}

struct Pending {
    review: CallReview,
    context: CallContext,
    connection_cancel: CancellationToken,
    expires: Instant,
    decision: Option<bool>,
}
impl Pending {
    fn check(&self) -> Result<(), RuntimeError> {
        self.context.check()?;
        if self.connection_cancel.is_cancelled() {
            return Err(RuntimeError::ConnectionNotReady);
        }
        if self.expires <= Instant::now() {
            return Err(RuntimeError::ApprovalExpired);
        }
        Ok(())
    }
}

/// Fields are private: a caller cannot manufacture or alter an approved call.
pub(super) struct ApprovedCall {
    pending: Pending,
}
impl ApprovedCall {
    pub fn review(&self) -> &CallReview {
        &self.pending.review
    }
    pub fn context(&self) -> &CallContext {
        &self.pending.context
    }
    pub fn check(&self) -> Result<(), RuntimeError> {
        self.pending.check()
    }
}

#[derive(Default)]
pub(super) struct PermissionRegistry {
    pending: Mutex<HashMap<Uuid, Pending>>,
}
impl PermissionRegistry {
    pub fn request(
        &self,
        review: CallReview,
        context: CallContext,
        connection_cancel: CancellationToken,
    ) -> Result<CallReview, RuntimeError> {
        let entry = Pending {
            review,
            context,
            connection_cancel,
            expires: Instant::now() + APPROVAL_TTL,
            decision: None,
        };
        entry.check()?;
        let mut pending = self.pending.lock().map_err(|_| RuntimeError::Busy)?;
        pending.retain(|_, entry| entry.check().is_ok());
        if pending.len() >= MAX_PENDING
            || pending
                .values()
                .any(|old| old.review.scope == entry.review.scope)
        {
            return Err(RuntimeError::Busy);
        }
        let review = entry.review.clone();
        pending.insert(review.request_id, entry);
        Ok(review)
    }

    pub fn decide(&self, context: &CallContext, id: Uuid, allow: bool) -> Result<(), RuntimeError> {
        context.check()?;
        let mut pending = self.pending.lock().map_err(|_| RuntimeError::Busy)?;
        let entry = pending.get_mut(&id).ok_or(RuntimeError::ApprovalExpired)?;
        if entry.review.scope != context.scope {
            return Err(RuntimeError::PermissionDenied);
        }
        entry.check()?;
        // A late/duplicate decision cannot change a previous rejection or allow.
        if entry.decision.is_some() {
            return Err(RuntimeError::ApprovalExpired);
        }
        entry.decision = Some(allow);
        Ok(())
    }

    pub fn take(&self, context: &CallContext, id: Uuid) -> Result<ApprovedCall, RuntimeError> {
        context.check()?;
        let mut pending = self.pending.lock().map_err(|_| RuntimeError::Busy)?;
        let entry = pending.get(&id).ok_or(RuntimeError::ApprovalExpired)?;
        if entry.review.scope != context.scope {
            return Err(RuntimeError::PermissionDenied);
        }
        entry.check()?;
        if entry.decision.is_none() {
            return Err(RuntimeError::PermissionRequired);
        }
        let entry = pending.remove(&id).ok_or(RuntimeError::ApprovalExpired)?;
        if entry.decision != Some(true) {
            return Err(RuntimeError::PermissionDenied);
        }
        Ok(ApprovedCall { pending: entry })
    }

    pub fn clear(&self) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(
        registry: &PermissionRegistry,
        context: &CallContext,
        connection: &CancellationToken,
    ) -> CallReview {
        registry
            .request(
                CallReview {
                    request_id: Uuid::new_v4(),
                    scope: context.scope.clone(),
                    target: CallTarget {
                        connection_id: Uuid::new_v4(),
                        alias: "mcp_alias".into(),
                        catalog_revision: "revision".into(),
                    },
                    integration_id: Uuid::new_v4(),
                    server_name: "server".into(),
                    tool_name: "search".into(),
                    arguments: serde_json::json!({}),
                },
                context.clone(),
                connection.clone(),
            )
            .unwrap()
    }
    #[test]
    fn expiry_disconnect_and_decisions_are_isolated_between_conversations() {
        let root = tempfile::tempdir().unwrap();
        let documents = DocumentService::new(root.path().into());
        let registry = PermissionRegistry::default();
        let first = CallContext::new(
            &documents,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "same-call-id".into(),
            CancellationToken::new(),
        )
        .unwrap();
        let second = CallContext::new(
            &documents,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "same-call-id".into(),
            CancellationToken::new(),
        )
        .unwrap();
        let connection = CancellationToken::new();
        let a = request(&registry, &first, &connection);
        let b = request(&registry, &second, &connection);
        assert_eq!(
            registry.decide(&second, a.request_id, true),
            Err(RuntimeError::PermissionDenied)
        );
        registry.decide(&first, a.request_id, false).unwrap();
        assert!(matches!(
            registry.take(&first, a.request_id),
            Err(RuntimeError::PermissionDenied)
        ));
        registry.decide(&second, b.request_id, true).unwrap();
        registry
            .pending
            .lock()
            .unwrap()
            .get_mut(&b.request_id)
            .unwrap()
            .expires = Instant::now();
        assert!(matches!(
            registry.take(&second, b.request_id),
            Err(RuntimeError::ApprovalExpired)
        ));
        let b = request(&registry, &second, &connection);
        registry.decide(&second, b.request_id, true).unwrap();
        connection.cancel();
        assert!(matches!(
            registry.take(&second, b.request_id),
            Err(RuntimeError::ConnectionNotReady)
        ));
    }
    #[test]
    fn registry_bounds_pending_reviews_and_prunes_cancelled_owners() {
        let root = tempfile::tempdir().unwrap();
        let documents = DocumentService::new(root.path().into());
        let registry = PermissionRegistry::default();
        let owner = CancellationToken::new();
        let connection = CancellationToken::new();
        for _ in 0..MAX_PENDING {
            let context = CallContext::new(
                &documents,
                Uuid::new_v4(),
                Uuid::new_v4(),
                "call".into(),
                owner.clone(),
            )
            .unwrap();
            request(&registry, &context, &connection);
        }
        let pending = registry.pending.lock().unwrap();
        assert_eq!(pending.len(), MAX_PENDING);
        let review = pending.values().next().unwrap().review.clone();
        let context = pending.values().next().unwrap().context.clone();
        drop(pending);
        assert!(matches!(
            registry.request(review, context, connection.clone()),
            Err(RuntimeError::Busy)
        ));
        owner.cancel();
        let context = CallContext::new(
            &documents,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "call".into(),
            CancellationToken::new(),
        )
        .unwrap();
        request(&registry, &context, &connection);
        assert_eq!(registry.pending.lock().unwrap().len(), 1);
        documents.retire();
        assert!(matches!(
            registry.take(&context, Uuid::new_v4()),
            Err(RuntimeError::WorkspaceChanged)
        ));
    }
}
