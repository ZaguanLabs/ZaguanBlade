//! Direct launches with an explicit environment, process-tree ownership and
//! bounded diagnostics. These are trusted programs, not sandboxed processes.
use super::{
    config::{ConfigValue, ProcessConfig},
    credentials::SecretStore,
    RuntimeError,
};
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncReadExt, task::JoinHandle};
use uuid::Uuid;

const INHERITED_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "TMP",
    "TEMP",
    "TMPDIR",
    "SystemRoot",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "LANG",
    "LC_ALL",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_CACHE_HOME",
    "XDG_RUNTIME_DIR",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
];

pub struct PreparedProcess {
    pub executable: PathBuf,
    pub cwd: PathBuf,
    pub args: Vec<String>,
    env: BTreeMap<OsString, OsString>,
    pub fingerprint: String,
}

impl PreparedProcess {
    pub fn resolve(
        config: &ProcessConfig,
        integration: Uuid,
        workspace: &Path,
        secrets: &dyn SecretStore,
    ) -> Result<Self, RuntimeError> {
        let inherited = INHERITED_ENV
            .iter()
            .filter_map(|key| std::env::var_os(key).map(|value| (OsString::from(key), value)))
            .collect();
        Self::resolve_with_env(config, integration, workspace, secrets, inherited)
    }

    fn resolve_with_env(
        config: &ProcessConfig,
        integration: Uuid,
        workspace: &Path,
        secrets: &dyn SecretStore,
        mut env: BTreeMap<OsString, OsString>,
    ) -> Result<Self, RuntimeError> {
        let cwd = workspace
            .join(config.cwd.as_deref().unwrap_or("."))
            .canonicalize()
            .map_err(|_| RuntimeError::InvalidDirectory)?;
        if !cwd.is_dir() {
            return Err(RuntimeError::InvalidDirectory);
        }
        for (key, value) in &config.env {
            let value = match value {
                ConfigValue::Text { value } => value.clone(),
                ConfigValue::Secret { name } => secrets
                    .get(integration, name)?
                    .ok_or(RuntimeError::SecretMissing)?,
            };
            if value.len() > 8192 || value.contains('\0') {
                return Err(RuntimeError::InvalidSecret);
            }
            env.insert(key.into(), value.into());
        }
        let executable =
            resolve_executable(&config.command, &cwd, env.get(&OsString::from("PATH")))?;
        // Bind approval to the resolved launch, secret values, and executable
        // metadata. No raw environment or digest is returned to the webview.
        let metadata =
            std::fs::metadata(&executable).map_err(|_| RuntimeError::ExecutableNotFound)?;
        let mut bytes = Vec::new();
        for value in std::iter::once(executable.as_os_str())
            .chain(std::iter::once(cwd.as_os_str()))
            .chain(config.args.iter().map(|arg| std::ffi::OsStr::new(arg)))
            .chain(
                env.iter()
                    .flat_map(|(key, value)| [key.as_os_str(), value.as_os_str()]),
            )
        {
            bytes.extend_from_slice(&(value.as_encoded_bytes().len() as u64).to_le_bytes());
            bytes.extend_from_slice(value.as_encoded_bytes());
        }
        bytes.extend_from_slice(&metadata.len().to_le_bytes());
        if let Ok(modified) = metadata.modified().and_then(|time| {
            time.duration_since(std::time::UNIX_EPOCH)
                .map_err(std::io::Error::other)
        }) {
            bytes.extend_from_slice(&modified.as_nanos().to_le_bytes());
        }
        Ok(Self {
            executable,
            cwd,
            args: config.args.clone(),
            env,
            fingerprint: super::store::fingerprint(&bytes),
        })
    }
}

fn resolve_executable(
    command: &str,
    cwd: &Path,
    search_path: Option<&OsString>,
) -> Result<PathBuf, RuntimeError> {
    let path = Path::new(command);
    let candidates = if path.is_absolute() || path.components().count() > 1 {
        vec![cwd.join(path)]
    } else {
        search_path
            .map(|value| {
                std::env::split_paths(value)
                    .filter(|dir| dir.is_absolute())
                    .map(|dir| dir.join(path))
                    .collect()
            })
            .unwrap_or_default()
    };
    for candidate in candidates {
        #[cfg(windows)]
        let candidate = if candidate.extension().is_none() {
            candidate.with_extension("exe")
        } else {
            candidate
        };
        #[cfg(windows)]
        if !candidate
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
        {
            continue;
        }
        let Ok(resolved) = candidate.canonicalize() else {
            continue;
        };
        let Ok(meta) = resolved.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        return Ok(resolved);
    }
    Err(RuntimeError::ExecutableNotFound)
}

pub struct SupervisedProcess {
    child: Option<Box<dyn ChildWrapper>>,
    stderr: JoinHandle<()>,
}

impl SupervisedProcess {
    pub fn spawn(
        prepared: &PreparedProcess,
    ) -> Result<
        (
            Self,
            tokio::process::ChildStdout,
            tokio::process::ChildStdin,
        ),
        RuntimeError,
    > {
        let mut command = CommandWrap::with_new(&prepared.executable, |command| {
            command
                .args(&prepared.args)
                .current_dir(&prepared.cwd)
                .env_clear()
                .envs(&prepared.env)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        });
        #[cfg(unix)]
        command.wrap(process_wrap::tokio::ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(process_wrap::tokio::JobObject);
        command.wrap(KillOnDrop);
        let mut child = command.spawn().map_err(|_| RuntimeError::LaunchFailed)?;
        // Pipes are guaranteed by the command above. Own the child before taking
        // them so even an unexpected missing pipe follows the same cleanup path.
        let mut stderr = child.stderr().take();
        let stderr = tokio::spawn(async move {
            if let Some(ref mut stream) = stderr {
                let mut buffer = [0; 8192];
                while matches!(stream.read(&mut buffer).await, Ok(n) if n > 0) {}
            }
        });
        let mut process = Self {
            child: Some(child),
            stderr,
        };
        let child = process.child.as_mut().ok_or(RuntimeError::LaunchFailed)?;
        let stdout = child.stdout().take().ok_or(RuntimeError::LaunchFailed)?;
        let stdin = child.stdin().take().ok_or(RuntimeError::LaunchFailed)?;
        Ok((process, stdout, stdin))
    }

    /// Call after closing protocol stdin. Give the server a short opportunity to
    /// flush, then terminate/reap the owned process tree even if only descendants remain.
    pub async fn shutdown_gracefully(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = tokio::time::timeout(Duration::from_millis(750), child.wait()).await;
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
        self.stderr.abort();
    }

    pub async fn shutdown(mut self) {
        self.stderr.abort();
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
        }
    }
}

impl Drop for SupervisedProcess {
    fn drop(&mut self) {
        self.stderr.abort();
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn(async move {
                    let _ = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
                });
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    struct NoSecrets;
    impl SecretStore for NoSecrets {
        fn get(&self, _: Uuid, _: &str) -> Result<Option<String>, RuntimeError> {
            Ok(None)
        }
    }
    fn config() -> ProcessConfig {
        ProcessConfig {
            command: "/bin/sh".into(),
            args: vec![],
            cwd: None,
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn resolution_preserves_arguments_and_rejects_missing_secrets() {
        let root = tempfile::tempdir().unwrap();
        let mut config = config();
        config.args = vec!["space and ; $(literal)".into(), "".into()];
        let process = PreparedProcess::resolve_with_env(
            &config,
            Uuid::new_v4(),
            root.path(),
            &NoSecrets,
            BTreeMap::new(),
        )
        .unwrap();
        assert_eq!(process.args, config.args);
        assert!(process.env.is_empty());
        config.env.insert(
            "TOKEN".into(),
            ConfigValue::Secret {
                name: "token".into(),
            },
        );
        assert!(matches!(
            PreparedProcess::resolve(&config, Uuid::new_v4(), root.path(), &NoSecrets),
            Err(RuntimeError::SecretMissing)
        ));
    }

    #[test]
    fn relative_path_entries_do_not_select_workspace_executables() {
        let root = tempfile::tempdir().unwrap();
        let mut config = config();
        config.command = "sh".into();
        let env = [(OsString::from("PATH"), OsString::from(".:relative"))].into();
        assert!(matches!(
            PreparedProcess::resolve_with_env(
                &config,
                Uuid::new_v4(),
                root.path(),
                &NoSecrets,
                env
            ),
            Err(RuntimeError::ExecutableNotFound)
        ));
        assert!(!INHERITED_ENV.contains(&"BLADE_API_KEY"));
        assert!(!INHERITED_ENV.contains(&"LD_LIBRARY_PATH"));
    }

    #[tokio::test]
    async fn shutdown_and_drop_close_descendant_stdout() {
        for explicit_shutdown in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let mut config = config();
            config.args = vec!["-c".into(), "sleep 60 & printf ready; wait".into()];
            let prepared =
                PreparedProcess::resolve(&config, Uuid::new_v4(), root.path(), &NoSecrets).unwrap();
            let (process, mut stdout, _stdin) = SupervisedProcess::spawn(&prepared).unwrap();
            let mut ready = [0; 5];
            tokio::time::timeout(Duration::from_secs(3), stdout.read_exact(&mut ready))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(&ready, b"ready");
            if explicit_shutdown {
                process.shutdown().await;
            } else {
                drop(process);
            }
            let mut remainder = Vec::new();
            tokio::time::timeout(Duration::from_secs(3), stdout.read_to_end(&mut remainder))
                .await
                .expect("descendant retained pipe after cleanup")
                .unwrap();
        }
    }
}
