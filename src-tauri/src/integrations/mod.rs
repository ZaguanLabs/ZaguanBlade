//! Integration configuration, identities and supervised protocol lifetimes.
//! Saving configuration never starts a process or grants launch permission.

pub(crate) mod call_result;
pub mod catalog;
pub mod config;
pub mod connections;
pub mod credentials;
pub mod identity;
pub(crate) mod invocation;
pub mod mcp_session;
pub(crate) mod permissions;
pub mod probe;
pub mod process;
pub mod runtime;
pub mod store;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeError {
    DesktopOnly,
    ConfigChanged,
    IntegrationDisabled,
    PolicyUnavailable,
    ConnectionNotReady,
    WorkspaceChanged,
    UnsupportedTransport,
    ExecutableNotFound,
    InvalidDirectory,
    SecretUnavailable,
    SecretMissing,
    InvalidSecret,
    ApprovalExpired,
    Busy,
    LaunchFailed,
    ProtocolFailed,
    InvalidCatalog,
    CatalogChanged,
    UnsupportedVersion,
    OutputLimit,
    TimedOut,
    Cancelled,
    PermissionRequired,
    PermissionDenied,
    InvalidArguments,
    UnsupportedResult,
}
