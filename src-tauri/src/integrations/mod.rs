//! Integration configuration, identities and supervised protocol probes.
//! Saving configuration never starts a process or grants launch permission.

pub mod catalog;
pub mod config;
pub mod connections;
pub mod mcp_session;
pub mod credentials;
pub mod identity;
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
}
