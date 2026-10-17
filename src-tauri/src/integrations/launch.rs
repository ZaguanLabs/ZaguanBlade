//! A resolved MCP/ACP endpoint: a local process or a Streamable HTTP server.
//! Approvals bind to the fingerprint of everything resolved here, secrets included.
use super::{
    config::{ConnectionConfig, McpTransport},
    credentials::SecretStore,
    http::PreparedHttp,
    process::PreparedProcess,
    RuntimeError,
};
use std::{path::Path, sync::Arc};
use uuid::Uuid;

pub enum PreparedTransport {
    Process(PreparedProcess),
    Http(PreparedHttp),
}

impl PreparedTransport {
    pub fn resolve(
        connection: &ConnectionConfig,
        integration: Uuid,
        workspace: &Path,
        secrets: &Arc<dyn SecretStore>,
    ) -> Result<Self, RuntimeError> {
        Ok(match connection {
            ConnectionConfig::Mcp {
                transport: McpTransport::Stdio { process },
            }
            | ConnectionConfig::Acp { process, .. } => Self::Process(PreparedProcess::resolve(
                process,
                integration,
                workspace,
                secrets.as_ref(),
            )?),
            ConnectionConfig::Mcp {
                transport:
                    McpTransport::StreamableHttp {
                        url,
                        headers,
                        oauth,
                    },
            } => Self::Http(PreparedHttp::resolve(
                url,
                headers,
                oauth.as_ref(),
                integration,
                secrets,
            )?),
        })
    }

    pub fn fingerprint(&self) -> &str {
        match self {
            Self::Process(process) => &process.fingerprint,
            Self::Http(http) => &http.fingerprint,
        }
    }
}
