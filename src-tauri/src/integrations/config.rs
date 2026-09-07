use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

pub const SCHEMA_VERSION: u32 = 1;
pub const MAX_CONFIG_BYTES: usize = 1024 * 1024;

/// Stable codes are translated by the desktop UI. Never include configuration
/// values in errors: arguments, URLs and environment values can be sensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigError {
    UnsupportedVersion,
    InvalidConfig,
    InvalidId,
    DuplicateId,
    InvalidName,
    InvalidCommand,
    InvalidArguments,
    InvalidDirectory,
    InvalidEnvironment,
    InvalidUrl,
    InvalidHeaders,
    InvalidReference,
    TooLarge,
    ReadFailed,
    WriteFailed,
    Conflict,
    DesktopOnly,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntegrationConfig {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub entries: Vec<IntegrationDefinition>,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for IntegrationConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct IntegrationDefinition {
    pub id: Uuid,
    pub name: String,
    /// Desired policy only. Effective use also requires workspace policy, trust,
    /// and a connected protocol runtime. New definitions are disabled by default.
    #[serde(default)]
    pub enabled: bool,
    pub connection: ConnectionConfig,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "protocol", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectionConfig {
    Mcp {
        transport: McpTransport,
    },
    Acp {
        process: ProcessConfig,
        #[serde(default)]
        mcp_servers: Vec<Uuid>,
    },
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum McpTransport {
    Stdio {
        process: ProcessConfig,
    },
    StreamableHttp {
        url: String,
        #[serde(default)]
        headers: BTreeMap<String, ConfigValue>,
    },
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ProcessConfig {
    /// Executable and arguments are separate; never interpolate a shell command.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// None resolves to the owning workspace when a runtime is started.
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, ConfigValue>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfigValue {
    Text {
        value: String,
    },
    /// A name scoped to this integration's keyring namespace, never a secret value.
    Secret {
        name: String,
    },
}

/// Workspace policy can only disable a global definition; it cannot authorize
/// execution or replace executable configuration from a checked-out repository.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceIntegrationSettings {
    #[serde(default)]
    pub disabled_ids: Vec<Uuid>,
}

impl WorkspaceIntegrationSettings {
    pub fn allows(&self, definition: &IntegrationDefinition) -> bool {
        definition.enabled && !self.disabled_ids.contains(&definition.id)
    }
}

fn safe_text(value: &str, max: usize) -> bool {
    value.len() <= max && !value.contains('\0')
}

fn valid_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-' | b'.'))
}

impl ConfigValue {
    fn validate(&self) -> bool {
        match self {
            Self::Text { value } => safe_text(value, 8192),
            Self::Secret { name } => valid_key(name),
        }
    }
}

impl ProcessConfig {
    fn validate(&self) -> Result<(), ConfigError> {
        if self.command.trim().is_empty()
            || !safe_text(&self.command, 4096)
            || self.command.contains(['\r', '\n'])
        {
            return Err(ConfigError::InvalidCommand);
        }
        if self.args.len() > 256 || self.args.iter().any(|arg| !safe_text(arg, 8192)) {
            return Err(ConfigError::InvalidArguments);
        }
        if self
            .cwd
            .as_ref()
            .is_some_and(|cwd| cwd.trim().is_empty() || !safe_text(cwd, 4096))
        {
            return Err(ConfigError::InvalidDirectory);
        }
        if self.env.len() > 128
            || self
                .env
                .iter()
                .any(|(key, value)| !valid_key(key) || !value.validate())
        {
            return Err(ConfigError::InvalidEnvironment);
        }
        Ok(())
    }
}

impl IntegrationConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(ConfigError::UnsupportedVersion);
        }
        if self.entries.len() > 128 {
            return Err(ConfigError::TooLarge);
        }
        let mut ids = HashSet::new();
        for entry in &self.entries {
            if entry.id.is_nil() {
                return Err(ConfigError::InvalidId);
            }
            if !ids.insert(entry.id) {
                return Err(ConfigError::DuplicateId);
            }
            if entry.name.trim().is_empty()
                || entry.name.len() > 128
                || entry.name.chars().any(char::is_control)
            {
                return Err(ConfigError::InvalidName);
            }
            match &entry.connection {
                ConnectionConfig::Mcp {
                    transport: McpTransport::Stdio { process },
                }
                | ConnectionConfig::Acp { process, .. } => process.validate()?,
                ConnectionConfig::Mcp {
                    transport: McpTransport::StreamableHttp { url, headers },
                } => {
                    let parsed = reqwest::Url::parse(url).map_err(|_| ConfigError::InvalidUrl)?;
                    if url.len() > 8192
                        || !matches!(parsed.scheme(), "http" | "https")
                        || parsed.host_str().is_none()
                        || !parsed.username().is_empty()
                        || parsed.password().is_some()
                        || parsed.fragment().is_some()
                        || url.chars().any(char::is_control)
                    {
                        return Err(ConfigError::InvalidUrl);
                    }
                    if headers.len() > 64 {
                        return Err(ConfigError::InvalidHeaders);
                    }
                    let mut names = HashSet::new();
                    for (name, value) in headers {
                        if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err()
                            || !names.insert(name.to_ascii_lowercase())
                            || !value.validate()
                            || matches!(value, ConfigValue::Text { value } if reqwest::header::HeaderValue::from_str(value).is_err())
                        {
                            return Err(ConfigError::InvalidHeaders);
                        }
                        // Authentication headers must reference protected storage.
                        if matches!(
                            name.to_ascii_lowercase().as_str(),
                            "authorization" | "proxy-authorization" | "cookie"
                        ) && matches!(value, ConfigValue::Text { .. })
                        {
                            return Err(ConfigError::InvalidHeaders);
                        }
                    }
                }
            }
        }
        for entry in &self.entries {
            if let ConnectionConfig::Acp { mcp_servers, .. } = &entry.connection {
                let mut forwarded = HashSet::new();
                for id in mcp_servers {
                    if !forwarded.insert(id)
                        || !self.entries.iter().any(|candidate| {
                            candidate.id == *id
                                && matches!(candidate.connection, ConnectionConfig::Mcp { .. })
                        })
                    {
                        return Err(ConfigError::InvalidReference);
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn fixture() -> IntegrationConfig {
        serde_json::from_value(json!({"entries":[{"id":Uuid::new_v4(),"name":"Atlas Scout",
            "connection":{"protocol":"mcp","transport":{"type":"stdio","process":{
                "command":"atlas-scout","args":["mcp"]}}}}]}))
        .unwrap()
    }

    #[test]
    fn old_or_empty_configuration_defaults_to_no_automatic_launch() {
        let empty: IntegrationConfig = serde_json::from_str("{}").unwrap();
        assert!(empty.entries.is_empty());
        let config = fixture();
        assert!(!config.entries[0].enabled);
        assert!(config.validate().is_ok());
        assert!(config == serde_json::from_slice(&serde_json::to_vec(&config).unwrap()).unwrap());
    }

    #[test]
    fn workspace_disable_wins_and_cannot_enable_a_global_entry() {
        let mut entry = fixture().entries.remove(0);
        assert!(!WorkspaceIntegrationSettings::default().allows(&entry));
        entry.enabled = true;
        assert!(WorkspaceIntegrationSettings::default().allows(&entry));
        assert!(!WorkspaceIntegrationSettings {
            disabled_ids: vec![entry.id]
        }
        .allows(&entry));
    }

    #[test]
    fn duplicate_ids_and_unknown_versions_do_not_silently_replace_configuration() {
        let mut config = fixture();
        config.entries.push(config.entries[0].clone());
        assert_eq!(config.validate(), Err(ConfigError::DuplicateId));
        config.schema_version += 1;
        assert_eq!(config.validate(), Err(ConfigError::UnsupportedVersion));
        assert!(serde_json::from_value::<IntegrationConfig>(json!({"unexpected":true})).is_err());
    }

    #[test]
    fn arguments_remain_individual_values_and_invalid_process_data_is_rejected() {
        let mut config = fixture();
        let ConnectionConfig::Mcp {
            transport: McpTransport::Stdio { process },
        } = &mut config.entries[0].connection
        else {
            panic!()
        };
        process.args = vec![
            "path with spaces".into(),
            "$(no-shell-expansion)".into(),
            "".into(),
        ];
        assert!(config.validate().is_ok());
        let ConnectionConfig::Mcp {
            transport: McpTransport::Stdio { process },
        } = &mut config.entries[0].connection
        else {
            panic!()
        };
        process.command = "bad\0command".into();
        assert_eq!(config.validate(), Err(ConfigError::InvalidCommand));
    }

    #[test]
    fn http_validates_transport_and_keeps_authentication_out_of_plaintext_headers() {
        let mut config = fixture();
        for url in [
            "file:///tmp/socket",
            "https://user:password@example.com/mcp",
            "https://example.com/#fragment",
        ] {
            config.entries[0].connection = ConnectionConfig::Mcp {
                transport: McpTransport::StreamableHttp {
                    url: url.into(),
                    headers: BTreeMap::new(),
                },
            };
            assert_eq!(config.validate(), Err(ConfigError::InvalidUrl));
        }
        config.entries[0].connection = ConnectionConfig::Mcp {
            transport: McpTransport::StreamableHttp {
                url: "http://localhost:8080/mcp".into(),
                headers: BTreeMap::from([(
                    "Authorization".into(),
                    ConfigValue::Text {
                        value: "Bearer sensitive".into(),
                    },
                )]),
            },
        };
        assert_eq!(config.validate(), Err(ConfigError::InvalidHeaders));
        let ConnectionConfig::Mcp {
            transport: McpTransport::StreamableHttp { headers, .. },
        } = &mut config.entries[0].connection
        else {
            panic!()
        };
        headers.insert(
            "Authorization".into(),
            ConfigValue::Secret {
                name: "access-token".into(),
            },
        );
        assert!(config.validate().is_ok());
    }

    #[test]
    fn acp_forwarding_requires_existing_mcp_ids_and_survives_display_renames() {
        let mut config = fixture();
        let mcp_id = config.entries[0].id;
        config.entries.push(IntegrationDefinition {
            id: Uuid::new_v4(),
            name: "Agent".into(),
            enabled: false,
            connection: ConnectionConfig::Acp {
                process: ProcessConfig {
                    command: "agent".into(),
                    args: vec![],
                    cwd: None,
                    env: BTreeMap::new(),
                },
                mcp_servers: vec![mcp_id],
            },
        });
        config.entries[0].name = "Renamed".into();
        assert!(config.validate().is_ok());
        config.entries.remove(0);
        assert_eq!(config.validate(), Err(ConfigError::InvalidReference));
    }
}
