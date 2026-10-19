//! One conversation with a local ACP (Agent Client Protocol v1) agent. The
//! agent runs its own loop: Blade shows what it reports, answers its permission
//! requests with the exact option the user picked, and never executes a tool
//! the agent reports. Incoming handlers only queue work, so a pending UI
//! decision never holds the protocol reader.
use super::{
    acp_config::{ConfigChange, SessionConfig},
    acp_files::{FileError, WorkspaceFiles},
    acp_terminals::{TerminalView, Terminals},
    RuntimeError,
};
use agent_client_protocol::{
    schema::{
        v1::{
            AuthMethod, AuthenticateRequest, CancelNotification, ClientCapabilities, ContentBlock,
            CreateTerminalRequest, CreateTerminalResponse, EmbeddedResource,
            EmbeddedResourceResource, ErrorCode, FileSystemCapabilities, ImageContent,
            Implementation, InitializeRequest, InitializeResponse, KillTerminalRequest,
            KillTerminalResponse, LoadSessionRequest, McpServer, NewSessionRequest,
            NewSessionResponse, PermissionOptionKind, PlanEntryPriority, PlanEntryStatus,
            PromptRequest, PromptResponse, ReadTextFileRequest, ReadTextFileResponse,
            ReleaseTerminalRequest, ReleaseTerminalResponse, RequestPermissionOutcome,
            RequestPermissionRequest, RequestPermissionResponse, ResumeSessionRequest,
            SelectedPermissionOutcome, SessionConfigOption, SessionConfigOptionValue, SessionId,
            SessionNotification, SessionUpdate, SetSessionConfigOptionRequest,
            SetSessionModeRequest, StopReason, TerminalOutputRequest, TerminalOutputResponse,
            TextContent, TextResourceContents, ToolCallContent, ToolCallStatus, ToolKind,
            WaitForTerminalExitRequest, WaitForTerminalExitResponse, WriteTextFileRequest,
            WriteTextFileResponse,
        },
        ProtocolVersion,
    },
    Agent, ByteStreams, Client, ConnectionTo, Responder,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    path::PathBuf,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    sync::{mpsc, oneshot},
};
use tokio_util::{
    compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt},
    sync::CancellationToken,
};
use uuid::Uuid;

/// One JSON-RPC frame. Larger frames end the session instead of growing memory.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024;
const INCOMING_QUEUE: usize = 1024;
const COMMAND_QUEUE: usize = 16;
const MAX_PENDING_PERMISSIONS: usize = 8;
const MAX_PERMISSION_OPTIONS: usize = 16;
const MAX_TOOL_CALLS: usize = 512;
const MAX_TOOL_ITEMS: usize = 32;
const MAX_TOOL_TEXT: usize = 32 * 1024;
const MAX_TURN_TEXT: usize = 2 * 1024 * 1024;
const MAX_PLAN_ENTRIES: usize = 128;
pub(super) const MAX_LABEL: usize = 512;
pub(super) const MAX_DESCRIPTION: usize = 2048;
const MAX_PROMPT_BYTES: usize = 8 * 1024 * 1024;
const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(30);
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);
/// `session/load` replays the whole history first.
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
/// Agent-driven sign-in may wait on a browser.
const AUTH_TIMEOUT: Duration = Duration::from_secs(300);
/// After `session/cancel`, how long the agent has to end the turn.
#[cfg(not(test))]
const CANCEL_GRACE: Duration = Duration::from_secs(10);
#[cfg(test)]
const CANCEL_GRACE: Duration = Duration::from_millis(300);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentInfo {
    pub name: String,
    pub title: Option<String>,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthMethodInfo {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Needs an interactive terminal login, which Blade does not offer yet.
    pub terminal: bool,
}

/// What the agent declared in `initialize`, kept with the conversation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentSnapshot {
    pub protocol_version: u16,
    pub agent: Option<AgentInfo>,
    pub load_session: bool,
    pub resume_session: bool,
    pub close_session: bool,
    pub list_sessions: bool,
    pub logout: bool,
    pub prompt_image: bool,
    pub prompt_audio: bool,
    pub prompt_embedded_context: bool,
    pub mcp_http: bool,
    pub mcp_sse: bool,
    pub auth_methods: Vec<AuthMethodInfo>,
}

impl AgentSnapshot {
    fn from_response(response: &InitializeResponse) -> Self {
        let capabilities = &response.agent_capabilities;
        let auth_methods = response
            .auth_methods
            .iter()
            .filter_map(|method| match method {
                AuthMethod::Agent(method) => Some(AuthMethodInfo {
                    id: method.id.to_string(),
                    name: clip(&method.name, MAX_LABEL),
                    description: method
                        .description
                        .as_deref()
                        .map(|text| clip(text, MAX_DESCRIPTION)),
                    terminal: false,
                }),
                AuthMethod::Terminal(method) => Some(AuthMethodInfo {
                    id: method.id.to_string(),
                    name: clip(&method.name, MAX_LABEL),
                    description: method
                        .description
                        .as_deref()
                        .map(|text| clip(text, MAX_DESCRIPTION)),
                    terminal: true,
                }),
                _ => None,
            })
            .filter(|method| !method.id.is_empty() && method.id.len() <= MAX_LABEL)
            .take(16)
            .collect();
        Self {
            protocol_version: protocol_number(response.protocol_version),
            agent: response.agent_info.as_ref().map(|info| AgentInfo {
                name: clip(&info.name, MAX_LABEL),
                title: info.title.as_deref().map(|title| clip(title, MAX_LABEL)),
                version: clip(&info.version, MAX_LABEL),
            }),
            load_session: capabilities.load_session,
            resume_session: capabilities.session_capabilities.resume.is_some(),
            close_session: capabilities.session_capabilities.close.is_some(),
            list_sessions: capabilities.session_capabilities.list.is_some(),
            logout: capabilities.auth.logout.is_some(),
            prompt_image: capabilities.prompt_capabilities.image,
            prompt_audio: capabilities.prompt_capabilities.audio,
            prompt_embedded_context: capabilities.prompt_capabilities.embedded_context,
            mcp_http: capabilities.mcp_capabilities.http,
            mcp_sse: capabilities.mcp_capabilities.sse,
            auth_methods,
        }
    }
}

fn protocol_number(version: ProtocolVersion) -> u16 {
    version.to_string().parse().unwrap_or(0)
}

/// What the user sends in one turn.
#[derive(Clone, Debug)]
pub enum PromptPart {
    Text(String),
    Image {
        mime_type: String,
        data: String,
    },
    /// A text document the user attached; needs `embeddedContext`.
    Resource {
        uri: String,
        mime_type: Option<String>,
        text: String,
    },
}

fn prompt_blocks(
    parts: Vec<PromptPart>,
    snapshot: &AgentSnapshot,
) -> Result<Vec<ContentBlock>, RuntimeError> {
    let size: usize = parts
        .iter()
        .map(|part| match part {
            PromptPart::Text(text) => text.len(),
            PromptPart::Image { data, mime_type } => data.len() + mime_type.len(),
            PromptPart::Resource {
                uri,
                text,
                mime_type,
            } => uri.len() + text.len() + mime_type.as_ref().map_or(0, String::len),
        })
        .sum();
    if parts.is_empty() || parts.len() > 64 || size > MAX_PROMPT_BYTES {
        return Err(RuntimeError::InvalidArguments);
    }
    parts
        .into_iter()
        .map(|part| match part {
            PromptPart::Text(text) => Ok(ContentBlock::Text(TextContent::new(text))),
            PromptPart::Image { mime_type, data } if snapshot.prompt_image => {
                Ok(ContentBlock::Image(ImageContent::new(data, mime_type)))
            }
            PromptPart::Resource {
                uri,
                mime_type,
                text,
            } if snapshot.prompt_embedded_context => Ok(ContentBlock::Resource(
                EmbeddedResource::new(EmbeddedResourceResource::TextResourceContents(
                    TextResourceContents::new(text, uri).mime_type(mime_type),
                )),
            )),
            _ => Err(RuntimeError::UnsupportedResult),
        })
        .collect()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentView {
    Text { text: String },
    Link { uri: String, name: String },
    Resource { uri: String, text: Option<String> },
    Image { mime_type: String },
    Audio,
}

fn content_view(block: &ContentBlock, budget: usize) -> (ContentView, bool) {
    match block {
        ContentBlock::Text(text) => {
            let clipped = clip(&text.text, budget);
            let truncated = clipped.len() < text.text.len();
            (ContentView::Text { text: clipped }, truncated)
        }
        ContentBlock::ResourceLink(link) => (
            ContentView::Link {
                uri: clip(&link.uri, MAX_DESCRIPTION),
                name: clip(link.title.as_deref().unwrap_or(&link.name), MAX_LABEL),
            },
            false,
        ),
        ContentBlock::Resource(resource) => match &resource.resource {
            EmbeddedResourceResource::TextResourceContents(contents) => {
                let text = clip(&contents.text, budget);
                let truncated = text.len() < contents.text.len();
                (
                    ContentView::Resource {
                        uri: clip(&contents.uri, MAX_DESCRIPTION),
                        text: Some(text),
                    },
                    truncated,
                )
            }
            EmbeddedResourceResource::BlobResourceContents(contents) => (
                ContentView::Resource {
                    uri: clip(&contents.uri, MAX_DESCRIPTION),
                    text: None,
                },
                false,
            ),
            _ => (ContentView::Audio, false),
        },
        ContentBlock::Image(image) => (
            ContentView::Image {
                mime_type: clip(&image.mime_type, MAX_LABEL),
            },
            false,
        ),
        _ => (ContentView::Audio, false),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocationView {
    pub path: String,
    pub line: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContentView {
    Content {
        content: ContentView,
    },
    /// Shown for review only; Blade never applies it.
    Diff {
        path: String,
        old_text: Option<String>,
        new_text: String,
    },
    Terminal {
        terminal_id: String,
    },
}

/// The merged state of one tool call the agent reported. Reports are display
/// only: Blade runs nothing because of them.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolCallView {
    pub id: String,
    pub title: String,
    pub kind: ToolKind,
    pub status: ToolCallStatus,
    pub locations: Vec<LocationView>,
    pub content: Vec<ToolContentView>,
    pub truncated: bool,
}

impl ToolCallView {
    fn empty(id: String) -> Self {
        Self {
            id,
            title: String::new(),
            kind: ToolKind::Other,
            status: ToolCallStatus::Pending,
            locations: Vec::new(),
            content: Vec::new(),
            truncated: false,
        }
    }

    fn set_locations(&mut self, locations: &[agent_client_protocol::schema::v1::ToolCallLocation]) {
        self.locations = locations
            .iter()
            .take(MAX_TOOL_ITEMS)
            .map(|location| LocationView {
                path: clip(&location.path.to_string_lossy(), MAX_DESCRIPTION),
                line: location.line,
            })
            .collect();
    }

    fn set_content(&mut self, content: &[ToolCallContent]) {
        let mut budget = MAX_TOOL_TEXT;
        let mut truncated = content.len() > MAX_TOOL_ITEMS;
        self.content = content
            .iter()
            .take(MAX_TOOL_ITEMS)
            .map(|item| match item {
                ToolCallContent::Content(content) => {
                    let (view, cut) = content_view(&content.content, budget);
                    truncated |= cut;
                    if let ContentView::Text { text }
                    | ContentView::Resource {
                        text: Some(text), ..
                    } = &view
                    {
                        budget = budget.saturating_sub(text.len());
                    }
                    ToolContentView::Content { content: view }
                }
                ToolCallContent::Diff(diff) => {
                    let old_text = diff.old_text.as_deref().map(|text| clip(text, budget / 2));
                    let new_text = clip(&diff.new_text, budget / 2);
                    truncated |= new_text.len() < diff.new_text.len()
                        || old_text.as_ref().map(String::len)
                            < diff.old_text.as_ref().map(String::len);
                    budget = budget
                        .saturating_sub(new_text.len() + old_text.as_ref().map_or(0, String::len));
                    ToolContentView::Diff {
                        path: clip(&diff.path.to_string_lossy(), MAX_DESCRIPTION),
                        old_text,
                        new_text,
                    }
                }
                ToolCallContent::Terminal(terminal) => ToolContentView::Terminal {
                    terminal_id: clip(&terminal.terminal_id.to_string(), MAX_LABEL),
                },
                _ => ToolContentView::Content {
                    content: ContentView::Audio,
                },
            })
            .collect();
        self.truncated = truncated;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanEntryView {
    pub content: String,
    pub priority: PlanEntryPriority,
    pub status: PlanEntryStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionOptionView {
    pub id: String,
    pub name: String,
    pub kind: PermissionOptionKind,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionView {
    pub request_id: Uuid,
    pub tool_call: ToolCallView,
    pub options: Vec<PermissionOptionView>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AcpEvent {
    MessageChunk {
        turn: Option<u64>,
        content: ContentView,
    },
    ThoughtChunk {
        turn: Option<u64>,
        content: ContentView,
    },
    /// The agent's text for this turn passed the display budget; later
    /// chunks of this turn are dropped.
    TextTruncated {
        turn: Option<u64>,
    },
    ToolCall {
        turn: Option<u64>,
        call: ToolCallView,
    },
    Plan {
        turn: Option<u64>,
        entries: Vec<PlanEntryView>,
    },
    Permission {
        turn: Option<u64>,
        request: PermissionView,
    },
    /// `option_id` is `None` when the request was cancelled.
    PermissionResolved {
        request_id: Uuid,
        option_id: Option<String>,
    },
    /// The agent's session settings changed; carries the full new state.
    ConfigChanged {
        config: SessionConfig,
    },
    TitleChanged {
        title: Option<String>,
    },
    /// The agent changed a workspace file through Blade; the change can be
    /// reviewed and rejected in the editor.
    FileWritten {
        turn: Option<u64>,
        path: String,
    },
    /// A terminal the agent runs through Blade started, printed or ended.
    Terminal {
        turn: Option<u64>,
        terminal: TerminalView,
    },
    /// Blade refused to let the agent write a file.
    FileRefused {
        turn: Option<u64>,
        path: String,
        reason: FileError,
    },
    Usage {
        used: u64,
        size: u64,
    },
    TurnEnded {
        turn: u64,
        stop_reason: StopReason,
    },
    TurnFailed {
        turn: u64,
        error: RuntimeError,
    },
    /// The session ended without the user closing it.
    Exited {
        error: Option<RuntimeError>,
    },
}

pub type Sink = Arc<dyn Fn(AcpEvent) + Send + Sync>;

/// Stored with the transcript so a finished turn stays readable without the
/// agent. Tool calls and diffs are the agent's reports, never Blade actions.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentActivity {
    pub agent_name: String,
    #[serde(default)]
    pub tool_calls: Vec<ToolCallView>,
    /// Reports beyond the stored limit.
    #[serde(default)]
    pub omitted_tool_calls: usize,
    #[serde(default)]
    pub plan: Vec<PlanEntryView>,
    #[serde(default)]
    pub stop_reason: Option<StopReason>,
    /// A runtime error code when the turn did not end normally.
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub text_truncated: bool,
}

pub struct FinishedTurn {
    pub turn: u64,
    pub text: String,
    pub reasoning: String,
    pub activity: AgentActivity,
}

const MAX_STORED_TOOL_CALLS: usize = 128;

/// Collects one turn's events into what the transcript keeps.
#[derive(Default)]
pub struct TurnRecorder {
    turn: Option<u64>,
    text: String,
    reasoning: String,
    tools: Vec<ToolCallView>,
    omitted: usize,
    plan: Vec<PlanEntryView>,
    truncated: bool,
}

impl TurnRecorder {
    /// Returns the finished turn when `event` ends the one being recorded.
    pub fn observe(&mut self, event: &AcpEvent, agent_name: &str) -> Option<FinishedTurn> {
        let turn = match event {
            AcpEvent::MessageChunk { turn, .. }
            | AcpEvent::ThoughtChunk { turn, .. }
            | AcpEvent::TextTruncated { turn }
            | AcpEvent::ToolCall { turn, .. }
            | AcpEvent::Plan { turn, .. } => *turn,
            AcpEvent::TurnEnded { turn, .. } | AcpEvent::TurnFailed { turn, .. } => Some(*turn),
            _ => None,
        }?;
        if self.turn != Some(turn) {
            *self = Self {
                turn: Some(turn),
                ..Self::default()
            };
        }
        match event {
            AcpEvent::MessageChunk {
                content: ContentView::Text { text },
                ..
            } => self.text.push_str(text),
            AcpEvent::ThoughtChunk {
                content: ContentView::Text { text },
                ..
            } => self.reasoning.push_str(text),
            AcpEvent::TextTruncated { .. } => self.truncated = true,
            AcpEvent::ToolCall { call, .. } => {
                if let Some(known) = self.tools.iter_mut().find(|known| known.id == call.id) {
                    *known = call.clone();
                } else if self.tools.len() < MAX_STORED_TOOL_CALLS {
                    self.tools.push(call.clone());
                } else {
                    self.omitted += 1;
                }
            }
            AcpEvent::Plan { entries, .. } => self.plan = entries.clone(),
            AcpEvent::TurnEnded { stop_reason, .. } => {
                return Some(self.finish(agent_name, Some(*stop_reason), None))
            }
            AcpEvent::TurnFailed { error, .. } => {
                return Some(self.finish(agent_name, None, Some(*error)))
            }
            _ => {}
        }
        None
    }

    /// Ends a turn that the user abandoned by closing the session.
    pub fn abandon(&mut self, agent_name: &str) -> Option<FinishedTurn> {
        self.turn?;
        Some(self.finish(agent_name, None, Some(RuntimeError::Cancelled)))
    }

    fn finish(
        &mut self,
        agent_name: &str,
        stop_reason: Option<StopReason>,
        error: Option<RuntimeError>,
    ) -> FinishedTurn {
        let recorded = std::mem::take(self);
        FinishedTurn {
            turn: recorded.turn.unwrap_or_default(),
            text: recorded.text,
            reasoning: recorded.reasoning,
            activity: AgentActivity {
                agent_name: agent_name.to_owned(),
                tool_calls: recorded.tools,
                omitted_tool_calls: recorded.omitted,
                plan: recorded.plan,
                stop_reason,
                error: error.and_then(|error| {
                    serde_json::to_value(error)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned))
                }),
                text_truncated: recorded.truncated,
            },
        }
    }
}

enum Incoming {
    Update(SessionNotification),
    Permission(
        RequestPermissionRequest,
        Responder<RequestPermissionResponse>,
    ),
    Read(ReadTextFileRequest, Responder<ReadTextFileResponse>),
    Write(WriteTextFileRequest, Responder<WriteTextFileResponse>),
    Terminal(TerminalCall),
}

/// One `terminal/*` request and where its answer goes.
enum TerminalCall {
    Create(CreateTerminalRequest, Responder<CreateTerminalResponse>),
    Output(TerminalOutputRequest, Responder<TerminalOutputResponse>),
    Wait(
        WaitForTerminalExitRequest,
        Responder<WaitForTerminalExitResponse>,
    ),
    Kill(KillTerminalRequest, Responder<KillTerminalResponse>),
    Release(ReleaseTerminalRequest, Responder<ReleaseTerminalResponse>),
}

impl TerminalCall {
    fn session(&self) -> &SessionId {
        match self {
            Self::Create(request, _) => &request.session_id,
            Self::Output(request, _) => &request.session_id,
            Self::Wait(request, _) => &request.session_id,
            Self::Kill(request, _) => &request.session_id,
            Self::Release(request, _) => &request.session_id,
        }
    }

    fn refuse(self, error: agent_client_protocol::Error) {
        let _ = match self {
            Self::Create(_, responder) => responder.respond_with_error(error),
            Self::Output(_, responder) => responder.respond_with_error(error),
            Self::Wait(_, responder) => responder.respond_with_error(error),
            Self::Kill(_, responder) => responder.respond_with_error(error),
            Self::Release(_, responder) => responder.respond_with_error(error),
        };
    }

    async fn run(self, terminals: Arc<Terminals>, sink: Sink, turn: Option<u64>) {
        fn answer<T: agent_client_protocol::JsonRpcResponse>(
            responder: Responder<T>,
            result: Result<T, agent_client_protocol::Error>,
        ) {
            let _ = match result {
                Ok(response) => responder.respond(response),
                Err(error) => responder.respond_with_error(error),
            };
        }
        match self {
            Self::Create(request, responder) => {
                let on_change: super::acp_terminals::OnChange =
                    Arc::new(move |terminal| sink(AcpEvent::Terminal { turn, terminal }));
                answer(
                    responder,
                    terminals
                        .create(request, on_change)
                        .map(CreateTerminalResponse::new),
                );
            }
            Self::Output(request, responder) => {
                answer(responder, terminals.output(&request.terminal_id.0))
            }
            Self::Wait(request, responder) => answer(
                responder,
                terminals
                    .wait_for_exit(&request.terminal_id.0)
                    .await
                    .map(WaitForTerminalExitResponse::new),
            ),
            Self::Kill(request, responder) => answer(
                responder,
                terminals
                    .kill(&request.terminal_id.0)
                    .map(|()| KillTerminalResponse::new()),
            ),
            Self::Release(request, responder) => answer(
                responder,
                terminals
                    .release(&request.terminal_id.0)
                    .map(|()| ReleaseTerminalResponse::new()),
            ),
        }
    }
}

enum Command {
    Prompt {
        parts: Vec<PromptPart>,
        reply: oneshot::Sender<Result<u64, RuntimeError>>,
    },
    Cancel {
        reply: oneshot::Sender<()>,
    },
    Authenticate {
        method_id: String,
        reply: oneshot::Sender<Result<String, RuntimeError>>,
    },
    Permission {
        request_id: Uuid,
        option_id: Option<String>,
        reply: oneshot::Sender<Result<(), RuntimeError>>,
    },
    Configure {
        change: ConfigChange,
        reply: oneshot::Sender<Result<SessionConfig, RuntimeError>>,
    },
    Close,
}

type Pending = Pin<Box<dyn Future<Output = Finished> + Send>>;

enum Finished {
    Prompt(u64, Result<PromptResponse, agent_client_protocol::Error>),
    Authenticated(
        Result<Opened, RuntimeError>,
        oneshot::Sender<Result<String, RuntimeError>>,
    ),
    Configured(
        Result<Applied, RuntimeError>,
        oneshot::Sender<Result<SessionConfig, RuntimeError>>,
    ),
}

/// What the agent accepted.
enum Applied {
    /// The full option state `session/set_config_option` returned.
    Options(Vec<SessionConfigOption>),
    Mode(String),
}

/// Where the session works, the MCP servers it is given and, when set, the
/// workspace files the agent may read and write through Blade. Servers whose
/// transport the agent did not advertise are left out before `session/new`.
#[derive(Clone)]
pub struct SessionSetup {
    pub cwd: PathBuf,
    pub mcp_servers: Vec<McpServer>,
    pub files: Option<Arc<WorkspaceFiles>>,
    /// When set, the agent may run commands in terminals Blade owns.
    pub terminals: Option<Arc<Terminals>>,
    /// The agent's earlier session for this conversation, to continue.
    pub resume: Option<String>,
}

/// How the agent's session for this conversation began.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reopened {
    New,
    /// The agent continued its earlier session (`session/resume`).
    Resumed,
    /// The agent reloaded its earlier session (`session/load`); the history it
    /// replayed is already in Blade's transcript and is not shown again.
    Loaded,
    /// The agent could not continue its earlier session and started afresh.
    Replaced,
}

/// A session the agent opened.
struct Opened {
    session_id: SessionId,
    config: SessionConfig,
    how: Reopened,
}
impl From<PathBuf> for SessionSetup {
    fn from(cwd: PathBuf) -> Self {
        Self {
            cwd,
            mcp_servers: Vec::new(),
            files: None,
            terminals: None,
            resume: None,
        }
    }
}
impl SessionSetup {
    fn supported_by(mut self, snapshot: &AgentSnapshot) -> Self {
        self.mcp_servers.retain(|server| supports(snapshot, server));
        self
    }
}
/// Whether the agent advertised the transport of this MCP server.
pub fn supports(snapshot: &AgentSnapshot, server: &McpServer) -> bool {
    match server {
        McpServer::Stdio(_) => true,
        McpServer::Http(_) => snapshot.mcp_http,
        McpServer::Sse(_) => snapshot.mcp_sse,
        _ => false,
    }
}

#[derive(Clone)]
pub struct AcpSession {
    commands: mpsc::Sender<Command>,
    finished: CancellationToken,
    pub snapshot: Arc<AgentSnapshot>,
    config: Arc<std::sync::Mutex<SessionConfig>>,
    reopened: Arc<std::sync::Mutex<Option<Reopened>>>,
}

pub struct Started {
    pub session: AcpSession,
    /// `None` when the agent needs a sign-in before it creates a session.
    pub session_id: Option<String>,
}

impl AcpSession {
    /// Initializes the agent and asks it for a session in the setup's `cwd`.
    /// Returns once the agent answered, or when it requires authentication first.
    pub async fn start(
        read: impl AsyncRead + Send + Unpin + 'static,
        write: impl AsyncWrite + Send + Unpin + 'static,
        setup: impl Into<SessionSetup>,
        sink: Sink,
    ) -> Result<Started, RuntimeError> {
        let setup = setup.into();
        let eof = CancellationToken::new();
        let read = FrameGuard::new(read, eof.clone());
        let (incoming_tx, incoming) = mpsc::channel(INCOMING_QUEUE);
        let (commands, command_rx) = mpsc::channel(COMMAND_QUEUE);
        let (started_tx, started_rx) = oneshot::channel();
        let overflow = Arc::new(AtomicBool::new(false));
        let finished = CancellationToken::new();
        let config: Arc<std::sync::Mutex<SessionConfig>> = Arc::default();
        let reopened: Arc<std::sync::Mutex<Option<Reopened>>> = Arc::default();
        let replaying = Arc::new(AtomicBool::new(false));
        let actor = Actor {
            sink: sink.clone(),
            config: config.clone(),
            reopened: reopened.clone(),
            replaying: replaying.clone(),
            files: setup.files.clone(),
            terminals: setup.terminals.clone(),
            incoming,
            commands: command_rx,
            overflow: overflow.clone(),
            eof,
            snapshot: None,
            session: None,
            turn: 0,
            prompting: None,
            cancel_deadline: None,
            permissions: HashMap::new(),
            tools: HashMap::new(),
            tool_order: VecDeque::new(),
            turn_text: 0,
            text_truncated: false,
        };
        let done = finished.clone();
        let live = Arc::new(AtomicBool::new(false));
        let actor_live = live.clone();
        // The actor's own account of how the session ended. The connection
        // result is used only when the actor never got to report one.
        let ended: Arc<std::sync::Mutex<Option<Exit>>> = Default::default();
        let actor_ended = ended.clone();
        let update_tx = incoming_tx.clone();
        let update_overflow = overflow.clone();
        let update_replaying = replaying.clone();
        let read_tx = incoming_tx.clone();
        let read_overflow = overflow.clone();
        let write_tx = incoming_tx.clone();
        let write_overflow = overflow.clone();
        let permission_overflow = overflow.clone();
        // Queues a `terminal/*` request for the actor; a full queue ends the session.
        macro_rules! terminal_request {
            ($builder:expr, $request:ty, $variant:ident) => {{
                let queue = incoming_tx.clone();
                let overflow = overflow.clone();
                $builder.on_receive_request(
                    async move |request: $request, responder, _connection| {
                        let call = Incoming::Terminal(TerminalCall::$variant(request, responder));
                        if let Err(error) = queue.try_send(call) {
                            overflow.store(true, Ordering::Release);
                            if let Incoming::Terminal(call) = error.into_inner() {
                                call.refuse(agent_client_protocol::Error::internal_error());
                            }
                        }
                        Ok(())
                    },
                    agent_client_protocol::on_receive_request!(),
                )
            }};
        }
        let builder = Client.builder();
        let builder = terminal_request!(builder, CreateTerminalRequest, Create);
        let builder = terminal_request!(builder, TerminalOutputRequest, Output);
        let builder = terminal_request!(builder, WaitForTerminalExitRequest, Wait);
        let builder = terminal_request!(builder, KillTerminalRequest, Kill);
        let builder = terminal_request!(builder, ReleaseTerminalRequest, Release);
        tokio::spawn(async move {
            let result = builder
                .on_receive_notification(
                    async move |notification: SessionNotification, _connection| {
                        if update_replaying.load(Ordering::Acquire)
                            && replayed(&notification.update)
                        {
                            return Ok(());
                        }
                        if update_tx.try_send(Incoming::Update(notification)).is_err() {
                            update_overflow.store(true, Ordering::Release);
                        }
                        Ok(())
                    },
                    agent_client_protocol::on_receive_notification!(),
                )
                .on_receive_request(
                    async move |request: RequestPermissionRequest, responder, _connection| {
                        match incoming_tx.try_send(Incoming::Permission(request, responder)) {
                            Ok(()) => Ok(()),
                            Err(error) => {
                                permission_overflow.store(true, Ordering::Release);
                                match error.into_inner() {
                                    Incoming::Permission(_, responder) => {
                                        responder.respond(RequestPermissionResponse::new(
                                            RequestPermissionOutcome::Cancelled,
                                        ))
                                    }
                                    _ => Ok(()),
                                }
                            }
                        }
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |request: ReadTextFileRequest, responder, _connection| match read_tx
                        .try_send(Incoming::Read(request, responder))
                    {
                        Ok(()) => Ok(()),
                        Err(error) => {
                            read_overflow.store(true, Ordering::Release);
                            match error.into_inner() {
                                Incoming::Read(_, responder) => responder.respond_with_error(
                                    agent_client_protocol::Error::internal_error(),
                                ),
                                _ => Ok(()),
                            }
                        }
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .on_receive_request(
                    async move |request: WriteTextFileRequest, responder, _connection| {
                        match write_tx.try_send(Incoming::Write(request, responder)) {
                            Ok(()) => Ok(()),
                            Err(error) => {
                                write_overflow.store(true, Ordering::Release);
                                match error.into_inner() {
                                    Incoming::Write(_, responder) => responder.respond_with_error(
                                        agent_client_protocol::Error::internal_error(),
                                    ),
                                    _ => Ok(()),
                                }
                            }
                        }
                    },
                    agent_client_protocol::on_receive_request!(),
                )
                .connect_with(
                    ByteStreams::new(write.compat_write(), read.compat()),
                    async move |connection: ConnectionTo<Agent>| {
                        let exit = actor.run(connection, setup, started_tx, actor_live).await;
                        if let Ok(mut ended) = actor_ended.lock() {
                            *ended = Some(exit);
                        }
                        Ok(())
                    },
                )
                .await;
            let exit = ended
                .lock()
                .ok()
                .and_then(|mut ended| ended.take())
                .unwrap_or(match result {
                    Ok(()) => None,
                    Err(_) => Some(Some(RuntimeError::ProtocolFailed)),
                });
            if let (Some(error), true) = (exit, live.load(Ordering::Acquire)) {
                sink(AcpEvent::Exited { error });
            }
            done.cancel();
        });
        match started_rx.await {
            Ok(Ok((snapshot, session_id))) => Ok(Started {
                session: AcpSession {
                    commands,
                    finished,
                    snapshot,
                    config,
                    reopened,
                },
                session_id,
            }),
            Ok(Err(error)) => Err(error),
            Err(_) => Err(RuntimeError::ProtocolFailed),
        }
    }

    async fn request<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<T>) -> Command,
    ) -> Result<T, RuntimeError> {
        let (reply, response) = oneshot::channel();
        self.commands
            .send(command(reply))
            .await
            .map_err(|_| RuntimeError::ConnectionNotReady)?;
        response.await.map_err(|_| RuntimeError::ConnectionNotReady)
    }

    /// Starts a turn and returns its number. Events report its progress.
    pub async fn prompt(&self, parts: Vec<PromptPart>) -> Result<u64, RuntimeError> {
        self.request(|reply| Command::Prompt { parts, reply })
            .await?
    }

    /// Asks the agent to stop the current turn. Pending permission requests
    /// are answered as cancelled.
    pub async fn cancel(&self) -> Result<(), RuntimeError> {
        self.request(|reply| Command::Cancel { reply }).await
    }

    /// Runs one of the agent's own sign-in methods, then creates the session.
    pub async fn authenticate(&self, method_id: String) -> Result<String, RuntimeError> {
        self.request(|reply| Command::Authenticate { method_id, reply })
            .await?
    }

    /// Answers a permission request. `None` cancels it.
    pub async fn answer_permission(
        &self,
        request_id: Uuid,
        option_id: Option<String>,
    ) -> Result<(), RuntimeError> {
        self.request(|reply| Command::Permission {
            request_id,
            option_id,
            reply,
        })
        .await?
    }

    /// Applies one of the session settings the agent offered and returns the
    /// agent's full new state. Refused while a turn runs.
    pub async fn configure(&self, change: ConfigChange) -> Result<SessionConfig, RuntimeError> {
        self.request(|reply| Command::Configure { change, reply })
            .await?
    }

    /// How the session began; `None` until it exists (it may need a sign-in).
    pub fn reopened(&self) -> Option<Reopened> {
        self.reopened.lock().ok().and_then(|reopened| *reopened)
    }

    /// The agent's current session settings.
    pub fn config(&self) -> SessionConfig {
        self.config
            .lock()
            .map(|config| config.clone())
            .unwrap_or_default()
    }

    /// Ends the session. Pending permission requests are answered as
    /// cancelled and a running turn is asked to stop.
    pub fn close(&self) {
        let _ = self.commands.try_send(Command::Close);
    }

    /// Resolves once the connection has ended for any reason.
    pub async fn finished(&self) {
        self.finished.cancelled().await;
    }

    pub fn is_finished(&self) -> bool {
        self.finished.is_cancelled()
    }
}

struct Parked {
    responder: Responder<RequestPermissionResponse>,
    options: Vec<String>,
}

struct Actor {
    sink: Sink,
    config: Arc<std::sync::Mutex<SessionConfig>>,
    reopened: Arc<std::sync::Mutex<Option<Reopened>>>,
    /// Set while `session/load` replays history Blade already keeps.
    replaying: Arc<AtomicBool>,
    files: Option<Arc<WorkspaceFiles>>,
    terminals: Option<Arc<Terminals>>,
    incoming: mpsc::Receiver<Incoming>,
    commands: mpsc::Receiver<Command>,
    overflow: Arc<AtomicBool>,
    eof: CancellationToken,
    snapshot: Option<Arc<AgentSnapshot>>,
    session: Option<SessionId>,
    turn: u64,
    prompting: Option<u64>,
    cancel_deadline: Option<tokio::time::Instant>,
    permissions: HashMap<Uuid, Parked>,
    tools: HashMap<String, ToolCallView>,
    tool_order: VecDeque<String>,
    turn_text: usize,
    text_truncated: bool,
}

/// `None`: the user closed the session. `Some(error)`: it ended on its own.
type Exit = Option<Option<RuntimeError>>;

impl Actor {
    async fn run(
        mut self,
        connection: ConnectionTo<Agent>,
        setup: SessionSetup,
        started: oneshot::Sender<Result<(Arc<AgentSnapshot>, Option<String>), RuntimeError>>,
        live: Arc<AtomicBool>,
    ) -> Exit {
        let opened = tokio::select! {
            biased;
            _ = self.eof.cancelled() => Err(RuntimeError::ProtocolFailed),
            opened = open(&connection, setup, &self.replaying) => opened,
        };
        let (snapshot, session, setup) = match opened {
            Ok(opened) => opened,
            Err(error) => {
                let _ = started.send(Err(error));
                return None;
            }
        };
        let snapshot = Arc::new(snapshot);
        live.store(true, Ordering::Release);
        self.snapshot = Some(snapshot.clone());
        let session = session.map(|opened| self.adopt(opened));
        self.session = session.clone();
        if started
            .send(Ok((snapshot, session.map(|id| id.to_string()))))
            .is_err()
        {
            return None;
        }
        let mut pending: Option<Pending> = None;
        let exit = loop {
            if self.overflow.load(Ordering::Acquire) {
                break Some(Some(RuntimeError::OutputLimit));
            }
            let deadline = self.cancel_deadline;
            tokio::select! {
                biased;
                _ = self.eof.cancelled() => break Some(None),
                _ = sleep_until(deadline) => {
                    if let Some(turn) = self.prompting.take() {
                        (self.sink)(AcpEvent::TurnFailed { turn, error: RuntimeError::TimedOut });
                    }
                    break Some(Some(RuntimeError::TimedOut));
                }
                Some(incoming) = self.incoming.recv() => self.receive(incoming),
                finished = poll_pending(&mut pending) => {
                    pending = None;
                    self.finish(finished);
                }
                command = self.commands.recv() => match command {
                    None | Some(Command::Close) => break None,
                    Some(command) => if let Some(next) = self.command(&connection, &setup, command, pending.is_some()) {
                        pending = Some(next);
                    },
                },
            }
        };
        self.cancel_permissions();
        if let Some(terminals) = &self.terminals {
            terminals.close_all();
        }
        if let Some(turn) = self.prompting.take() {
            match exit {
                None => {
                    if let Some(session) = &self.session {
                        let _ =
                            connection.send_notification(CancelNotification::new(session.clone()));
                    }
                }
                Some(error) => (self.sink)(AcpEvent::TurnFailed {
                    turn,
                    error: error.unwrap_or(RuntimeError::AgentExited),
                }),
            }
        }
        exit
    }

    fn command(
        &mut self,
        connection: &ConnectionTo<Agent>,
        setup: &SessionSetup,
        command: Command,
        busy: bool,
    ) -> Option<Pending> {
        match command {
            Command::Prompt { parts, reply } => {
                let Some(session) = self.session.clone() else {
                    let _ = reply.send(Err(RuntimeError::AuthenticationRequired));
                    return None;
                };
                if busy {
                    let _ = reply.send(Err(RuntimeError::Busy));
                    return None;
                }
                let snapshot = self.snapshot.clone()?;
                let blocks = match prompt_blocks(parts, &snapshot) {
                    Ok(blocks) => blocks,
                    Err(error) => {
                        let _ = reply.send(Err(error));
                        return None;
                    }
                };
                self.turn += 1;
                let turn = self.turn;
                self.prompting = Some(turn);
                self.turn_text = 0;
                self.text_truncated = false;
                let _ = reply.send(Ok(turn));
                let connection = connection.clone();
                Some(Box::pin(async move {
                    Finished::Prompt(
                        turn,
                        connection
                            .send_request(PromptRequest::new(session, blocks))
                            .block_task()
                            .await,
                    )
                }))
            }
            Command::Cancel { reply } => {
                if let (Some(_), Some(session)) = (self.prompting, &self.session) {
                    if self.cancel_deadline.is_none() {
                        let _ =
                            connection.send_notification(CancelNotification::new(session.clone()));
                        self.cancel_deadline = Some(tokio::time::Instant::now() + CANCEL_GRACE);
                    }
                }
                self.cancel_permissions();
                let _ = reply.send(());
                None
            }
            Command::Authenticate { method_id, reply } => {
                let offered = self.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot
                        .auth_methods
                        .iter()
                        .any(|method| method.id == method_id && !method.terminal)
                });
                if !offered {
                    let _ = reply.send(Err(RuntimeError::InvalidArguments));
                    return None;
                }
                if self.session.is_some() {
                    let _ = reply.send(Err(RuntimeError::Busy));
                    return None;
                }
                if busy {
                    let _ = reply.send(Err(RuntimeError::Busy));
                    return None;
                }
                let connection = connection.clone();
                let setup = setup.clone();
                let snapshot = self.snapshot.clone()?;
                let replaying = self.replaying.clone();
                Some(Box::pin(async move {
                    let result = async {
                        tokio::time::timeout(
                            AUTH_TIMEOUT,
                            connection
                                .send_request(AuthenticateRequest::new(method_id))
                                .block_task(),
                        )
                        .await
                        .map_err(|_| RuntimeError::TimedOut)?
                        .map_err(|error| match error.code {
                            ErrorCode::AuthRequired => RuntimeError::AuthorizationDenied,
                            _ => RuntimeError::AuthorizationFailed,
                        })?;
                        open_session(&connection, &snapshot, setup, &replaying).await
                    }
                    .await;
                    Finished::Authenticated(result, reply)
                }))
            }
            Command::Permission {
                request_id,
                option_id,
                reply,
            } => {
                let Some(parked) = self.permissions.remove(&request_id) else {
                    let _ = reply.send(Err(RuntimeError::ApprovalExpired));
                    return None;
                };
                if let Some(option) = &option_id {
                    if !parked.options.contains(option) {
                        self.permissions.insert(request_id, parked);
                        let _ = reply.send(Err(RuntimeError::InvalidArguments));
                        return None;
                    }
                }
                let outcome = match &option_id {
                    Some(option) => RequestPermissionOutcome::Selected(
                        SelectedPermissionOutcome::new(option.clone()),
                    ),
                    None => RequestPermissionOutcome::Cancelled,
                };
                let sent = parked
                    .responder
                    .respond(RequestPermissionResponse::new(outcome))
                    .map_err(|_| RuntimeError::ConnectionNotReady);
                (self.sink)(AcpEvent::PermissionResolved {
                    request_id,
                    option_id,
                });
                let _ = reply.send(sent);
                None
            }
            Command::Configure { change, reply } => {
                let Some(session) = self.session.clone() else {
                    let _ = reply.send(Err(RuntimeError::AuthenticationRequired));
                    return None;
                };
                if busy {
                    let _ = reply.send(Err(RuntimeError::Busy));
                    return None;
                }
                let config = self.current_config();
                match config.check(&change) {
                    Ok(false) => {}
                    Ok(true) => {
                        let _ = reply.send(Ok(config));
                        return None;
                    }
                    Err(error) => {
                        let _ = reply.send(Err(error));
                        return None;
                    }
                }
                let connection = connection.clone();
                Some(Box::pin(async move {
                    let result = configure(&connection, session, change).await;
                    Finished::Configured(result, reply)
                }))
            }
            Command::Close => None,
        }
    }

    fn current_config(&self) -> SessionConfig {
        self.config
            .lock()
            .map(|config| config.clone())
            .unwrap_or_default()
    }

    /// Changes the settings and reports the new state when `change` altered it.
    fn change_config(&self, change: impl FnOnce(&mut SessionConfig)) -> SessionConfig {
        let Ok(mut config) = self.config.lock() else {
            return SessionConfig::default();
        };
        let before = config.clone();
        change(&mut config);
        let after = config.clone();
        drop(config);
        if after != before {
            (self.sink)(AcpEvent::ConfigChanged {
                config: after.clone(),
            });
        }
        after
    }

    /// Takes the settings the session declared and returns its id.
    fn adopt(&self, opened: Opened) -> SessionId {
        self.change_config(|config| *config = opened.config);
        if let Ok(mut reopened) = self.reopened.lock() {
            *reopened = Some(opened.how);
        }
        opened.session_id
    }

    fn finish(&mut self, finished: Finished) {
        match finished {
            Finished::Prompt(turn, result) => {
                if self.prompting == Some(turn) {
                    self.prompting = None;
                }
                self.cancel_deadline = None;
                match result {
                    Ok(response) => (self.sink)(AcpEvent::TurnEnded {
                        turn,
                        stop_reason: response.stop_reason,
                    }),
                    Err(error) => (self.sink)(AcpEvent::TurnFailed {
                        turn,
                        error: match error.code {
                            ErrorCode::AuthRequired => RuntimeError::AuthenticationRequired,
                            ErrorCode::RequestCancelled => RuntimeError::Cancelled,
                            _ => RuntimeError::ProtocolFailed,
                        },
                    }),
                }
            }
            Finished::Authenticated(result, reply) => {
                let result = result.map(|opened| {
                    let session = self.adopt(opened);
                    self.session = Some(session.clone());
                    session.to_string()
                });
                let _ = reply.send(result);
            }
            Finished::Configured(result, reply) => {
                let result = result.map(|applied| {
                    self.change_config(|config| match applied {
                        Applied::Options(options) => config.replace_options(&options),
                        Applied::Mode(mode_id) => {
                            config.set_mode(&mode_id);
                        }
                    })
                });
                let _ = reply.send(result);
            }
        }
    }

    fn receive(&mut self, incoming: Incoming) {
        match incoming {
            Incoming::Update(notification) => {
                if self.session.as_ref() == Some(&notification.session_id) {
                    self.update(notification.update);
                }
            }
            Incoming::Permission(request, responder) => self.park(request, responder),
            Incoming::Read(request, responder) => {
                let files = match self.files_for(&request.session_id) {
                    Ok(files) => files,
                    Err(error) => {
                        let _ = responder.respond_with_error(error);
                        return;
                    }
                };
                tokio::task::spawn_blocking(move || {
                    let _ = match files.read(&request.path, request.line, request.limit) {
                        Ok(content) => responder.respond(ReadTextFileResponse::new(content)),
                        Err(error) => {
                            responder.respond_with_error(error.to_protocol(&request.path))
                        }
                    };
                });
            }
            Incoming::Terminal(call) => {
                let terminals = match self.terminals_for(call.session()) {
                    Ok(terminals) => terminals,
                    Err(error) => return call.refuse(error),
                };
                tokio::spawn(call.run(terminals, self.sink.clone(), self.prompting));
            }
            Incoming::Write(request, responder) => {
                let files = match self.files_for(&request.session_id) {
                    Ok(files) => files,
                    Err(error) => {
                        let _ = responder.respond_with_error(error);
                        return;
                    }
                };
                let sink = self.sink.clone();
                let turn = self.prompting;
                tokio::task::spawn_blocking(move || {
                    let _ = match files.write(&request.path, &request.content) {
                        Ok(written) => {
                            if let Some(path) = written {
                                sink(AcpEvent::FileWritten {
                                    turn,
                                    path: clip(&path.to_string_lossy(), MAX_DESCRIPTION),
                                });
                            }
                            responder.respond(WriteTextFileResponse::new())
                        }
                        Err(error) => {
                            if matches!(
                                error,
                                FileError::UnsavedChanges
                                    | FileError::Protected
                                    | FileError::OutsideWorkspace
                            ) {
                                sink(AcpEvent::FileRefused {
                                    turn,
                                    path: clip(&request.path.to_string_lossy(), MAX_DESCRIPTION),
                                    reason: error,
                                });
                            }
                            responder.respond_with_error(error.to_protocol(&request.path))
                        }
                    };
                });
            }
        }
    }

    /// The terminals, for a request that belongs to this session.
    fn terminals_for(
        &self,
        session: &SessionId,
    ) -> Result<Arc<Terminals>, agent_client_protocol::Error> {
        let terminals = self
            .terminals
            .clone()
            .ok_or_else(agent_client_protocol::Error::method_not_found)?;
        if self.session.as_ref() != Some(session) {
            return Err(agent_client_protocol::Error::invalid_params());
        }
        Ok(terminals)
    }

    /// The workspace files, for a request that belongs to this session.
    fn files_for(
        &self,
        session: &SessionId,
    ) -> Result<Arc<WorkspaceFiles>, agent_client_protocol::Error> {
        let files = self
            .files
            .clone()
            .ok_or_else(agent_client_protocol::Error::method_not_found)?;
        if self.session.as_ref() != Some(session) {
            return Err(agent_client_protocol::Error::invalid_params());
        }
        Ok(files)
    }

    fn park(
        &mut self,
        request: RequestPermissionRequest,
        responder: Responder<RequestPermissionResponse>,
    ) {
        let mut ids = Vec::new();
        let valid = self.session.as_ref() == Some(&request.session_id)
            && !request.options.is_empty()
            && request.options.len() <= MAX_PERMISSION_OPTIONS
            && request.options.iter().all(|option| {
                let id = option.option_id.to_string();
                let fresh = !id.is_empty() && id.len() <= MAX_LABEL && !ids.contains(&id);
                ids.push(id);
                fresh
            });
        if !valid {
            let _ = responder.respond_with_error(agent_client_protocol::Error::invalid_params());
            return;
        }
        if self.permissions.len() >= MAX_PENDING_PERMISSIONS || self.cancel_deadline.is_some() {
            let _ = responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ));
            return;
        }
        let id = request.tool_call.tool_call_id.to_string();
        let mut call = self
            .tools
            .get(&id)
            .cloned()
            .unwrap_or_else(|| ToolCallView::empty(clip(&id, MAX_LABEL)));
        merge(&mut call, &request.tool_call.fields);
        let request_id = Uuid::new_v4();
        let options = request
            .options
            .iter()
            .map(|option| PermissionOptionView {
                id: option.option_id.to_string(),
                name: clip(&option.name, MAX_LABEL),
                kind: option.kind,
            })
            .collect();
        self.permissions.insert(
            request_id,
            Parked {
                responder,
                options: ids,
            },
        );
        (self.sink)(AcpEvent::Permission {
            turn: self.prompting,
            request: PermissionView {
                request_id,
                tool_call: call,
                options,
            },
        });
    }

    fn cancel_permissions(&mut self) {
        for (request_id, parked) in self.permissions.drain() {
            let _ = parked.responder.respond(RequestPermissionResponse::new(
                RequestPermissionOutcome::Cancelled,
            ));
            (self.sink)(AcpEvent::PermissionResolved {
                request_id,
                option_id: None,
            });
        }
    }

    fn text(&mut self, content: &ContentBlock, thought: bool) {
        if self.text_truncated {
            return;
        }
        let (view, _) = content_view(content, MAX_TURN_TEXT - self.turn_text);
        if let ContentView::Text { text } = &view {
            self.turn_text += text.len();
            if self.turn_text >= MAX_TURN_TEXT {
                self.text_truncated = true;
            }
        }
        let turn = self.prompting;
        (self.sink)(if thought {
            AcpEvent::ThoughtChunk {
                turn,
                content: view,
            }
        } else {
            AcpEvent::MessageChunk {
                turn,
                content: view,
            }
        });
        if self.text_truncated {
            (self.sink)(AcpEvent::TextTruncated { turn });
        }
    }

    fn track(&mut self, id: &str) -> &mut ToolCallView {
        if !self.tools.contains_key(id) {
            if self.tools.len() >= MAX_TOOL_CALLS {
                let evict = self
                    .tool_order
                    .iter()
                    .position(|old| {
                        self.tools.get(old).is_some_and(|call| {
                            matches!(
                                call.status,
                                ToolCallStatus::Completed | ToolCallStatus::Failed
                            )
                        })
                    })
                    .unwrap_or(0);
                if let Some(old) = self.tool_order.remove(evict) {
                    self.tools.remove(&old);
                }
            }
            self.tool_order.push_back(id.to_owned());
            self.tools
                .insert(id.to_owned(), ToolCallView::empty(id.to_owned()));
        }
        self.tools.get_mut(id).expect("tracked tool call")
    }

    fn update(&mut self, update: SessionUpdate) {
        let turn = self.prompting;
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => self.text(&chunk.content, false),
            SessionUpdate::AgentThoughtChunk(chunk) => self.text(&chunk.content, true),
            SessionUpdate::ToolCall(report) => {
                let id = clip(&report.tool_call_id.to_string(), MAX_LABEL);
                let call = self.track(&id);
                call.title = clip(&report.title, MAX_DESCRIPTION);
                call.kind = report.kind;
                call.status = report.status;
                call.set_locations(&report.locations);
                call.set_content(&report.content);
                let call = call.clone();
                (self.sink)(AcpEvent::ToolCall { turn, call });
            }
            SessionUpdate::ToolCallUpdate(report) => {
                let id = clip(&report.tool_call_id.to_string(), MAX_LABEL);
                let call = self.track(&id);
                merge(call, &report.fields);
                let call = call.clone();
                (self.sink)(AcpEvent::ToolCall { turn, call });
            }
            SessionUpdate::Plan(plan) => (self.sink)(AcpEvent::Plan {
                turn,
                entries: plan
                    .entries
                    .iter()
                    .take(MAX_PLAN_ENTRIES)
                    .map(|entry| PlanEntryView {
                        content: clip(&entry.content, MAX_DESCRIPTION),
                        priority: entry.priority.clone(),
                        status: entry.status.clone(),
                    })
                    .collect(),
            }),
            SessionUpdate::CurrentModeUpdate(update) => {
                self.change_config(|config| {
                    config.set_mode(&update.current_mode_id.to_string());
                });
            }
            SessionUpdate::ConfigOptionUpdate(update) => {
                self.change_config(|config| config.replace_options(&update.config_options));
            }
            SessionUpdate::SessionInfoUpdate(update) => {
                use agent_client_protocol::schema::MaybeUndefined;
                match update.title {
                    MaybeUndefined::Undefined => {}
                    MaybeUndefined::Null => (self.sink)(AcpEvent::TitleChanged { title: None }),
                    MaybeUndefined::Value(title) => (self.sink)(AcpEvent::TitleChanged {
                        title: Some(clip(&title, MAX_LABEL)),
                    }),
                }
            }
            SessionUpdate::UsageUpdate(usage) => (self.sink)(AcpEvent::Usage {
                used: usage.used,
                size: usage.size,
            }),
            // Echoes and replays of the user's own text, command lists and
            // capabilities Blade did not advertise: not shown yet.
            _ => {}
        }
    }
}

fn merge(
    call: &mut ToolCallView,
    fields: &agent_client_protocol::schema::v1::ToolCallUpdateFields,
) {
    if let Some(title) = &fields.title {
        call.title = clip(title, MAX_DESCRIPTION);
    }
    if let Some(kind) = fields.kind {
        call.kind = kind;
    }
    if let Some(status) = fields.status {
        call.status = status;
    }
    if let Some(locations) = &fields.locations {
        call.set_locations(locations);
    }
    if let Some(content) = &fields.content {
        call.set_content(content);
    }
}

async fn open(
    connection: &ConnectionTo<Agent>,
    setup: SessionSetup,
    replaying: &AtomicBool,
) -> Result<(AgentSnapshot, Option<Opened>, SessionSetup), RuntimeError> {
    let initialize = InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(
            ClientCapabilities::new()
                .fs(FileSystemCapabilities::new()
                    .read_text_file(setup.files.is_some())
                    .write_text_file(setup.files.is_some()))
                .terminal(setup.terminals.is_some()),
        )
        .client_info(Implementation::new(
            "zaguan-blade",
            env!("CARGO_PKG_VERSION"),
        ));
    let response = tokio::time::timeout(
        INITIALIZE_TIMEOUT,
        connection.send_request(initialize).block_task(),
    )
    .await
    .map_err(|_| RuntimeError::TimedOut)?
    .map_err(|_| RuntimeError::ProtocolFailed)?;
    if response.protocol_version != ProtocolVersion::V1 {
        return Err(RuntimeError::UnsupportedVersion);
    }
    let snapshot = AgentSnapshot::from_response(&response);
    let setup = setup.supported_by(&snapshot);
    match open_session(connection, &snapshot, setup.clone(), replaying).await {
        Ok(session) => Ok((snapshot, Some(session), setup)),
        Err(RuntimeError::AuthenticationRequired) => Ok((snapshot, None, setup)),
        Err(error) => Err(error),
    }
}

/// What `session/load` replays from the earlier conversation and Blade
/// already keeps. Setting changes go too: the load answer carries the current
/// settings, and replayed ones would undo them. Titles, command lists and
/// usage still apply.
fn replayed(update: &SessionUpdate) -> bool {
    matches!(
        update,
        SessionUpdate::UserMessageChunk(_)
            | SessionUpdate::AgentMessageChunk(_)
            | SessionUpdate::AgentThoughtChunk(_)
            | SessionUpdate::ToolCall(_)
            | SessionUpdate::ToolCallUpdate(_)
            | SessionUpdate::Plan(_)
            | SessionUpdate::ConfigOptionUpdate(_)
            | SessionUpdate::CurrentModeUpdate(_)
    )
}

/// Continues the conversation's earlier session when the agent can (resume
/// first: it does not replay), otherwise starts a new one.
async fn open_session(
    connection: &ConnectionTo<Agent>,
    snapshot: &AgentSnapshot,
    setup: SessionSetup,
    replaying: &AtomicBool,
) -> Result<Opened, RuntimeError> {
    let refused = |error: agent_client_protocol::Error| match error.code {
        ErrorCode::AuthRequired => RuntimeError::AuthenticationRequired,
        _ => RuntimeError::ProtocolFailed,
    };
    let earlier = setup
        .resume
        .clone()
        .filter(|id| !id.is_empty() && id.len() <= MAX_LABEL);
    if let Some(earlier) = &earlier {
        let session_id = SessionId::new(earlier.as_str());
        let reopened = if snapshot.resume_session {
            tokio::time::timeout(
                SESSION_TIMEOUT,
                connection
                    .send_request(
                        ResumeSessionRequest::new(session_id.clone(), setup.cwd.clone())
                            .mcp_servers(setup.mcp_servers.clone()),
                    )
                    .block_task(),
            )
            .await
            .map_err(|_| RuntimeError::TimedOut)
            .and_then(|result| result.map_err(refused))
            .map(|response| (response.config_options, response.modes, Reopened::Resumed))
        } else if snapshot.load_session {
            // The SDK runs notification handlers in arrival order before it
            // routes the response, so the whole replay sees this flag.
            replaying.store(true, Ordering::Release);
            let loaded = tokio::time::timeout(
                LOAD_TIMEOUT,
                connection
                    .send_request(
                        LoadSessionRequest::new(session_id.clone(), setup.cwd.clone())
                            .mcp_servers(setup.mcp_servers.clone()),
                    )
                    .block_task(),
            )
            .await;
            replaying.store(false, Ordering::Release);
            loaded
                .map_err(|_| RuntimeError::TimedOut)
                .and_then(|result| result.map_err(refused))
                .map(|response| (response.config_options, response.modes, Reopened::Loaded))
        } else {
            Err(RuntimeError::UnsupportedResult)
        };
        match reopened {
            Ok((options, modes, how)) => {
                return Ok(Opened {
                    session_id,
                    config: SessionConfig::new(options.as_deref(), modes.as_ref()),
                    how,
                })
            }
            Err(RuntimeError::AuthenticationRequired) => {
                return Err(RuntimeError::AuthenticationRequired)
            }
            // The agent no longer has it; a new session follows.
            Err(_) => {}
        }
    }
    let response = new_session(connection, setup).await?;
    Ok(Opened {
        session_id: response.session_id,
        config: SessionConfig::new(response.config_options.as_deref(), response.modes.as_ref()),
        how: if earlier.is_some() {
            Reopened::Replaced
        } else {
            Reopened::New
        },
    })
}

async fn new_session(
    connection: &ConnectionTo<Agent>,
    setup: SessionSetup,
) -> Result<NewSessionResponse, RuntimeError> {
    let response = tokio::time::timeout(
        SESSION_TIMEOUT,
        connection
            .send_request(NewSessionRequest::new(setup.cwd).mcp_servers(setup.mcp_servers))
            .block_task(),
    )
    .await
    .map_err(|_| RuntimeError::TimedOut)?
    .map_err(|error| match error.code {
        ErrorCode::AuthRequired => RuntimeError::AuthenticationRequired,
        _ => RuntimeError::ProtocolFailed,
    })?;
    let id = response.session_id.to_string();
    if id.is_empty() || id.len() > MAX_LABEL {
        return Err(RuntimeError::ProtocolFailed);
    }
    Ok(response)
}

async fn configure(
    connection: &ConnectionTo<Agent>,
    session: SessionId,
    change: ConfigChange,
) -> Result<Applied, RuntimeError> {
    let refused = |error: agent_client_protocol::Error| match error.code {
        ErrorCode::InvalidParams => RuntimeError::InvalidArguments,
        ErrorCode::AuthRequired => RuntimeError::AuthenticationRequired,
        _ => RuntimeError::ProtocolFailed,
    };
    let (config_id, value) = match change {
        ConfigChange::Mode { mode_id } => {
            tokio::time::timeout(
                SESSION_TIMEOUT,
                connection
                    .send_request(SetSessionModeRequest::new(session, mode_id.clone()))
                    .block_task(),
            )
            .await
            .map_err(|_| RuntimeError::TimedOut)?
            .map_err(refused)?;
            return Ok(Applied::Mode(mode_id));
        }
        ConfigChange::Select { config_id, value } => {
            (config_id, SessionConfigOptionValue::value_id(value))
        }
        ConfigChange::Boolean { config_id, value } => {
            (config_id, SessionConfigOptionValue::boolean(value))
        }
    };
    let response = tokio::time::timeout(
        SESSION_TIMEOUT,
        connection
            .send_request(SetSessionConfigOptionRequest::new(
                session, config_id, value,
            ))
            .block_task(),
    )
    .await
    .map_err(|_| RuntimeError::TimedOut)?
    .map_err(refused)?;
    Ok(Applied::Options(response.config_options))
}

async fn sleep_until(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn poll_pending(pending: &mut Option<Pending>) -> Finished {
    match pending {
        Some(future) => future.await,
        None => std::future::pending().await,
    }
}

pub(super) fn clip(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

/// Ends the stream on an over-long frame and reports end of input, so the
/// session notices an agent that exited even while it is idle.
struct FrameGuard<R> {
    inner: R,
    since_newline: usize,
    limit: usize,
    eof: CancellationToken,
}

impl<R> FrameGuard<R> {
    fn new(inner: R, eof: CancellationToken) -> Self {
        Self {
            inner,
            since_newline: 0,
            limit: MAX_FRAME_BYTES,
            eof,
        }
    }
}

impl<R: AsyncRead + Unpin> AsyncRead for FrameGuard<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let read = &buf.filled()[before..];
                if read.is_empty() {
                    this.eof.cancel();
                    return Poll::Ready(Ok(()));
                }
                match read.iter().rposition(|byte| *byte == b'\n') {
                    Some(last) => this.since_newline = read.len() - last - 1,
                    None => this.since_newline += read.len(),
                }
                if this.since_newline > this.limit {
                    this.eof.cancel();
                    return Poll::Ready(Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "ACP frame too large",
                    )));
                }
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(error)) => {
                this.eof.cancel();
                Poll::Ready(Err(error))
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::sync::Mutex;
    use tokio::io::{
        AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, Lines, ReadHalf, WriteHalf,
    };

    pub(crate) struct Wire {
        input: Lines<BufReader<ReadHalf<DuplexStream>>>,
        output: WriteHalf<DuplexStream>,
    }

    impl Wire {
        async fn read(&mut self) -> Value {
            let line = self
                .input
                .next_line()
                .await
                .unwrap()
                .expect("unexpected EOF");
            serde_json::from_str(&line).unwrap()
        }

        async fn send(&mut self, message: Value) {
            let mut bytes = serde_json::to_vec(&message).unwrap();
            bytes.push(b'\n');
            self.output.write_all(&bytes).await.unwrap();
        }

        async fn reply(&mut self, request: &Value, result: Value) {
            self.send(json!({"jsonrpc":"2.0","id":request["id"],"result":result}))
                .await;
        }

        async fn fail(&mut self, request: &Value, code: i64) {
            self.send(
                json!({"jsonrpc":"2.0","id":request["id"],"error":{"code":code,"message":"no"}}),
            )
            .await;
        }

        async fn expect(&mut self, method: &str) -> Value {
            let message = self.read().await;
            assert_eq!(message["method"], method, "unexpected message: {message}");
            message
        }

        async fn update(&mut self, update: Value) {
            self.send(json!({"jsonrpc":"2.0","method":"session/update",
                "params":{"sessionId":"s1","update":update}}))
                .await;
        }

        async fn ask(&mut self, id: u64, options: Value) {
            self.send(json!({"jsonrpc":"2.0","id":id,"method":"session/request_permission",
                "params":{"sessionId":"s1","toolCall":{"toolCallId":"call-1","title":"Edit main.rs"},
                    "options":options}}))
                .await;
        }

        /// Answers `initialize` and the first `session/new`.
        async fn handshake(&mut self, auth_required: bool) {
            self.handshake_with(auth_required, json!({"sessionId":"s1"}))
                .await;
        }

        async fn handshake_with(&mut self, auth_required: bool, session_result: Value) {
            let initialize = self.expect("initialize").await;
            assert_eq!(initialize["params"]["protocolVersion"], 1);
            let capabilities = &initialize["params"]["clientCapabilities"];
            assert_eq!(capabilities["fs"]["readTextFile"], false);
            assert_eq!(capabilities["fs"]["writeTextFile"], false);
            assert_eq!(capabilities["terminal"], false);
            assert_eq!(initialize["params"]["clientInfo"]["name"], "zaguan-blade");
            self.reply(
                &initialize,
                json!({"protocolVersion":1,
                "agentCapabilities":{"loadSession":true,"promptCapabilities":{"image":true},
                    "sessionCapabilities":{"resume":{}}},
                "agentInfo":{"name":"fixture-agent","version":"1.2.3"},
                "authMethods":[{"id":"agent-login","name":"Log in"},
                    {"type":"terminal","id":"tty-login","name":"Terminal login"}]}),
            )
            .await;
            let session = self.expect("session/new").await;
            assert_eq!(session["params"]["mcpServers"], json!([]));
            assert_eq!(session["params"]["cwd"], "/workspace");
            if auth_required {
                self.fail(&session, -32000).await;
            } else {
                self.reply(&session, session_result).await;
            }
        }
    }

    pub(crate) struct Fixture {
        events: Arc<Mutex<Vec<AcpEvent>>>,
        notify: Arc<tokio::sync::Notify>,
    }

    impl Fixture {
        async fn wait_for(&self, matches: impl Fn(&AcpEvent) -> bool) -> AcpEvent {
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let notified = self.notify.notified();
                    if let Some(event) = self.events.lock().unwrap().iter().find(|e| matches(e)) {
                        return event.clone();
                    }
                    notified.await;
                }
            })
            .await
            .expect("event did not arrive")
        }

        fn events(&self) -> Vec<AcpEvent> {
            self.events.lock().unwrap().clone()
        }
    }

    fn connect() -> (
        Wire,
        impl Future<Output = Result<Started, RuntimeError>>,
        Fixture,
    ) {
        connect_with(PathBuf::from("/workspace").into())
    }

    fn connect_with(
        setup: SessionSetup,
    ) -> (
        Wire,
        impl Future<Output = Result<Started, RuntimeError>>,
        Fixture,
    ) {
        let (client, agent) = tokio::io::duplex(1 << 20);
        let (read, write) = tokio::io::split(client);
        let (agent_read, agent_write) = tokio::io::split(agent);
        let events = Arc::new(Mutex::new(Vec::new()));
        let notify = Arc::new(tokio::sync::Notify::new());
        let sink_events = events.clone();
        let sink_notify = notify.clone();
        let sink: Sink = Arc::new(move |event| {
            sink_events.lock().unwrap().push(event);
            sink_notify.notify_waiters();
        });
        (
            Wire {
                input: BufReader::new(agent_read).lines(),
                output: agent_write,
            },
            AcpSession::start(read, write, setup, sink),
            Fixture { events, notify },
        )
    }

    /// A session with a fake agent that answered `initialize` and `session/new`.
    pub(crate) async fn ready() -> (Wire, AcpSession, Fixture) {
        let (mut wire, start, fixture) = connect();
        let (started, ()) = tokio::join!(start, wire.handshake(false));
        let started = started.unwrap();
        assert_eq!(started.session_id.as_deref(), Some("s1"));
        (wire, started.session, fixture)
    }

    /// A ready session whose `session/new` answer was `session_result`.
    async fn ready_with(session_result: Value) -> (Wire, AcpSession, Fixture) {
        let (mut wire, start, fixture) = connect();
        let (started, ()) = tokio::join!(start, wire.handshake_with(false, session_result));
        (wire, started.unwrap().session, fixture)
    }

    fn select_option(id: &str, current: &str, values: &[&str]) -> Value {
        json!({"id":id,"name":id,"type":"select","currentValue":current,
            "options":values.iter().map(|value| json!({"value":value,"name":value})).collect::<Vec<_>>()})
    }

    fn current(config: &SessionConfig, id: &str) -> Option<String> {
        use super::super::acp_config::ConfigKindView;
        config
            .options
            .iter()
            .find(|option| option.id == id)
            .map(|option| match &option.kind {
                ConfigKindView::Select { current_value, .. } => current_value.clone(),
                ConfigKindView::Boolean { current_value } => current_value.to_string(),
            })
    }

    #[tokio::test]
    async fn config_options_are_set_through_the_agent_and_its_full_state_is_kept() {
        let (mut wire, session, fixture) = ready_with(json!({"sessionId":"s1",
            "configOptions":[select_option("model","a",&["a","b"]),
                {"id":"fast","name":"Fast","type":"boolean","currentValue":false}],
            "modes":{"currentModeId":"build","availableModes":[{"id":"build","name":"Build"}]}}))
        .await;
        let config = session.config();
        assert_eq!(current(&config, "model").as_deref(), Some("a"));
        assert_eq!(config.modes, None);

        let select = |value: &str| ConfigChange::Select {
            config_id: "model".into(),
            value: value.into(),
        };
        // Values the agent did not offer, the current value and legacy modes
        // never reach the agent.
        assert_eq!(
            session.configure(select("c")).await,
            Err(RuntimeError::InvalidArguments)
        );
        assert_eq!(session.configure(select("a")).await, Ok(config));
        assert_eq!(
            session
                .configure(ConfigChange::Mode {
                    mode_id: "build".into()
                })
                .await,
            Err(RuntimeError::InvalidArguments)
        );

        let agent = async {
            let request = wire.expect("session/set_config_option").await;
            assert_eq!(
                request["params"],
                json!({"sessionId":"s1","configId":"model","value":"b"})
            );
            // The agent's answer is the whole state, which may change others.
            wire.reply(
                &request,
                json!({"configOptions":[select_option("model","b",&["a","b"]),
                    select_option("effort","low",&["low","high"])]}),
            )
            .await;
        };
        let (applied, ()) = tokio::join!(session.configure(select("b")), agent);
        let applied = applied.unwrap();
        assert_eq!(current(&applied, "model").as_deref(), Some("b"));
        assert_eq!(current(&applied, "effort").as_deref(), Some("low"));
        assert_eq!(current(&applied, "fast"), None);
        assert_eq!(session.config(), applied);
        fixture
            .wait_for(
                |event| matches!(event, AcpEvent::ConfigChanged { config } if *config == applied),
            )
            .await;

        // A boolean for a select option is refused locally; an agent's
        // refusal leaves the state as it was.
        assert_eq!(
            session
                .configure(ConfigChange::Boolean {
                    config_id: "model".into(),
                    value: true
                })
                .await,
            Err(RuntimeError::InvalidArguments)
        );
        let agent = async {
            let request = wire.expect("session/set_config_option").await;
            assert_eq!(
                request["params"],
                json!({"sessionId":"s1","configId":"effort","value":"high"})
            );
            wire.fail(&request, -32602).await;
        };
        let (refused, ()) = tokio::join!(
            session.configure(ConfigChange::Select {
                config_id: "effort".into(),
                value: "high".into()
            }),
            agent
        );
        assert_eq!(refused, Err(RuntimeError::InvalidArguments));
        assert_eq!(session.config(), applied);

        // The agent changes its own settings.
        wire.update(json!({"sessionUpdate":"config_option_update",
            "configOptions":[select_option("model","a",&["a"])]}))
            .await;
        fixture
            .wait_for(|event| {
                matches!(event, AcpEvent::ConfigChanged { config }
                if current(config, "model").as_deref() == Some("a") && config.options.len() == 1)
            })
            .await;
        assert_eq!(current(&session.config(), "model").as_deref(), Some("a"));
    }

    #[tokio::test]
    async fn legacy_modes_are_set_with_set_mode() {
        let (mut wire, session, fixture) = ready_with(json!({"sessionId":"s1",
            "modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"},
                {"id":"code","name":"Code"}]}}))
        .await;
        let agent = async {
            let request = wire.expect("session/set_mode").await;
            assert_eq!(request["params"], json!({"sessionId":"s1","modeId":"code"}));
            wire.reply(&request, json!({})).await;
        };
        let (applied, ()) = tokio::join!(
            session.configure(ConfigChange::Mode {
                mode_id: "code".into()
            }),
            agent
        );
        assert_eq!(applied.unwrap().modes.unwrap().current_mode_id, "code");
        wire.update(json!({"sessionUpdate":"current_mode_update","currentModeId":"ask"}))
            .await;
        fixture
            .wait_for(|event| {
                matches!(event, AcpEvent::ConfigChanged { config }
                if config.modes.as_ref().is_some_and(|modes| modes.current_mode_id == "ask"))
            })
            .await;
    }

    #[tokio::test]
    async fn the_agent_reads_and_writes_workspace_files_through_blade() {
        use crate::{document_service::DocumentService, integrations::acp_files::NoJournal};
        let dir = tempfile::tempdir().unwrap();
        let documents = Arc::new(DocumentService::new(dir.path().to_path_buf()));
        let root = documents.workspace_root().to_path_buf();
        std::fs::write(root.join("main.rs"), "one\ntwo\nthree\n").unwrap();
        documents.sync("open.rs", Some(1), "unsaved\n").unwrap();
        std::fs::write(root.join("open.rs"), "saved\n").unwrap();

        let (client, agent) = tokio::io::duplex(1 << 20);
        let (read, write) = tokio::io::split(client);
        let (agent_read, agent_write) = tokio::io::split(agent);
        let mut wire = Wire {
            input: BufReader::new(agent_read).lines(),
            output: agent_write,
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let setup = SessionSetup {
            cwd: root.clone(),
            mcp_servers: Vec::new(),
            files: Some(Arc::new(WorkspaceFiles::new(
                documents.clone(),
                Arc::new(NoJournal),
            ))),
            terminals: None,
            resume: None,
        };
        let start = AcpSession::start(
            read,
            write,
            setup,
            Arc::new(move |event| sink_events.lock().unwrap().push(event)),
        );
        let agent = async {
            let initialize = wire.expect("initialize").await;
            assert_eq!(
                initialize["params"]["clientCapabilities"]["fs"],
                json!({"readTextFile":true,"writeTextFile":true})
            );
            wire.reply(&initialize, json!({"protocolVersion":1})).await;
            let session = wire.expect("session/new").await;
            wire.reply(&session, json!({"sessionId":"s1"})).await;
        };
        let (started, ()) = tokio::join!(start, agent);
        let _session = started.unwrap().session;

        let path = |name: &str| root.join(name).to_string_lossy().into_owned();
        let mut call = async |id: u64, method: &str, params: Value| {
            wire.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
                .await;
            let answer = wire.read().await;
            assert_eq!(answer["id"], id, "unexpected message: {answer}");
            answer
        };
        let answer = call(
            1,
            "fs/read_text_file",
            json!({"sessionId":"s1","path":path("main.rs"),"line":2,"limit":1}),
        )
        .await;
        assert_eq!(answer["result"], json!({"content":"two\n"}));
        let answer = call(
            2,
            "fs/read_text_file",
            json!({"sessionId":"s1","path":path("open.rs")}),
        )
        .await;
        assert_eq!(
            answer["result"]["content"], "unsaved\n",
            "the editor's text wins"
        );
        let answer = call(
            3,
            "fs/read_text_file",
            json!({"sessionId":"other","path":path("main.rs")}),
        )
        .await;
        assert_eq!(answer["error"]["code"], -32602);
        let answer = call(
            4,
            "fs/read_text_file",
            json!({"sessionId":"s1","path":path("gone.rs")}),
        )
        .await;
        assert_eq!(answer["error"]["code"], -32002);

        let answer = call(
            5,
            "fs/write_text_file",
            json!({"sessionId":"s1","path":path("src/new.rs"),"content":"fn main() {}\n"}),
        )
        .await;
        assert_eq!(answer["result"], json!({}));
        assert_eq!(
            std::fs::read_to_string(root.join("src/new.rs")).unwrap(),
            "fn main() {}\n"
        );
        let answer = call(
            6,
            "fs/write_text_file",
            json!({"sessionId":"s1","path":path("open.rs"),"content":"theirs\n"}),
        )
        .await;
        assert_eq!(answer["error"]["code"], -32603);
        assert!(answer["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unsaved changes"));
        assert_eq!(
            std::fs::read_to_string(root.join("open.rs")).unwrap(),
            "saved\n"
        );

        let events = events.lock().unwrap().clone();
        assert!(events.contains(&AcpEvent::FileWritten {
            turn: None,
            path: path("src/new.rs"),
        }));
        assert!(events.contains(&AcpEvent::FileRefused {
            turn: None,
            path: path("open.rs"),
            reason: FileError::UnsavedChanges,
        }));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_agent_runs_commands_in_terminals_blade_owns() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let path = std::env::var_os("PATH").unwrap_or_default();
        let (client, agent) = tokio::io::duplex(1 << 20);
        let (read, write) = tokio::io::split(client);
        let (agent_read, agent_write) = tokio::io::split(agent);
        let mut wire = Wire {
            input: BufReader::new(agent_read).lines(),
            output: agent_write,
        };
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let setup = SessionSetup {
            cwd: root.clone(),
            mcp_servers: Vec::new(),
            files: None,
            terminals: Some(Arc::new(Terminals::new(
                root.clone(),
                vec![("PATH".into(), path)],
            ))),
            resume: None,
        };
        let start = AcpSession::start(
            read,
            write,
            setup,
            Arc::new(move |event| sink_events.lock().unwrap().push(event)),
        );
        let agent = async {
            let initialize = wire.expect("initialize").await;
            let capabilities = &initialize["params"]["clientCapabilities"];
            assert_eq!(capabilities["terminal"], true);
            assert_eq!(capabilities["fs"]["readTextFile"], false);
            wire.reply(&initialize, json!({"protocolVersion":1})).await;
            let session = wire.expect("session/new").await;
            wire.reply(&session, json!({"sessionId":"s1"})).await;
        };
        let (started, ()) = tokio::join!(start, agent);
        let session = started.unwrap().session;

        let mut call = async |id: u64, method: &str, params: Value| {
            wire.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
                .await;
            let answer = wire.read().await;
            assert_eq!(answer["id"], id, "unexpected message: {answer}");
            answer
        };
        let created = call(
            1,
            "terminal/create",
            json!({"sessionId":"s1","command":"echo built; exit 2"}),
        )
        .await;
        let id = created["result"]["terminalId"].as_str().unwrap().to_owned();
        let exit = call(
            2,
            "terminal/wait_for_exit",
            json!({"sessionId":"s1","terminalId":id}),
        )
        .await;
        assert_eq!(exit["result"]["exitCode"], 2);
        let output = call(
            3,
            "terminal/output",
            json!({"sessionId":"s1","terminalId":id}),
        )
        .await;
        assert_eq!(output["result"]["output"], "built\n");
        assert_eq!(output["result"]["truncated"], false);
        assert_eq!(output["result"]["exitStatus"]["exitCode"], 2);
        let other = call(
            4,
            "terminal/output",
            json!({"sessionId":"other","terminalId":id}),
        )
        .await;
        assert_eq!(other["error"]["code"], -32602);
        let released = call(
            5,
            "terminal/release",
            json!({"sessionId":"s1","terminalId":id}),
        )
        .await;
        assert_eq!(released["result"], json!({}));
        let gone = call(
            6,
            "terminal/output",
            json!({"sessionId":"s1","terminalId":id}),
        )
        .await;
        assert_eq!(gone["error"]["code"], -32002);

        // A running terminal is stopped when the session ends.
        let running = call(
            7,
            "terminal/create",
            json!({"sessionId":"s1","command":"sleep","args":["30"]}),
        )
        .await;
        assert!(running["result"]["terminalId"].is_string());
        session.close();
        session.finished().await;
        tokio::time::sleep(Duration::from_millis(600)).await;
        let events = events.lock().unwrap().clone();
        let views: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AcpEvent::Terminal { terminal, .. } => Some(terminal.clone()),
                _ => None,
            })
            .collect();
        assert!(views.iter().any(|view| view.id == id
            && view.released
            && view.output == "built\n"
            && view
                .exit
                .as_ref()
                .is_some_and(|exit| exit.exit_code == Some(2))));
        assert!(views.iter().any(|view| view.command == "sleep 30"
            && view.released
            && view
                .exit
                .as_ref()
                .is_some_and(|exit| exit.signal.as_deref() == Some("SIGKILL"))));
    }

    #[tokio::test]
    async fn session_new_carries_the_servers_the_agent_can_use() {
        use agent_client_protocol::schema::v1::{
            EnvVariable, HttpHeader, McpServerHttp, McpServerStdio,
        };
        let (client, agent) = tokio::io::duplex(1 << 20);
        let (read, write) = tokio::io::split(client);
        let (agent_read, agent_write) = tokio::io::split(agent);
        let mut wire = Wire {
            input: BufReader::new(agent_read).lines(),
            output: agent_write,
        };
        let setup = SessionSetup {
            cwd: PathBuf::from("/workspace"),
            mcp_servers: vec![
                McpServer::Stdio(
                    McpServerStdio::new("Lantern", "/usr/bin/lantern")
                        .args(vec!["mcp".into()])
                        .env(vec![EnvVariable::new("KEY", "value")]),
                ),
                McpServer::Http(
                    McpServerHttp::new("Remote", "https://mcp.example.com/mcp")
                        .headers(vec![HttpHeader::new("x-token", "t")]),
                ),
            ],
            files: None,
            terminals: None,
            resume: None,
        };
        let start = AcpSession::start(read, write, setup, Arc::new(|_| {}));
        let agent = async {
            let initialize = wire.expect("initialize").await;
            wire.reply(
                &initialize,
                json!({"protocolVersion":1,"agentCapabilities":{"mcpCapabilities":{"http":false}}}),
            )
            .await;
            let session = wire.expect("session/new").await;
            assert_eq!(
                session["params"]["mcpServers"],
                json!([{"name":"Lantern","command":"/usr/bin/lantern","args":["mcp"],
                    "env":[{"name":"KEY","value":"value"}]}])
            );
            wire.reply(&session, json!({"sessionId":"s1"})).await;
        };
        let (started, ()) = tokio::join!(start, agent);
        assert_eq!(started.unwrap().session_id.as_deref(), Some("s1"));
    }

    #[tokio::test]
    async fn a_turn_streams_merged_reports_and_answers_with_the_chosen_option() {
        let (mut wire, session, fixture) = ready().await;
        let snapshot = &session.snapshot;
        assert_eq!(snapshot.protocol_version, 1);
        assert_eq!(snapshot.agent.as_ref().unwrap().name, "fixture-agent");
        assert!(snapshot.load_session && snapshot.resume_session && snapshot.prompt_image);
        assert!(!snapshot.close_session && !snapshot.prompt_embedded_context);
        assert_eq!(snapshot.auth_methods.len(), 2);
        assert!(snapshot.auth_methods[1].terminal);

        let turn = session
            .prompt(vec![PromptPart::Text("Fix the bug".into())])
            .await
            .unwrap();
        assert_eq!(turn, 1);
        let prompt = wire.expect("session/prompt").await;
        assert_eq!(prompt["params"]["prompt"][0]["text"], "Fix the bug");
        assert_eq!(
            session.prompt(vec![PromptPart::Text("again".into())]).await,
            Err(RuntimeError::Busy)
        );
        wire.update(json!({"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"Looking"}})).await;
        wire.update(json!({"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"On it."}})).await;
        // Another session's update and an update kind this schema does not know.
        wire.send(json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"other",
            "update":{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"leak"}}}})).await;
        wire.update(json!({"sessionUpdate":"future_kind","value":1}))
            .await;
        wire.update(json!({"sessionUpdate":"tool_call","toolCallId":"call-1","title":"Edit main.rs",
            "kind":"edit","status":"pending","locations":[{"path":"/workspace/src/main.rs","line":3}]})).await;
        wire.update(json!({"sessionUpdate":"plan","entries":[{"content":"Fix","priority":"high","status":"in_progress"}]})).await;
        wire.ask(
            7,
            json!([{"optionId":"allow","name":"Allow","kind":"allow_once"},
            {"optionId":"never","name":"Reject always","kind":"reject_always"}]),
        )
        .await;

        let AcpEvent::Permission {
            turn: Some(1),
            request,
        } = fixture
            .wait_for(|event| matches!(event, AcpEvent::Permission { .. }))
            .await
        else {
            panic!("permission without a turn");
        };
        assert_eq!(request.tool_call.title, "Edit main.rs");
        assert_eq!(request.tool_call.locations[0].line, Some(3));
        assert_eq!(request.options[1].kind, PermissionOptionKind::RejectAlways);
        assert_eq!(
            session
                .answer_permission(request.request_id, Some("made-up".into()))
                .await,
            Err(RuntimeError::InvalidArguments)
        );
        session
            .answer_permission(request.request_id, Some("allow".into()))
            .await
            .unwrap();
        let decision = wire.read().await;
        assert_eq!(decision["id"], 7);
        assert_eq!(decision["result"]["outcome"]["outcome"], "selected");
        assert_eq!(decision["result"]["outcome"]["optionId"], "allow");
        assert_eq!(
            session.answer_permission(request.request_id, None).await,
            Err(RuntimeError::ApprovalExpired)
        );

        wire.update(json!({"sessionUpdate":"tool_call_update","toolCallId":"call-1","status":"completed",
            "content":[{"type":"diff","path":"/workspace/src/main.rs","oldText":"a","newText":"b"}]})).await;
        wire.reply(&prompt, json!({"stopReason":"end_turn"})).await;
        fixture
            .wait_for(|event| matches!(event, AcpEvent::TurnEnded { .. }))
            .await;

        let events = fixture.events();
        let texts: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AcpEvent::MessageChunk {
                    content: ContentView::Text { text },
                    ..
                }
                | AcpEvent::ThoughtChunk {
                    content: ContentView::Text { text },
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["Looking", "On it."]);
        let calls: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AcpEvent::ToolCall { call, .. } => Some(call),
                _ => None,
            })
            .collect();
        assert_eq!(calls.len(), 2);
        let last = calls[1];
        assert_eq!(last.status, ToolCallStatus::Completed);
        assert_eq!(last.kind, ToolKind::Edit);
        assert_eq!(
            last.title, "Edit main.rs",
            "an update keeps fields it omits"
        );
        assert_eq!(last.locations.len(), 1);
        assert_eq!(
            last.content,
            [ToolContentView::Diff {
                path: "/workspace/src/main.rs".into(),
                old_text: Some("a".into()),
                new_text: "b".into()
            }]
        );
        assert!(events.contains(&AcpEvent::PermissionResolved {
            request_id: request.request_id,
            option_id: Some("allow".into())
        }));
        assert!(events
            .iter()
            .any(|event| matches!(event, AcpEvent::Plan { entries, .. } if entries.len() == 1)));
        assert_eq!(
            events.last(),
            Some(&AcpEvent::TurnEnded {
                turn: 1,
                stop_reason: StopReason::EndTurn
            })
        );
        session.close();
        tokio::time::timeout(Duration::from_secs(5), session.finished())
            .await
            .unwrap();
        assert!(
            !fixture
                .events()
                .iter()
                .any(|event| matches!(event, AcpEvent::Exited { .. })),
            "closing is not reported as an exit"
        );
    }

    #[tokio::test]
    async fn cancel_answers_pending_permissions_and_waits_for_the_stop_reason() {
        let (mut wire, session, fixture) = ready().await;
        session
            .prompt(vec![PromptPart::Text("Long task".into())])
            .await
            .unwrap();
        let prompt = wire.expect("session/prompt").await;
        wire.ask(
            3,
            json!([{"optionId":"allow","name":"Allow","kind":"allow_once"}]),
        )
        .await;
        fixture
            .wait_for(|event| matches!(event, AcpEvent::Permission { .. }))
            .await;
        session.cancel().await.unwrap();
        let mut saw_cancel = false;
        let mut saw_answer = false;
        for _ in 0..2 {
            let message = wire.read().await;
            if message["method"] == "session/cancel" {
                assert_eq!(message["params"]["sessionId"], "s1");
                saw_cancel = true;
            } else {
                assert_eq!(message["id"], 3);
                assert_eq!(message["result"]["outcome"]["outcome"], "cancelled");
                saw_answer = true;
            }
        }
        assert!(saw_cancel && saw_answer);
        // A permission request after cancellation is refused straight away.
        wire.ask(
            4,
            json!([{"optionId":"allow","name":"Allow","kind":"allow_once"}]),
        )
        .await;
        let refused = wire.read().await;
        assert_eq!(refused["id"], 4);
        assert_eq!(refused["result"]["outcome"]["outcome"], "cancelled");
        wire.reply(&prompt, json!({"stopReason":"cancelled"})).await;
        assert_eq!(
            fixture
                .wait_for(|event| matches!(event, AcpEvent::TurnEnded { .. }))
                .await,
            AcpEvent::TurnEnded {
                turn: 1,
                stop_reason: StopReason::Cancelled
            }
        );
        // The session stays usable for another turn.
        assert_eq!(
            session.prompt(vec![PromptPart::Text("Next".into())]).await,
            Ok(2)
        );
        wire.expect("session/prompt").await;
    }

    #[tokio::test]
    async fn an_agent_that_ignores_cancellation_is_stopped() {
        let (mut wire, session, fixture) = ready().await;
        session
            .prompt(vec![PromptPart::Text("Spin".into())])
            .await
            .unwrap();
        wire.expect("session/prompt").await;
        session.cancel().await.unwrap();
        wire.expect("session/cancel").await;
        assert_eq!(
            fixture
                .wait_for(|event| matches!(event, AcpEvent::TurnFailed { .. }))
                .await,
            AcpEvent::TurnFailed {
                turn: 1,
                error: RuntimeError::TimedOut
            }
        );
        fixture
            .wait_for(|event| {
                *event
                    == AcpEvent::Exited {
                        error: Some(RuntimeError::TimedOut),
                    }
            })
            .await;
        tokio::time::timeout(Duration::from_secs(5), session.finished())
            .await
            .unwrap();
    }

    /// Starts a session that continues `earlier`, with an agent that
    /// advertises `capabilities`.
    async fn reopen(
        capabilities: Value,
        agent: impl AsyncFnOnce(&mut Wire),
    ) -> (Wire, Started, Fixture) {
        let setup = SessionSetup {
            resume: Some("earlier".into()),
            ..PathBuf::from("/workspace").into()
        };
        let (mut wire, start, fixture) = connect_with(setup);
        let handshake = async {
            let initialize = wire.expect("initialize").await;
            wire.reply(
                &initialize,
                json!({"protocolVersion":1,"agentCapabilities":capabilities}),
            )
            .await;
            agent(&mut wire).await;
        };
        let (started, ()) = tokio::join!(start, handshake);
        (wire, started.unwrap(), fixture)
    }

    #[tokio::test]
    async fn a_stored_conversation_resumes_its_session_without_a_replay() {
        let (_wire, started, _fixture) = reopen(
            json!({"loadSession":true,"sessionCapabilities":{"resume":{}}}),
            async |wire| {
                let resume = wire.expect("session/resume").await;
                assert_eq!(resume["params"]["sessionId"], "earlier");
                assert_eq!(resume["params"]["cwd"], "/workspace");
                wire.reply(
                    &resume,
                    json!({"configOptions":[select_option("model","b",&["a","b"])]}),
                )
                .await;
            },
        )
        .await;
        assert_eq!(started.session_id.as_deref(), Some("earlier"));
        assert_eq!(started.session.reopened(), Some(Reopened::Resumed));
        assert_eq!(
            current(&started.session.config(), "model").as_deref(),
            Some("b")
        );
    }

    #[tokio::test]
    async fn a_loaded_session_skips_its_replay_but_shows_what_follows() {
        let (mut wire, started, fixture) = reopen(json!({"loadSession":true}), async |wire| {
            let load = wire.expect("session/load").await;
            assert_eq!(load["params"]["sessionId"], "earlier");
            let replay = |update: Value| {
                json!({"jsonrpc":"2.0","method":"session/update",
                    "params":{"sessionId":"earlier","update":update}})
            };
            wire.send(replay(json!({"sessionUpdate":"user_message_chunk",
                "content":{"type":"text","text":"old question"}})))
                .await;
            wire.send(replay(json!({"sessionUpdate":"agent_message_chunk",
                "content":{"type":"text","text":"old answer"}})))
                .await;
            wire.send(replay(
                json!({"sessionUpdate":"tool_call","toolCallId":"old",
                "title":"Old edit","status":"completed"}),
            ))
            .await;
            wire.send(replay(json!({"sessionUpdate":"config_option_update",
                "configOptions":[select_option("model","a",&["a","b"])]})))
                .await;
            wire.send(replay(
                json!({"sessionUpdate":"session_info_update","title":"Kept"}),
            ))
            .await;
            wire.reply(
                &load,
                json!({"configOptions":[select_option("model","b",&["a","b"])]}),
            )
            .await;
        })
        .await;
        let session = started.session;
        assert_eq!(session.reopened(), Some(Reopened::Loaded));
        let prompt = async {
            let prompt = wire.expect("session/prompt").await;
            assert_eq!(prompt["params"]["sessionId"], "earlier");
            wire.send(json!({"jsonrpc":"2.0","method":"session/update",
                "params":{"sessionId":"earlier","update":{"sessionUpdate":"agent_message_chunk",
                "content":{"type":"text","text":"new answer"}}}}))
                .await;
            wire.reply(&prompt, json!({"stopReason":"end_turn"})).await;
        };
        let (turn, ()) = tokio::join!(session.prompt(vec![PromptPart::Text("hi".into())]), prompt);
        assert_eq!(turn, Ok(1));
        fixture
            .wait_for(|event| matches!(event, AcpEvent::TurnEnded { turn: 1, .. }))
            .await;
        let events = fixture.events();
        let texts: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AcpEvent::MessageChunk {
                    content: ContentView::Text { text },
                    ..
                } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, ["new answer"]);
        assert!(!events
            .iter()
            .any(|event| matches!(event, AcpEvent::ToolCall { .. })));
        assert!(
            events.iter().all(|event| match event {
                AcpEvent::ConfigChanged { config } =>
                    current(config, "model").as_deref() == Some("b"),
                _ => true,
            }),
            "only the current settings are shown"
        );
        assert!(events.iter().any(
            |event| matches!(event, AcpEvent::TitleChanged { title } if title.as_deref() == Some("Kept"))
        ));
        assert_eq!(
            current(&session.config(), "model").as_deref(),
            Some("b"),
            "the replayed setting does not undo the current one"
        );
    }

    #[tokio::test]
    async fn an_agent_that_cannot_continue_starts_a_new_session() {
        let (_wire, started, _fixture) =
            reopen(json!({"sessionCapabilities":{"resume":{}}}), async |wire| {
                let resume = wire.expect("session/resume").await;
                wire.fail(&resume, -32002).await;
                let session = wire.expect("session/new").await;
                wire.reply(&session, json!({"sessionId":"s2"})).await;
            })
            .await;
        assert_eq!(started.session_id.as_deref(), Some("s2"));
        assert_eq!(started.session.reopened(), Some(Reopened::Replaced));

        let (_wire, started, _fixture) = reopen(json!({}), async |wire| {
            let session = wire.expect("session/new").await;
            wire.reply(&session, json!({"sessionId":"s3"})).await;
        })
        .await;
        assert_eq!(started.session_id.as_deref(), Some("s3"));
        assert_eq!(started.session.reopened(), Some(Reopened::Replaced));
    }

    #[tokio::test]
    async fn an_image_goes_to_an_agent_that_takes_images() {
        let (mut wire, session, _fixture) = ready().await;
        let parts = vec![
            PromptPart::Text("What is this?".into()),
            PromptPart::Image {
                mime_type: "image/png".into(),
                data: "AAAA".into(),
            },
        ];
        let (turn, prompt) = tokio::join!(session.prompt(parts), wire.expect("session/prompt"));
        assert_eq!(turn, Ok(1));
        assert_eq!(
            prompt["params"]["prompt"],
            json!([{"type":"text","text":"What is this?"},
                {"type":"image","mimeType":"image/png","data":"AAAA"}])
        );
    }

    #[tokio::test]
    async fn agent_sign_in_creates_the_session() {
        let (mut wire, start, _fixture) = connect();
        let (started, ()) = tokio::join!(start, wire.handshake(true));
        let Started {
            session,
            session_id,
        } = started.unwrap();
        assert_eq!(session_id, None);
        assert_eq!(
            session.prompt(vec![PromptPart::Text("hi".into())]).await,
            Err(RuntimeError::AuthenticationRequired)
        );
        assert_eq!(
            session.authenticate("unknown".into()).await,
            Err(RuntimeError::InvalidArguments)
        );
        assert_eq!(
            session.authenticate("tty-login".into()).await,
            Err(RuntimeError::InvalidArguments),
            "terminal sign-in is not offered"
        );
        let agent = async {
            let authenticate = wire.expect("authenticate").await;
            assert_eq!(authenticate["params"]["methodId"], "agent-login");
            wire.reply(&authenticate, json!({})).await;
            let session = wire.expect("session/new").await;
            wire.reply(&session, json!({"sessionId":"s1"})).await;
        };
        let (signed_in, ()) = tokio::join!(session.authenticate("agent-login".into()), agent);
        assert_eq!(signed_in.as_deref(), Ok("s1"));
        assert_eq!(
            session.prompt(vec![PromptPart::Text("hi".into())]).await,
            Ok(1)
        );
        wire.expect("session/prompt").await;
    }

    #[tokio::test]
    async fn an_agent_exit_fails_the_turn_and_ends_the_session() {
        let (mut wire, session, fixture) = ready().await;
        session
            .prompt(vec![PromptPart::Text("Work".into())])
            .await
            .unwrap();
        wire.expect("session/prompt").await;
        wire.ask(
            9,
            json!([{"optionId":"allow","name":"Allow","kind":"allow_once"}]),
        )
        .await;
        fixture
            .wait_for(|event| matches!(event, AcpEvent::Permission { .. }))
            .await;
        drop(wire);
        tokio::time::timeout(Duration::from_secs(5), session.finished())
            .await
            .unwrap();
        let events = fixture.events();
        assert!(events.contains(&AcpEvent::TurnFailed {
            turn: 1,
            error: RuntimeError::AgentExited
        }));
        assert!(events.iter().any(|event| matches!(
            event,
            AcpEvent::PermissionResolved {
                option_id: None,
                ..
            }
        )));
        assert!(
            matches!(events.last(), Some(AcpEvent::Exited { error: None })),
            "{events:?}"
        );
        assert_eq!(
            session.prompt(vec![PromptPart::Text("more".into())]).await,
            Err(RuntimeError::ConnectionNotReady)
        );
    }

    #[tokio::test]
    async fn invalid_permission_requests_and_unsupported_prompts_are_refused() {
        let (mut wire, session, fixture) = ready().await;
        wire.send(
            json!({"jsonrpc":"2.0","id":1,"method":"session/request_permission",
            "params":{"sessionId":"other","toolCall":{"toolCallId":"c"},
                "options":[{"optionId":"a","name":"A","kind":"allow_once"}]}}),
        )
        .await;
        assert_eq!(wire.read().await["error"]["code"], -32602);
        wire.ask(2, json!([])).await;
        assert_eq!(wire.read().await["error"]["code"], -32602);
        wire.ask(
            3,
            json!([{"optionId":"a","name":"A","kind":"allow_once"},
            {"optionId":"a","name":"B","kind":"reject_once"}]),
        )
        .await;
        assert_eq!(wire.read().await["error"]["code"], -32602);
        assert!(!fixture
            .events()
            .iter()
            .any(|event| matches!(event, AcpEvent::Permission { .. })));
        assert_eq!(
            session
                .prompt(vec![PromptPart::Resource {
                    uri: "file:///a".into(),
                    mime_type: None,
                    text: "x".into()
                }])
                .await,
            Err(RuntimeError::UnsupportedResult),
            "embedded context was not advertised"
        );
        assert_eq!(
            session.prompt(Vec::new()).await,
            Err(RuntimeError::InvalidArguments)
        );
    }

    #[tokio::test]
    async fn other_protocol_versions_are_refused() {
        let (mut wire, start, fixture) = connect();
        let agent = async {
            let initialize = wire.expect("initialize").await;
            wire.reply(&initialize, json!({"protocolVersion":2})).await;
        };
        let (started, ()) = tokio::join!(start, agent);
        assert_eq!(started.err(), Some(RuntimeError::UnsupportedVersion));
        assert!(fixture.events().is_empty());
    }

    #[tokio::test]
    async fn an_over_long_frame_ends_the_stream() {
        let (mut agent, client) = tokio::io::duplex(1024);
        let eof = CancellationToken::new();
        let mut guard = FrameGuard::new(client, eof.clone());
        guard.limit = 16;
        let mut buffer = vec![0; 64];
        agent.write_all(b"{\"short\":1}\n0123").await.unwrap();
        let read = tokio::io::AsyncReadExt::read(&mut guard, &mut buffer)
            .await
            .unwrap();
        assert_eq!(&buffer[..read], b"{\"short\":1}\n0123");
        agent.write_all(b"456789abcdef!").await.unwrap();
        let error = tokio::io::AsyncReadExt::read(&mut guard, &mut buffer)
            .await
            .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(eof.is_cancelled());
    }
}

#[cfg(test)]
mod recorder_tests {
    use super::*;

    fn call(id: &str, status: ToolCallStatus) -> ToolCallView {
        ToolCallView {
            status,
            ..ToolCallView::empty(id.into())
        }
    }

    #[test]
    fn a_turn_keeps_its_text_and_the_latest_report_of_each_tool_call() {
        let mut recorder = TurnRecorder::default();
        let text = |turn, text: &str| AcpEvent::MessageChunk {
            turn,
            content: ContentView::Text { text: text.into() },
        };
        assert!(recorder
            .observe(&text(None, "outside a turn"), "Agent")
            .is_none());
        for event in [
            text(Some(1), "Hello "),
            AcpEvent::ThoughtChunk {
                turn: Some(1),
                content: ContentView::Text {
                    text: "thinking".into(),
                },
            },
            AcpEvent::ToolCall {
                turn: Some(1),
                call: call("a", ToolCallStatus::Pending),
            },
            AcpEvent::ToolCall {
                turn: Some(1),
                call: call("b", ToolCallStatus::Pending),
            },
            AcpEvent::ToolCall {
                turn: Some(1),
                call: call("a", ToolCallStatus::Completed),
            },
            text(Some(1), "world"),
        ] {
            assert!(recorder.observe(&event, "Agent").is_none());
        }
        let finished = recorder
            .observe(
                &AcpEvent::TurnEnded {
                    turn: 1,
                    stop_reason: StopReason::EndTurn,
                },
                "Agent",
            )
            .unwrap();
        assert_eq!(finished.turn, 1);
        assert_eq!(finished.text, "Hello world");
        assert_eq!(finished.reasoning, "thinking");
        let ids: Vec<_> = finished
            .activity
            .tool_calls
            .iter()
            .map(|call| (call.id.as_str(), call.status))
            .collect();
        assert_eq!(
            ids,
            [
                ("a", ToolCallStatus::Completed),
                ("b", ToolCallStatus::Pending)
            ]
        );
        assert_eq!(finished.activity.stop_reason, Some(StopReason::EndTurn));
        assert!(
            recorder.abandon("Agent").is_none(),
            "nothing left to record"
        );

        recorder.observe(&text(Some(2), "partial"), "Agent");
        let failed = recorder
            .observe(
                &AcpEvent::TurnFailed {
                    turn: 2,
                    error: RuntimeError::AgentExited,
                },
                "Agent",
            )
            .unwrap();
        assert_eq!(failed.text, "partial");
        assert_eq!(failed.activity.error.as_deref(), Some("agent_exited"));

        recorder.observe(&text(Some(3), "cut off"), "Agent");
        let abandoned = recorder.abandon("Agent").unwrap();
        assert_eq!(abandoned.turn, 3);
        assert_eq!(abandoned.activity.error.as_deref(), Some("cancelled"));
        let stored = serde_json::to_value(&abandoned.activity).unwrap();
        assert_eq!(
            serde_json::from_value::<AgentActivity>(stored).unwrap(),
            abandoned.activity
        );
    }
}
