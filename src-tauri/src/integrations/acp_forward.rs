//! MCP servers an ACP agent receives in `session/new`. The agent starts and
//! calls them itself: Blade decides only what it hands over. A server Blade
//! cannot hand over faithfully is left out, with the reason.
use super::{
    config::{
        ConfigValue, ConnectionConfig, IntegrationConfig, IntegrationDefinition, McpTransport,
        WorkspaceIntegrationSettings,
    },
    credentials::SecretStore,
    http::PreparedHttp,
    process::PreparedProcess,
    store::fingerprint,
    RuntimeError,
};
use agent_client_protocol::schema::v1::{
    EnvVariable, HttpHeader, McpServer, McpServerHttp, McpServerStdio,
};
use serde::Serialize;
use std::{collections::HashSet, path::Path, sync::Arc};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SharedTransport {
    Stdio,
    Http,
}

/// A server the agent is given, with the names (never values) of the
/// secrets it receives along with it.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SharedServer {
    pub id: Uuid,
    pub name: String,
    pub transport: SharedTransport,
    pub secrets: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Switched off, globally or for this workspace.
    Disabled,
    /// A working directory other than the workspace; the standard stdio
    /// declaration has none, so the agent would start it elsewhere.
    Directory,
    /// Uses the user's OAuth sign-in, which is not handed to other programs.
    #[serde(rename = "oauth")]
    OAuth,
    /// Plain HTTP to another machine.
    Insecure,
    /// The agent did not advertise this transport.
    Transport,
    /// Could not be resolved (missing program or secret, for example).
    Unavailable,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SkippedServer {
    pub id: Uuid,
    pub name: String,
    pub reason: SkipReason,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RuntimeError>,
}

/// What the launch review and the agent view show.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct ForwardingView {
    pub shared: Vec<SharedServer>,
    pub skipped: Vec<SkippedServer>,
}

pub struct Forwarding {
    pub servers: Vec<McpServer>,
    pub view: ForwardingView,
    /// Binds approval to every forwarded launch, secret values included.
    pub fingerprint: String,
}

impl Forwarding {
    pub fn none() -> Self {
        plan_from(
            &[],
            &|_| None,
            &WorkspaceIntegrationSettings::default(),
            |_| unreachable!(),
        )
    }

    /// Moves servers whose transport the agent did not advertise to
    /// `skipped`, as `session/new` leaves them out.
    pub fn view_for(&self, supported: impl Fn(&McpServer) -> bool) -> ForwardingView {
        let mut view = self.view.clone();
        let mut kept = Vec::new();
        for (shared, server) in view.shared.drain(..).zip(&self.servers) {
            if supported(server) {
                kept.push(shared);
            } else {
                view.skipped.push(SkippedServer {
                    id: shared.id,
                    name: shared.name,
                    reason: SkipReason::Transport,
                    error: None,
                });
            }
        }
        view.shared = kept;
        view
    }
}

/// The servers selected for `agent`, in the order selected.
pub fn plan(
    config: &IntegrationConfig,
    agent: &IntegrationDefinition,
    workspace: &WorkspaceIntegrationSettings,
    root: &Path,
    secrets: &Arc<dyn SecretStore>,
) -> Forwarding {
    let ConnectionConfig::Acp { mcp_servers, .. } = &agent.connection else {
        return Forwarding::none();
    };
    let find = |id: Uuid| config.entries.iter().find(|entry| entry.id == id);
    plan_from(mcp_servers, &find, workspace, |entry| {
        resolve(entry, root, secrets)
    })
}

type Resolved = Result<(McpServer, Vec<String>, String), (SkipReason, Option<RuntimeError>)>;

fn plan_from<'a>(
    ids: &[Uuid],
    find: &dyn Fn(Uuid) -> Option<&'a IntegrationDefinition>,
    workspace: &WorkspaceIntegrationSettings,
    resolve: impl Fn(&IntegrationDefinition) -> Resolved,
) -> Forwarding {
    let mut forwarding = Forwarding {
        servers: Vec::new(),
        view: ForwardingView::default(),
        fingerprint: String::new(),
    };
    let mut bytes = Vec::new();
    let mut names = HashSet::new();
    for &id in ids {
        let Some(entry) = find(id) else {
            continue;
        };
        let resolved = if workspace.allows(entry) {
            resolve(entry)
        } else {
            Err((SkipReason::Disabled, None))
        };
        match resolved {
            Ok((server, secrets, launch)) => {
                // Agents name tools after the server; keep names distinct.
                let mut name = entry.name.clone();
                let mut n = 2;
                while !names.insert(name.clone()) {
                    name = format!("{} ({n})", entry.name);
                    n += 1;
                }
                let server = rename(server, &name);
                bytes.extend_from_slice(id.as_bytes());
                bytes.extend_from_slice(launch.as_bytes());
                forwarding.servers.push(server);
                forwarding.view.shared.push(SharedServer {
                    id,
                    name,
                    transport: match entry.connection {
                        ConnectionConfig::Mcp {
                            transport: McpTransport::StreamableHttp { .. },
                        } => SharedTransport::Http,
                        _ => SharedTransport::Stdio,
                    },
                    secrets,
                });
            }
            Err((reason, error)) => {
                bytes.extend_from_slice(id.as_bytes());
                bytes.push(reason as u8);
                forwarding.view.skipped.push(SkippedServer {
                    id,
                    name: entry.name.clone(),
                    reason,
                    error,
                });
            }
        }
    }
    forwarding.fingerprint = fingerprint(&bytes);
    forwarding
}

fn rename(server: McpServer, name: &str) -> McpServer {
    match server {
        McpServer::Stdio(mut stdio) => {
            stdio.name = name.into();
            McpServer::Stdio(stdio)
        }
        McpServer::Http(mut http) => {
            http.name = name.into();
            McpServer::Http(http)
        }
        other => other,
    }
}

fn secret_names<'a>(values: impl Iterator<Item = (&'a String, &'a ConfigValue)>) -> Vec<String> {
    values
        .filter(|(_, value)| matches!(value, ConfigValue::Secret { .. }))
        .map(|(name, _)| name.clone())
        .collect()
}

fn resolve(entry: &IntegrationDefinition, root: &Path, secrets: &Arc<dyn SecretStore>) -> Resolved {
    let unavailable = |error| (SkipReason::Unavailable, Some(error));
    match &entry.connection {
        ConnectionConfig::Mcp {
            transport: McpTransport::Stdio { process },
        } => {
            let prepared = PreparedProcess::resolve(process, entry.id, root, secrets.as_ref())
                .map_err(unavailable)?;
            let workspace = root.canonicalize().map_err(|_| {
                (
                    SkipReason::Unavailable,
                    Some(RuntimeError::WorkspaceChanged),
                )
            })?;
            if prepared.cwd != workspace {
                return Err((SkipReason::Directory, None));
            }
            let env = prepared
                .configured_env
                .iter()
                .map(|(name, value)| EnvVariable::new(name, value))
                .collect();
            let server = McpServerStdio::new(&entry.name, &prepared.executable)
                .args(prepared.args.clone())
                .env(env);
            Ok((
                McpServer::Stdio(server),
                secret_names(process.env.iter()),
                prepared.fingerprint,
            ))
        }
        ConnectionConfig::Mcp {
            transport:
                McpTransport::StreamableHttp {
                    url,
                    headers,
                    oauth,
                },
        } => {
            if oauth.is_some() {
                return Err((SkipReason::OAuth, None));
            }
            let prepared = PreparedHttp::resolve(url, headers, None, entry.id, secrets)
                .map_err(unavailable)?;
            if prepared.insecure {
                return Err((SkipReason::Insecure, None));
            }
            let headers = prepared
                .header_pairs()
                .into_iter()
                .map(|(name, value)| HttpHeader::new(name, value))
                .collect();
            Ok((
                McpServer::Http(McpServerHttp::new(&entry.name, url).headers(headers)),
                secret_names(headers_of(entry)),
                prepared.fingerprint,
            ))
        }
        ConnectionConfig::Acp { .. } => Err((SkipReason::Unavailable, None)),
    }
}

fn headers_of(entry: &IntegrationDefinition) -> impl Iterator<Item = (&String, &ConfigValue)> {
    match &entry.connection {
        ConnectionConfig::Mcp {
            transport: McpTransport::StreamableHttp { headers, .. },
        } => Some(headers.iter()),
        _ => None,
    }
    .into_iter()
    .flatten()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::integrations::config::{OAuthSettings, ProcessConfig};
    use std::{collections::BTreeMap, sync::Mutex};

    #[derive(Default)]
    struct Secrets(Mutex<BTreeMap<String, String>>);
    impl SecretStore for Secrets {
        fn get(&self, _: Uuid, name: &str) -> Result<Option<String>, RuntimeError> {
            Ok(self.0.lock().unwrap().get(name).cloned())
        }
    }
    impl Secrets {
        fn put(&self, name: &str, value: &str) {
            self.0.lock().unwrap().insert(name.into(), value.into());
        }
    }

    fn stdio(
        name: &str,
        cwd: Option<&str>,
        env: BTreeMap<String, ConfigValue>,
    ) -> IntegrationDefinition {
        IntegrationDefinition {
            id: Uuid::new_v4(),
            name: name.into(),
            enabled: true,
            connection: ConnectionConfig::Mcp {
                transport: McpTransport::Stdio {
                    process: ProcessConfig {
                        command: "sh".into(),
                        args: vec!["-c".into(), "true".into()],
                        cwd: cwd.map(Into::into),
                        env,
                    },
                },
            },
        }
    }
    fn http(name: &str, url: &str, oauth: bool) -> IntegrationDefinition {
        IntegrationDefinition {
            id: Uuid::new_v4(),
            name: name.into(),
            enabled: true,
            connection: ConnectionConfig::Mcp {
                transport: McpTransport::StreamableHttp {
                    url: url.into(),
                    headers: BTreeMap::from([(
                        "X-Token".into(),
                        ConfigValue::Secret {
                            name: "token".into(),
                        },
                    )]),
                    oauth: oauth.then(OAuthSettings::default),
                },
            },
        }
    }

    #[test]
    fn forwards_what_it_can_and_names_what_it_cannot() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("sub")).unwrap();
        let store = Arc::new(Secrets::default());
        store.put("key", "s3cret");
        store.put("token", "t0ken");
        let secrets: Arc<dyn SecretStore> = store.clone();
        let env = BTreeMap::from([
            (
                "API_KEY".to_string(),
                ConfigValue::Secret { name: "key".into() },
            ),
            (
                "MODE".to_string(),
                ConfigValue::Text {
                    value: "fast".into(),
                },
            ),
        ]);
        let lantern = stdio("Lantern", None, env);
        let twin = stdio("Lantern", Some("."), BTreeMap::new());
        let elsewhere = stdio("Elsewhere", Some("sub"), BTreeMap::new());
        let mut off = stdio("Off", None, BTreeMap::new());
        off.enabled = false;
        let hidden = stdio("Hidden", None, BTreeMap::new());
        let missing = stdio(
            "Missing",
            None,
            BTreeMap::from([(
                "K".to_string(),
                ConfigValue::Secret {
                    name: "absent".into(),
                },
            )]),
        );
        let remote = http("Remote", "https://mcp.example.com/mcp", false);
        let signed_in = http("Signed in", "https://mcp.example.com/mcp", true);
        let plain = http("Plain", "http://mcp.example.com/mcp", false);
        let servers = [
            &lantern, &twin, &elsewhere, &off, &hidden, &missing, &remote, &signed_in, &plain,
        ];
        let agent = IntegrationDefinition {
            id: Uuid::new_v4(),
            name: "Agent".into(),
            enabled: true,
            connection: ConnectionConfig::Acp {
                process: ProcessConfig {
                    command: "sh".into(),
                    args: vec![],
                    cwd: None,
                    env: BTreeMap::new(),
                },
                mcp_servers: servers.iter().map(|server| server.id).collect(),
            },
        };
        let config = IntegrationConfig {
            entries: servers
                .iter()
                .map(|server| (*server).clone())
                .chain([agent.clone()])
                .collect(),
            ..Default::default()
        };
        let workspace = WorkspaceIntegrationSettings {
            disabled_ids: vec![hidden.id],
            ..Default::default()
        };
        let forwarding = plan(&config, &agent, &workspace, root.path(), &secrets);
        let shared: Vec<_> = forwarding
            .view
            .shared
            .iter()
            .map(|server| {
                (
                    server.name.as_str(),
                    server.transport,
                    server.secrets.clone(),
                )
            })
            .collect();
        assert_eq!(
            shared,
            [
                (
                    "Lantern",
                    SharedTransport::Stdio,
                    vec!["API_KEY".to_string()]
                ),
                ("Lantern (2)", SharedTransport::Stdio, vec![]),
                ("Remote", SharedTransport::Http, vec!["X-Token".to_string()]),
            ]
        );
        let skipped: Vec<_> = forwarding
            .view
            .skipped
            .iter()
            .map(|server| (server.name.as_str(), server.reason, server.error))
            .collect();
        assert_eq!(
            skipped,
            [
                ("Elsewhere", SkipReason::Directory, None),
                ("Off", SkipReason::Disabled, None),
                ("Hidden", SkipReason::Disabled, None),
                (
                    "Missing",
                    SkipReason::Unavailable,
                    Some(RuntimeError::SecretMissing)
                ),
                ("Signed in", SkipReason::OAuth, None),
                ("Plain", SkipReason::Insecure, None),
            ]
        );
        let McpServer::Stdio(first) = &forwarding.servers[0] else {
            panic!("stdio expected");
        };
        assert!(first.command.is_absolute());
        assert_eq!(first.args, ["-c", "true"]);
        let env: Vec<_> = first
            .env
            .iter()
            .map(|var| (var.name.as_str(), var.value.as_str()))
            .collect();
        assert_eq!(env, [("API_KEY", "s3cret"), ("MODE", "fast")]);
        let McpServer::Http(remote) = &forwarding.servers[2] else {
            panic!("http expected");
        };
        assert_eq!(remote.headers[0].name, "x-token");
        assert_eq!(remote.headers[0].value, "t0ken");

        // Secret values are part of the approval; their change voids it.
        let before = forwarding.fingerprint.clone();
        store.put("key", "changed");
        assert_ne!(
            plan(&config, &agent, &workspace, root.path(), &secrets).fingerprint,
            before
        );

        // An agent without HTTP support gets the stdio servers only.
        let view = forwarding.view_for(|server| matches!(server, McpServer::Stdio(_)));
        assert_eq!(view.shared.len(), 2);
        assert_eq!(view.skipped.last().unwrap().reason, SkipReason::Transport);
        assert_eq!(view.skipped.last().unwrap().name, "Remote");
    }
}
