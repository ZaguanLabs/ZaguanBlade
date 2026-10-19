//! Terminals an ACP agent runs through Blade (`terminal/*`). Each command runs
//! in its own process group (a job object on Windows) with the agent's own
//! environment, in a workspace folder. Output is kept within a byte limit,
//! dropping the oldest bytes on a character boundary. Released terminals stop
//! and vanish for the agent; the user keeps seeing their last output.
use agent_client_protocol::{
    schema::v1::{CreateTerminalRequest, TerminalExitStatus, TerminalOutputResponse},
    Error,
};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    sync::{watch, Notify},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const MAX_LIVE: usize = 16;
/// Kept per terminal for the agent, whatever limit it asks for.
const MAX_OUTPUT: usize = 1024 * 1024;
/// What the agent view shows.
const DISPLAY_OUTPUT: usize = 64 * 1024;
const MAX_COMMAND: usize = 64 * 1024;
const MAX_ENV: usize = 64;
/// Output still in the pipes when the command exits.
const DRAIN: Duration = Duration::from_millis(250);
/// Updates to the agent view, at most this often per terminal.
const UPDATE_EVERY: Duration = Duration::from_millis(200);

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExitView {
    pub exit_code: Option<u32>,
    pub signal: Option<String>,
}

/// What the agent view shows of one terminal.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct TerminalView {
    pub id: String,
    pub command: String,
    pub cwd: String,
    pub output: String,
    /// Older output is no longer shown.
    pub truncated: bool,
    pub exit: Option<ExitView>,
    pub released: bool,
}

pub type OnChange = Arc<dyn Fn(TerminalView) + Send + Sync>;

#[derive(Default)]
struct Output {
    tail: VecDeque<u8>,
    total: u64,
}

impl Output {
    fn push(&mut self, bytes: &[u8], keep: usize) {
        self.total += bytes.len() as u64;
        self.tail.extend(bytes);
        let excess = self.tail.len().saturating_sub(keep);
        self.tail.drain(..excess);
    }

    /// The last `limit` bytes as text, starting on a character boundary and
    /// without a character still being written.
    /// Also whether older output was left out.
    fn last(&self, limit: usize) -> (String, bool) {
        let bytes: Vec<u8> = self.tail.iter().copied().collect();
        let mut start = bytes.len().saturating_sub(limit);
        while start < bytes.len() && bytes[start] & 0xC0 == 0x80 {
            start += 1;
        }
        let shown = &bytes[start..];
        (
            String::from_utf8_lossy(&shown[..complete_end(shown)]).into_owned(),
            self.total > shown.len() as u64,
        )
    }
}

/// Where the text ends once a trailing incomplete character is left out.
fn complete_end(bytes: &[u8]) -> usize {
    let len = bytes.len();
    for back in 1..=len.min(3) {
        let byte = bytes[len - back];
        if byte & 0xC0 == 0x80 {
            continue;
        }
        let width = match byte {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            _ => 4,
        };
        return if width > back { len - back } else { len };
    }
    len
}

struct Terminal {
    id: String,
    command: String,
    cwd: PathBuf,
    limit: usize,
    output: Mutex<Output>,
    exit: watch::Sender<Option<ExitView>>,
    changed: Notify,
    kill: CancellationToken,
    released: AtomicBool,
}

impl Terminal {
    fn view(&self) -> TerminalView {
        let (output, truncated) = self
            .output
            .lock()
            .map(|output| output.last(DISPLAY_OUTPUT))
            .unwrap_or_default();
        TerminalView {
            id: self.id.clone(),
            command: self.command.clone(),
            cwd: self.cwd.to_string_lossy().into_owned(),
            output,
            truncated,
            exit: self.exit.borrow().clone(),
            released: self.released.load(Ordering::Acquire),
        }
    }
}

fn exit_status(exit: &ExitView) -> TerminalExitStatus {
    TerminalExitStatus::new()
        .exit_code(exit.exit_code)
        .signal(exit.signal.clone())
}

fn exit_view(status: Option<std::process::ExitStatus>) -> ExitView {
    let Some(status) = status else {
        return ExitView {
            exit_code: None,
            signal: None,
        };
    };
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal().map(|number| {
            match number {
                1 => "SIGHUP",
                2 => "SIGINT",
                3 => "SIGQUIT",
                6 => "SIGABRT",
                9 => "SIGKILL",
                13 => "SIGPIPE",
                15 => "SIGTERM",
                _ => return format!("SIG{number}"),
            }
            .to_owned()
        })
    };
    #[cfg(not(unix))]
    let signal = None;
    ExitView {
        exit_code: status.code().and_then(|code| u32::try_from(code).ok()),
        signal,
    }
}

/// The command itself, below the process group or job wrappers, so its exit
/// is reported even while something it started in the background still runs.
fn leader(child: &mut dyn ChildWrapper) -> &mut dyn ChildWrapper {
    let mut child = child;
    for _ in 0..4 {
        child = child.inner_mut();
    }
    child
}

fn not_found() -> Error {
    Error::resource_not_found(None)
}

fn invalid(message: &str) -> Error {
    Error::new(-32602, message.to_owned())
}

/// The terminals of one agent session.
pub struct Terminals {
    root: PathBuf,
    environment: Vec<(OsString, OsString)>,
    live: Mutex<HashMap<String, Arc<Terminal>>>,
}

impl Terminals {
    /// `root` is the workspace; `environment` is the agent's own.
    pub fn new(root: PathBuf, environment: Vec<(OsString, OsString)>) -> Self {
        Self {
            root,
            environment,
            live: Mutex::new(HashMap::new()),
        }
    }

    fn get(&self, id: &str) -> Result<Arc<Terminal>, Error> {
        self.live
            .lock()
            .ok()
            .and_then(|live| live.get(id).cloned())
            .ok_or_else(not_found)
    }

    fn cwd(&self, cwd: Option<&Path>) -> Result<PathBuf, Error> {
        let Some(cwd) = cwd else {
            return Ok(self.root.clone());
        };
        if !cwd.is_absolute() {
            return Err(invalid("cwd must be an absolute path."));
        }
        let real = std::fs::canonicalize(cwd).map_err(|_| invalid("cwd does not exist."))?;
        if !real.starts_with(&self.root) || !real.is_dir() {
            return Err(invalid("cwd must be a folder inside the workspace."));
        }
        Ok(real)
    }

    /// Starts the command. With no arguments, `command` is a shell command line.
    pub fn create(
        &self,
        request: CreateTerminalRequest,
        on_change: OnChange,
    ) -> Result<String, Error> {
        let size = request.command.len() + request.args.iter().map(String::len).sum::<usize>();
        if request.command.trim().is_empty()
            || size > MAX_COMMAND
            || request.command.contains('\0')
            || request.args.iter().any(|arg| arg.contains('\0'))
        {
            return Err(invalid("The command is empty or too long."));
        }
        if request.env.len() > MAX_ENV
            || request.env.iter().any(|variable| {
                variable.name.is_empty()
                    || variable.name.contains(['=', '\0'])
                    || variable.value.contains('\0')
            })
        {
            return Err(invalid("Invalid environment variables."));
        }
        let cwd = self.cwd(request.cwd.as_deref())?;
        if self.live.lock().map_or(true, |live| live.len() >= MAX_LIVE) {
            return Err(Error::new(
                -32603,
                "Too many terminals are open; release some first.",
            ));
        }
        let limit = request
            .output_byte_limit
            .map_or(MAX_OUTPUT, |limit| (limit as usize).min(MAX_OUTPUT));
        let shell = request.args.is_empty();
        let program = if shell {
            shell_program(&self.environment)
        } else {
            PathBuf::from(&request.command)
        };
        let mut command = CommandWrap::with_new(&program, |command| {
            if shell {
                #[cfg(windows)]
                command.raw_arg(format!("/D /S /C \"{}\"", request.command));
                #[cfg(not(windows))]
                command.arg("-c").arg(&request.command);
            } else {
                command.args(&request.args);
            }
            command
                .current_dir(&cwd)
                .env_clear()
                .envs(self.environment.iter().map(|(key, value)| (key, value)))
                .envs(
                    request
                        .env
                        .iter()
                        .map(|variable| (&variable.name, &variable.value)),
                )
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        });
        #[cfg(unix)]
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(process_wrap::tokio::JobObject);
        command.wrap(KillOnDrop);
        let mut child = command
            .spawn()
            .map_err(|_| invalid("The command could not be started."))?;

        let shown = if shell {
            request.command.clone()
        } else {
            std::iter::once(request.command.as_str())
                .chain(request.args.iter().map(String::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let terminal = Arc::new(Terminal {
            id: format!("term-{}", Uuid::new_v4().simple()),
            command: super::acp_session::clip(&shown, 2048),
            cwd,
            limit,
            output: Mutex::default(),
            exit: watch::Sender::new(None),
            changed: Notify::new(),
            kill: CancellationToken::new(),
            released: AtomicBool::new(false),
        });
        let readers = [
            child
                .stdout()
                .take()
                .map(|pipe| read_into(pipe, terminal.clone())),
            child
                .stderr()
                .take()
                .map(|pipe| read_into(pipe, terminal.clone())),
        ];
        let owner = terminal.clone();
        tokio::spawn(async move {
            let mut killed = false;
            let status = loop {
                tokio::select! {
                    status = leader(child.as_mut()).wait() => break status.ok(),
                    _ = owner.kill.cancelled(), if !killed => {
                        killed = true;
                        let _ = child.start_kill();
                    }
                }
            };
            let _ = tokio::time::timeout(DRAIN, async {
                for reader in readers.into_iter().flatten() {
                    let _ = reader.await;
                }
            })
            .await;
            owner.exit.send_replace(Some(exit_view(status)));
            owner.changed.notify_one();
            // Background work the command left in its group runs until the
            // terminal is killed or released, or the session ends.
            owner.kill.cancelled().await;
            let _ = child.start_kill();
        });
        let watched = terminal.clone();
        tokio::spawn(async move {
            let mut exit = watched.exit.subscribe();
            loop {
                let done = exit.borrow_and_update().is_some();
                on_change(watched.view());
                if done && watched.released.load(Ordering::Acquire) {
                    break;
                }
                tokio::select! {
                    _ = watched.changed.notified() => {}
                    changed = exit.changed(), if !done => {
                        if changed.is_err() {
                            break;
                        }
                    }
                }
                tokio::time::sleep(UPDATE_EVERY).await;
            }
        });
        let id = terminal.id.clone();
        if let Ok(mut live) = self.live.lock() {
            live.insert(id.clone(), terminal);
        }
        Ok(id)
    }

    pub fn output(&self, id: &str) -> Result<TerminalOutputResponse, Error> {
        let terminal = self.get(id)?;
        let (output, truncated) = terminal
            .output
            .lock()
            .map(|output| output.last(terminal.limit))
            .unwrap_or_default();
        let exit = terminal.exit.borrow().clone();
        Ok(TerminalOutputResponse::new(output, truncated)
            .exit_status(exit.as_ref().map(exit_status)))
    }

    pub async fn wait_for_exit(&self, id: &str) -> Result<TerminalExitStatus, Error> {
        let mut exit = self.get(id)?.exit.subscribe();
        let exit = exit
            .wait_for(Option::is_some)
            .await
            .map_err(|_| Error::internal_error())?
            .clone();
        Ok(exit.as_ref().map(exit_status).unwrap_or_default())
    }

    /// Stops the command and everything it started; output stays available.
    pub fn kill(&self, id: &str) -> Result<(), Error> {
        self.get(id)?.kill.cancel();
        Ok(())
    }

    /// Stops the command and forgets the terminal; the user still sees it.
    pub fn release(&self, id: &str) -> Result<(), Error> {
        let terminal = self
            .live
            .lock()
            .ok()
            .and_then(|mut live| live.remove(id))
            .ok_or_else(not_found)?;
        terminal.released.store(true, Ordering::Release);
        terminal.kill.cancel();
        terminal.changed.notify_one();
        Ok(())
    }

    /// Stops every terminal; the session ended.
    pub fn close_all(&self) {
        let closed: Vec<_> = self
            .live
            .lock()
            .map(|mut live| live.drain().map(|(_, terminal)| terminal).collect())
            .unwrap_or_default();
        for terminal in closed {
            terminal.released.store(true, Ordering::Release);
            terminal.kill.cancel();
            terminal.changed.notify_one();
        }
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        self.close_all();
    }
}

fn read_into(
    mut pipe: impl AsyncRead + Unpin + Send + 'static,
    terminal: Arc<Terminal>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let keep = terminal.limit.max(DISPLAY_OUTPUT);
        let mut buffer = [0; 8192];
        while let Ok(read) = pipe.read(&mut buffer).await {
            if read == 0 {
                break;
            }
            if let Ok(mut output) = terminal.output.lock() {
                output.push(&buffer[..read], keep);
            }
            terminal.changed.notify_one();
        }
    })
}

/// The user's shell when the agent's environment names one, else the system's.
fn shell_program(environment: &[(OsString, OsString)]) -> PathBuf {
    #[cfg(windows)]
    {
        let _ = environment;
        PathBuf::from("cmd.exe")
    }
    #[cfg(not(windows))]
    {
        environment
            .iter()
            .find(|(key, _)| key == "SHELL")
            .map(|(_, value)| PathBuf::from(value))
            .filter(|shell| shell.is_absolute() && shell.is_file())
            .unwrap_or_else(|| PathBuf::from("/bin/sh"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_keeps_the_newest_bytes_on_character_boundaries() {
        let mut output = Output::default();
        output.push("añb".as_bytes(), 16);
        assert_eq!(output.last(16), ("añb".into(), false));
        // "ñ" is two bytes; a cut through it starts after it.
        assert_eq!(output.last(2), ("b".into(), true));
        assert_eq!(output.last(3), ("ñb".into(), true));
        let mut output = Output::default();
        output.push(&"€".as_bytes()[..2], 16);
        assert_eq!(
            output.last(16),
            (String::new(), false),
            "still being written"
        );
        output.push(&"€".as_bytes()[2..], 16);
        assert_eq!(output.last(16), ("€".into(), false));
        let mut output = Output::default();
        output.push(b"0123456789", 4);
        assert_eq!(output.tail.len(), 4);
        assert_eq!(output.last(8), ("6789".into(), true));
    }
}

#[cfg(all(test, unix))]
mod process_tests {
    use super::*;
    use agent_client_protocol::schema::v1::EnvVariable;

    fn terminals() -> (
        tempfile::TempDir,
        Terminals,
        Arc<Mutex<Vec<TerminalView>>>,
        OnChange,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let environment = vec![(
            OsString::from("PATH"),
            std::env::var_os("PATH").unwrap_or_default(),
        )];
        let views = Arc::new(Mutex::new(Vec::new()));
        let seen = views.clone();
        let on_change: OnChange = Arc::new(move |view| seen.lock().unwrap().push(view));
        (dir, Terminals::new(root, environment), views, on_change)
    }

    #[tokio::test]
    async fn a_command_runs_in_the_workspace_with_the_agents_environment() {
        let (_dir, terminals, views, on_change) = terminals();
        let id = terminals
            .create(
                CreateTerminalRequest::new("s1", "printf \"$GREETING\"; pwd; exit 3")
                    .env(vec![EnvVariable::new("GREETING", "hi\n")]),
                on_change,
            )
            .unwrap();
        let exit = terminals.wait_for_exit(&id).await.unwrap();
        assert_eq!(exit.exit_code, Some(3));
        let output = terminals.output(&id).unwrap();
        assert_eq!(output.output, format!("hi\n{}\n", terminals.root.display()));
        assert!(!output.truncated);
        assert_eq!(output.exit_status.unwrap().exit_code, Some(3));

        terminals.release(&id).unwrap();
        assert_eq!(terminals.output(&id).unwrap_err().code, not_found().code);
        tokio::time::sleep(Duration::from_millis(500)).await;
        let last = views.lock().unwrap().last().cloned().unwrap();
        assert!(last.released && last.output.starts_with("hi\n"));
        assert_eq!(last.exit.unwrap().exit_code, Some(3));
    }

    #[tokio::test]
    async fn arguments_run_without_a_shell_and_output_is_limited() {
        let (_dir, terminals, _views, on_change) = terminals();
        let id = terminals
            .create(
                CreateTerminalRequest::new("s1", "printf")
                    .args(vec!["%s".into(), "0123456789 $HOME".into()])
                    .output_byte_limit(5),
                on_change,
            )
            .unwrap();
        terminals.wait_for_exit(&id).await.unwrap();
        let output = terminals.output(&id).unwrap();
        assert_eq!(output.output, "$HOME");
        assert!(output.truncated);
    }

    #[tokio::test]
    async fn kill_stops_the_whole_group_and_exit_does_not_wait_for_background_work() {
        let (dir, terminals, _views, on_change) = terminals();
        let marker = dir.path().join("late");
        let id = terminals
            .create(
                CreateTerminalRequest::new(
                    "s1",
                    format!("(sleep 1; touch {}) & echo started", marker.display()),
                ),
                on_change.clone(),
            )
            .unwrap();
        // The shell exits at once though its background job still runs.
        let exit = tokio::time::timeout(Duration::from_secs(5), terminals.wait_for_exit(&id))
            .await
            .expect("exit is reported without waiting for background work")
            .unwrap();
        assert_eq!(exit.exit_code, Some(0));
        terminals.kill(&id).unwrap();
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert!(
            !marker.exists(),
            "the background job was stopped with its group"
        );
        assert!(terminals.output(&id).unwrap().output.starts_with("started"));

        let long = terminals
            .create(CreateTerminalRequest::new("s1", "sleep 30"), on_change)
            .unwrap();
        terminals.kill(&long).unwrap();
        let exit = tokio::time::timeout(Duration::from_secs(5), terminals.wait_for_exit(&long))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(exit.signal.as_deref(), Some("SIGKILL"));
    }

    #[tokio::test]
    async fn requests_outside_the_workspace_or_over_limits_are_refused() {
        let (_dir, terminals, _views, on_change) = terminals();
        let outside = tempfile::tempdir().unwrap();
        for request in [
            CreateTerminalRequest::new("s1", "true").cwd(outside.path().to_path_buf()),
            CreateTerminalRequest::new("s1", "true").cwd(PathBuf::from("relative")),
            CreateTerminalRequest::new("s1", "  "),
            CreateTerminalRequest::new("s1", "true").env(vec![EnvVariable::new("A=B", "x")]),
        ] {
            assert_eq!(
                terminals
                    .create(request, on_change.clone())
                    .unwrap_err()
                    .code,
                invalid("").code
            );
        }
        let ids: Vec<_> = (0..MAX_LIVE)
            .map(|_| {
                terminals
                    .create(
                        CreateTerminalRequest::new("s1", "sleep 30"),
                        on_change.clone(),
                    )
                    .unwrap()
            })
            .collect();
        assert!(terminals
            .create(CreateTerminalRequest::new("s1", "true"), on_change.clone())
            .is_err());
        terminals.release(&ids[0]).unwrap();
        assert!(terminals
            .create(CreateTerminalRequest::new("s1", "true"), on_change)
            .is_ok());
        terminals.close_all();
        assert!(terminals.output(&ids[1]).is_err());
    }
}
