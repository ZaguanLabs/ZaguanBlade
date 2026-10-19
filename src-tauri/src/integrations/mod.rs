//! Integration configuration, identities and supervised protocol lifetimes.
//! Saving configuration never starts a process or grants launch permission.

pub mod acp_config;
pub mod acp_files;
pub mod acp_terminals;
pub mod acp_forward;
pub mod acp_session;
pub mod agent_sessions;
pub(crate) mod call_result;
pub mod catalog;
pub mod config;
pub mod connections;
pub mod credentials;
pub mod elicitation;
pub mod http;
pub mod identity;
pub(crate) mod invocation;
pub mod launch;
pub mod launch_trust;
pub mod mcp_session;
pub(crate) mod native_catalog;
pub(crate) mod native_turn;
pub mod oauth;
pub(crate) mod permissions;
pub mod probe;
pub mod process;
pub mod prompts;
pub mod resources;
pub(crate) mod result_artifact;
pub mod runtime;
pub mod store;
pub mod uri_template;

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
    ArtifactUnavailable,
    CallLimit,
    AuthenticationRequired,
    ServerUnreachable,
    ResourceUnavailable,
    PromptUnavailable,
    #[serde(rename = "oauth_unsupported")]
    OAuthUnsupported,
    AuthorizationFailed,
    AuthorizationDenied,
    AgentExited,
}
