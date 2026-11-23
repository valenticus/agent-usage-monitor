use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::{Value, json};
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::models::{ProviderError, RateLimitSnapshot, RateLimitWindow};

/// `CREATE_NO_WINDOW` process creation flag (winbase.h).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
use crate::provider::RateLimitProvider;

static APP_SERVER_GATE: OnceLock<Arc<AsyncMutex<()>>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct CodexClientConfig {
    pub executable: String,
    pub request_timeout: Duration,
    pub cleanup_timeout: Duration,
}

impl Default for CodexClientConfig {
    fn default() -> Self {
        Self {
            executable: "codex".into(),
            request_timeout: Duration::from_secs(15),
            cleanup_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Error)]
enum ClientError {
    #[error("Codex CLI is unavailable on PATH")]
    NotFound,
    #[error("Codex app-server request timed out")]
    Timeout,
    #[error("Codex app-server request was cancelled")]
    Cancelled,
    #[error("Codex app-server process failed")]
    Process,
    #[error("Codex app-server could not be started")]
    Spawn,
    #[error("Codex app-server executable was not found")]
    SpawnNotFound,
    #[error("Codex app-server executable could not be accessed")]
    SpawnDenied,
    #[error("Codex app-server could not be started with this command")]
    SpawnInvalidInput,
    #[error("Codex app-server could not be created (OS error {0})")]
    SpawnOsError(i32),
    #[error("Codex app-server cleanup failed")]
    Cleanup,
    #[error("Codex app-server returned an invalid response")]
    Protocol,
}

impl From<ClientError> for ProviderError {
    fn from(value: ClientError) -> Self {
        let code = match value {
            ClientError::NotFound => "codex_unavailable",
            ClientError::Timeout => "codex_timeout",
            ClientError::Cancelled => "codex_cancelled",
            ClientError::Process => "codex_process_error",
            ClientError::Spawn => "codex_spawn_error",
            ClientError::SpawnNotFound => "codex_spawn_not_found",
            ClientError::SpawnDenied => "codex_spawn_denied",
            ClientError::SpawnInvalidInput => "codex_spawn_invalid_input",
            ClientError::SpawnOsError(_) => "codex_spawn_os_error",
            ClientError::Cleanup => "codex_cleanup_error",
            ClientError::Protocol => "codex_protocol_error",
        };
        let code = match value {
            ClientError::SpawnOsError(code) => format!("codex_spawn_os_{code}"),
            _ => code.into(),
        };
        Self {
            code,
            message: value.to_string(),
        }
    }
}

#[derive(Clone)]
pub struct CodexAppServerClient {
    config: CodexClientConfig,
    gate: Arc<AsyncMutex<()>>,
}

impl CodexAppServerClient {
    pub fn new(config: CodexClientConfig) -> Self {
        Self {
            config,
            gate: APP_SERVER_GATE
                .get_or_init(|| Arc::new(AsyncMutex::new(())))
                .clone(),
        }
    }

    pub async fn read_rate_limits(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<RateLimitSnapshot>, ProviderError> {
        let _guard = tokio::select! {
            _ = cancellation.cancelled() => return Err(ClientError::Cancelled.into()),
            guard = self.gate.lock() => guard,
        };
        self.read_inner(cancellation).await.map_err(Into::into)
    }

    async fn read_inner(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<RateLimitSnapshot>, ClientError> {
        let executable =
            resolve_executable(&self.config.executable).ok_or(ClientError::NotFound)?;
        let mut command = app_server_command(&executable)?;
        // The release UI is a GUI-subsystem process without a console: keep
        // the console helper (cmd/powershell/codex) from opening a window.
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => ClientError::SpawnNotFound,
                std::io::ErrorKind::PermissionDenied => ClientError::SpawnDenied,
                std::io::ErrorKind::InvalidInput => ClientError::SpawnInvalidInput,
                _ => error
                    .raw_os_error()
                    .map(ClientError::SpawnOsError)
                    .unwrap_or(ClientError::Spawn),
            })?;
        let operation = tokio::select! {
            _ = cancellation.cancelled() => Err(ClientError::Cancelled),
            result = timeout(self.config.request_timeout, Self::exchange(&mut child)) => match result {
                Ok(result) => result,
                Err(_) => Err(ClientError::Timeout),
            }
        };
        let cleanup = timeout(self.config.cleanup_timeout, terminate_child(&mut child)).await;
        match cleanup {
            Ok(Ok(())) => operation,
            _ => Err(ClientError::Cleanup),
        }
    }

    async fn exchange(child: &mut Child) -> Result<Vec<RateLimitSnapshot>, ClientError> {
        let mut input = child.stdin.take().ok_or(ClientError::Process)?;
        let stdout = child.stdout.take().ok_or(ClientError::Process)?;
        let mut output = BufReader::new(stdout);

        write_message(&mut input, json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": { "name": "agent-usage-monitor", "title": "Agent Usage Monitor", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": { "experimentalApi": true }
            }
        })).await?;
        let _ = read_response(&mut output, 1).await?;
        write_message(&mut input, json!({"method":"initialized"})).await?;
        write_message(
            &mut input,
            json!({
                "id": 2,
                "method": "account/rateLimits/read",
                "params": { "excludeResetCreditDetails": true }
            }),
        )
        .await?;
        let result = read_response(&mut output, 2).await?;
        parse_codex_limits(&result)
    }
}

#[derive(Clone)]
pub struct CodexRateLimitProvider {
    client: CodexAppServerClient,
}

impl CodexRateLimitProvider {
    pub fn new(client: CodexAppServerClient) -> Self {
        Self { client }
    }
}

impl RateLimitProvider for CodexRateLimitProvider {
    fn id(&self) -> &'static str {
        "codex"
    }

    async fn read_rate_limits(
        &self,
        cancellation: CancellationToken,
    ) -> Result<Vec<RateLimitSnapshot>, ProviderError> {
        self.client.read_rate_limits(cancellation).await
    }
}

const APP_SERVER_ARGUMENTS: [&str; 3] = ["app-server", "--listen", "stdio://"];

/// The command for the resolved Codex CLI:
///
/// * `.exe` (native, also the npm package's bundled binary): started directly.
/// * `.ps1`: Windows PowerShell from the system directory with `-File`.
/// * `.cmd` (npm shim): `cmd.exe` from the system directory with a verbatim
///   command line ([`crate::process_tools::cmd_shim_command_line`]); paths
///   cmd.exe cannot pass safely are refused and nothing is started.
fn app_server_command(executable: &str) -> Result<Command, ClientError> {
    #[cfg(windows)]
    {
        let path = Path::new(executable);
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::to_ascii_lowercase);
        match extension.as_deref() {
            Some("ps1") => {
                let mut command = Command::new(
                    crate::process_tools::system_tool_path(
                        r"WindowsPowerShell\v1.0\powershell.exe",
                    )
                    .map_err(|_| ClientError::Spawn)?,
                );
                command.args([
                    "-NoLogo",
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "Bypass",
                    "-File",
                ]);
                command.arg(executable);
                command.args(APP_SERVER_ARGUMENTS);
                Ok(command)
            }
            Some("cmd") => {
                use std::os::windows::process::CommandExt;
                let line = crate::process_tools::cmd_shim_command_line(path, &APP_SERVER_ARGUMENTS)
                    .map_err(|_| ClientError::SpawnInvalidInput)?;
                let mut command = Command::new(
                    crate::process_tools::system_tool_path("cmd.exe")
                        .map_err(|_| ClientError::Spawn)?,
                );
                command.as_std_mut().raw_arg(line);
                Ok(command)
            }
            _ => {
                let mut command = Command::new(executable);
                command.args(APP_SERVER_ARGUMENTS);
                Ok(command)
            }
        }
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new(executable);
        command.args(APP_SERVER_ARGUMENTS);
        Ok(command)
    }
}

fn resolve_executable(name: &str) -> Option<String> {
    resolve_executable_in(std::env::var_os("PATH").as_deref(), name)
}

/// An explicit path must be absolute. A bare name is searched in the
/// **absolute** entries of `path_variable` only; relative or empty entries
/// (including `.`) are skipped, so the current directory is never searched.
fn resolve_executable_in(path_variable: Option<&std::ffi::OsStr>, name: &str) -> Option<String> {
    let candidate = Path::new(name);
    if candidate.components().count() > 1 {
        return (candidate.is_absolute() && candidate.is_file()).then(|| name.to_owned());
    }
    std::env::split_paths(path_variable?)
        .filter(|directory| directory.is_absolute())
        .find_map(|directory| resolve_executable_in_dir(&directory, name))
}

fn resolve_executable_in_dir(dir: &Path, name: &str) -> Option<String> {
    #[cfg(not(windows))]
    {
        let path = dir.join(name);
        if path.is_file() {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    #[cfg(windows)]
    {
        let exe = dir.join(format!("{name}.exe"));
        if exe.is_file() {
            return Some(exe.to_string_lossy().into_owned());
        }
        let cmd = dir.join(format!("{name}.cmd"));
        if cmd.is_file() {
            if name == "codex" {
                if let Some(native) = resolve_npm_codex_binary(dir) {
                    return Some(native);
                }
            }
            return Some(cmd.to_string_lossy().into_owned());
        }
        let ps1 = dir.join(format!("{name}.ps1"));
        if ps1.is_file() {
            return Some(ps1.to_string_lossy().into_owned());
        }
    }
    None
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn resolve_npm_codex_binary(npm_dir: &Path) -> Option<String> {
    let path = npm_dir.join(
        "node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe",
    );
    path.is_file().then(|| path.to_string_lossy().into_owned())
}

#[cfg(all(windows, target_arch = "aarch64"))]
fn resolve_npm_codex_binary(npm_dir: &Path) -> Option<String> {
    let path = npm_dir.join(
        "node_modules/@openai/codex/node_modules/@openai/codex-win32-arm64/vendor/aarch64-pc-windows-msvc/bin/codex.exe",
    );
    path.is_file().then(|| path.to_string_lossy().into_owned())
}

#[cfg(all(windows, not(any(target_arch = "x86_64", target_arch = "aarch64"))))]
fn resolve_npm_codex_binary(_npm_dir: &Path) -> Option<String> {
    None
}

async fn terminate_child(child: &mut Child) -> Result<(), ClientError> {
    match child.try_wait().map_err(|_| ClientError::Process)? {
        Some(_) => Ok(()),
        None => {
            #[cfg(windows)]
            if let Some(pid) = child.id() {
                let pid = pid.to_string();
                // Without a verified taskkill path, only the direct child is
                // killed below; a bare "taskkill.exe" is never started.
                let succeeded = match crate::process_tools::system_tool_path("taskkill.exe") {
                    Ok(taskkill) => Command::new(taskkill)
                        .creation_flags(CREATE_NO_WINDOW)
                        .args(["/PID", &pid, "/T", "/F"])
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .await
                        .is_ok_and(|status| status.success()),
                    Err(_) => false,
                };
                if !succeeded {
                    child.kill().await.map_err(|_| ClientError::Process)?;
                }
                child.wait().await.map_err(|_| ClientError::Process)?;
                return Ok(());
            }
            #[cfg(unix)]
            if let Some(pid) = child.id() {
                let process_group = format!("-{pid}");
                let result = Command::new("kill")
                    .args(["-KILL", "--", &process_group])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await;
                if !result.is_ok_and(|status| status.success()) {
                    child.kill().await.map_err(|_| ClientError::Process)?;
                }
                child.wait().await.map_err(|_| ClientError::Process)?;
                return Ok(());
            }
            child.kill().await.map_err(|_| ClientError::Process)?;
            child.wait().await.map_err(|_| ClientError::Process)?;
            Ok(())
        }
    }
}

async fn write_message(
    input: &mut tokio::process::ChildStdin,
    message: Value,
) -> Result<(), ClientError> {
    let bytes = serde_json::to_vec(&message).map_err(|_| ClientError::Protocol)?;
    input
        .write_all(&bytes)
        .await
        .map_err(|_| ClientError::Process)?;
    input
        .write_all(b"\n")
        .await
        .map_err(|_| ClientError::Process)?;
    input.flush().await.map_err(|_| ClientError::Process)
}

async fn read_response(
    output: &mut (impl tokio::io::AsyncBufRead + Unpin),
    expected_id: u64,
) -> Result<Value, ClientError> {
    loop {
        let mut line = String::new();
        if output
            .read_line(&mut line)
            .await
            .map_err(|_| ClientError::Process)?
            == 0
        {
            return Err(ClientError::Process);
        }
        let value: Value = serde_json::from_str(&line).map_err(|_| ClientError::Protocol)?;
        let Some(id) = value.get("id") else { continue };
        if id.as_u64() != Some(expected_id) {
            return Err(ClientError::Protocol);
        }
        if value.get("error").is_some() {
            return Err(ClientError::Protocol);
        }
        return value.get("result").cloned().ok_or(ClientError::Protocol);
    }
}

fn parse_codex_limits(result: &Value) -> Result<Vec<RateLimitSnapshot>, ClientError> {
    let mut limits = Vec::new();
    if let Some(by_id) = result.get("rateLimitsByLimitId").and_then(Value::as_object) {
        for (id, value) in by_id {
            limits.push(parse_snapshot(value, Some(id.clone()))?);
        }
    } else if let Some(value) = result.get("rateLimits") {
        limits.push(parse_snapshot(value, None)?);
    } else {
        return Err(ClientError::Protocol);
    }
    if limits.is_empty() {
        return Err(ClientError::Protocol);
    }
    Ok(limits)
}

fn parse_snapshot(
    value: &Value,
    fallback_id: Option<String>,
) -> Result<RateLimitSnapshot, ClientError> {
    if !value.is_object() {
        return Err(ClientError::Protocol);
    }
    let window = |name: &str| -> Result<Option<RateLimitWindow>, ClientError> {
        let Some(value) = value.get(name) else {
            return Ok(None);
        };
        if value.is_null() {
            return Ok(None);
        }
        let used = value
            .get("usedPercent")
            .and_then(Value::as_f64)
            .ok_or(ClientError::Protocol)?;
        if !used.is_finite() {
            return Err(ClientError::Protocol);
        }
        Ok(Some(RateLimitWindow {
            used_percent: used,
            window_duration_mins: value.get("windowDurationMins").and_then(Value::as_u64),
            resets_at: value.get("resetsAt").and_then(Value::as_i64),
        }))
    };
    let primary = window("primary")?;
    let secondary = window("secondary")?;
    if primary.is_none() && secondary.is_none() {
        return Err(ClientError::Protocol);
    }
    let text = |key: &str| -> Result<Option<String>, ClientError> {
        match value.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s.clone())),
            _ => Err(ClientError::Protocol),
        }
    };
    Ok(RateLimitSnapshot {
        limit_id: text("limitId")?.or(fallback_id),
        limit_name: text("limitName")?,
        primary,
        secondary,
        plan_type: text("planType")?,
        rate_limit_reached_type: text("rateLimitReachedType")?,
    })
}

#[cfg(test)]
mod tests {
    use super::{CodexAppServerClient, CodexClientConfig, parse_codex_limits};
    use std::time::Duration;
    use tokio_util::sync::CancellationToken;

    struct MockServer {
        _directory: tempfile::TempDir,
        executable: String,
        pid_file: std::path::PathBuf,
        descendant_file: std::path::PathBuf,
        #[cfg_attr(not(windows), allow(dead_code))]
        args_file: std::path::PathBuf,
    }

    impl MockServer {
        fn new(mode: &str) -> Self {
            Self::in_folder(mode, None)
        }

        /// Windows: the PowerShell mock behind an npm-style `codex.cmd` shim in
        /// a folder whose name contains spaces and cmd.exe metacharacters.
        #[cfg(windows)]
        fn behind_cmd_shim(mode: &str, folder: &str) -> Self {
            Self::in_folder(mode, Some(folder))
        }

        fn in_folder(mode: &str, shim_folder: Option<&str>) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let pid_file = directory.path().join("child.pid");
            let descendant_file = directory.path().join("descendant.pid");
            let args_file = directory.path().join("args.txt");
            let script_directory = match shim_folder {
                Some(folder) => directory.path().join(folder),
                None => directory.path().to_path_buf(),
            };
            std::fs::create_dir_all(&script_directory).unwrap();
            #[cfg(windows)]
            let (extension, script) = (
                "ps1",
                format!(
                    "$ErrorActionPreference='Stop'\n[IO.File]::WriteAllText('{}', [string]$PID)\n[IO.File]::WriteAllText('{}', ($args -join '|'))\n$init=[Console]::ReadLine()\nif ('{}' -eq 'exit') {{ exit }}\nif ('{}' -eq 'malformed') {{ [Console]::WriteLine('not-json'); exit }}\n[Console]::WriteLine('{{\"id\":1,\"result\":{{}}}}')\n$notice=[Console]::ReadLine()\n$limits=[Console]::ReadLine()\nif ('{}' -eq 'timeout') {{ $desc=Start-Process $env:ComSpec -WindowStyle Hidden -ArgumentList '/c','ping','-n','31','127.0.0.1' -PassThru; [IO.File]::WriteAllText('{}', [string]$desc.Id); Start-Sleep -Seconds 30; exit }}\nif ('{}' -eq 'wrong_id') {{ [Console]::WriteLine('{{\"id\":99,\"result\":{{}}}}'); exit }}\n[Console]::WriteLine('{{\"id\":2,\"result\":{{\"rateLimits\":{{\"primary\":{{\"usedPercent\":23,\"windowDurationMins\":300}}}}}}}}')\n",
                    pid_file.display(),
                    args_file.display(),
                    mode,
                    mode,
                    mode,
                    descendant_file.display(),
                    mode
                ),
            );
            #[cfg(unix)]
            let (extension, script) = (
                "sh",
                format!(
                    "#!/bin/sh\necho $$ > '{}'\nIFS= read -r init || exit 0\nif [ '{}' = exit ]; then exit 0; fi\nif [ '{}' = malformed ]; then echo not-json; exit 0; fi\necho '{{\"id\":1,\"result\":{{}}}}'\nIFS= read -r notice || exit 0\nIFS= read -r limits || exit 0\nif [ '{}' = timeout ]; then sleep 30 & echo $! > '{}'; wait; fi\nif [ '{}' = wrong_id ]; then echo '{{\"id\":99,\"result\":{{}}}}'; exit 0; fi\necho '{{\"id\":2,\"result\":{{\"rateLimits\":{{\"primary\":{{\"usedPercent\":23,\"windowDurationMins\":300}}}}}}}}'\n",
                    pid_file.display(),
                    mode,
                    mode,
                    mode,
                    descendant_file.display(),
                    mode
                ),
            );
            #[cfg_attr(not(windows), allow(unused_mut))]
            let mut executable_path = script_directory.join(format!("mock-server.{extension}"));
            std::fs::write(&executable_path, script).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut permissions = std::fs::metadata(&executable_path).unwrap().permissions();
                permissions.set_mode(0o700);
                std::fs::set_permissions(&executable_path, permissions).unwrap();
            }
            #[cfg(windows)]
            if shim_folder.is_some() {
                // Same shape as npm's generated shims: forward all arguments.
                let shim = script_directory.join("codex.cmd");
                std::fs::write(
                    &shim,
                    "@echo off\r\n\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"%~dp0mock-server.ps1\" %*\r\n",
                )
                .unwrap();
                executable_path = shim;
            }
            Self {
                _directory: directory,
                executable: executable_path.to_string_lossy().into_owned(),
                pid_file,
                descendant_file,
                args_file,
            }
        }

        #[cfg(windows)]
        fn received_arguments(&self) -> String {
            std::fs::read_to_string(&self.args_file).unwrap_or_default()
        }

        fn config(&self, request_timeout: Duration) -> CodexClientConfig {
            CodexClientConfig {
                executable: self.executable.clone(),
                request_timeout,
                cleanup_timeout: Duration::from_secs(2),
            }
        }

        async fn assert_child_exited(&self) {
            for _ in 0..100 {
                if let Ok(pid) = std::fs::read_to_string(&self.pid_file) {
                    let child_stopped = !Self::pid_is_alive(pid.trim()).await;
                    let descendant_stopped = match std::fs::read_to_string(&self.descendant_file) {
                        Ok(descendant) => !Self::pid_is_alive(descendant.trim()).await,
                        Err(_) => true,
                    };
                    if child_stopped && descendant_stopped {
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            panic!("mock app-server or descendant process did not exit");
        }

        async fn pid_is_alive(pid: &str) -> bool {
            #[cfg(windows)]
            {
                let output = tokio::process::Command::new("tasklist")
                    .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
                    .output()
                    .await
                    .unwrap()
                    .stdout;
                String::from_utf8_lossy(&output).lines().any(|line| {
                    line.split(',')
                        .nth(1)
                        .is_some_and(|field| field.trim_matches('"') == pid)
                })
            }
            #[cfg(unix)]
            {
                tokio::process::Command::new("kill")
                    .args(["-0", pid])
                    .status()
                    .await
                    .map(|status| status.success())
                    .unwrap_or(false)
            }
        }

        /// Time-based budget: PowerShell start-up can take many seconds when
        /// several mock servers run in parallel on a loaded or battery-saving
        /// machine.
        async fn wait_until_descendant_started(&self) {
            let deadline = std::time::Instant::now() + Duration::from_secs(40);
            while std::time::Instant::now() < deadline {
                if self.descendant_file.is_file() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            panic!("mock app-server descendant did not start");
        }
    }
    #[test]
    fn parses_normalized_windows_and_validates_shapes() {
        let result = serde_json::json!({"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":21,"windowDurationMins":300,"resetsAt":1700000000}}}});
        let limits = parse_codex_limits(&result).unwrap();
        assert_eq!(limits[0].primary.as_ref().unwrap().used_percent, 21.0);
        assert_eq!(
            limits[0].primary.as_ref().unwrap().window_duration_mins,
            Some(300)
        );
        assert!(parse_codex_limits(&serde_json::json!({"rateLimits":{}})).is_err());
    }

    #[tokio::test]
    async fn reports_missing_cli_without_running_a_real_process() {
        let config = CodexClientConfig {
            executable: "definitely-not-a-real-codex-command".into(),
            request_timeout: Duration::from_millis(30),
            cleanup_timeout: Duration::from_secs(1),
        };
        let client = CodexAppServerClient::new(config);
        let error = client
            .read_rate_limits(CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "codex_unavailable");
    }

    #[cfg(all(windows, target_arch = "x86_64"))]
    #[test]
    fn resolves_official_npm_native_codex_before_cmd_shim() {
        let directory = tempfile::tempdir().unwrap();
        let native = directory.path().join(
            "node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe",
        );
        std::fs::create_dir_all(native.parent().unwrap()).unwrap();
        std::fs::write(directory.path().join("codex.cmd"), "shim").unwrap();
        std::fs::write(&native, "binary").unwrap();
        assert_eq!(
            super::resolve_executable_in_dir(directory.path(), "codex"),
            Some(native.to_string_lossy().into_owned())
        );
    }

    #[cfg(windows)]
    #[test]
    fn ignores_extensionless_script_shims_on_windows() {
        let directory = tempfile::tempdir().unwrap();
        let extensionless = directory.path().join("codex");
        let command_shim = directory.path().join("codex.cmd");
        std::fs::write(extensionless, "not a Windows executable").unwrap();
        std::fs::write(&command_shim, "shim").unwrap();
        assert_eq!(
            super::resolve_executable_in_dir(directory.path(), "codex"),
            Some(command_shim.to_string_lossy().into_owned())
        );
    }

    #[tokio::test]
    async fn rpc_response_matching_ignores_notifications_but_rejects_wrong_ids() {
        let input = "{\"method\":\"notice\"}\n{\"id\":1,\"result\":{\"ok\":true}}\n";
        let mut lines = tokio::io::BufReader::new(input.as_bytes());
        let result = super::read_response(&mut lines, 1).await.unwrap();
        assert_eq!(result["ok"], true);
        let wrong_id = "{\"id\":2,\"result\":{}}\n";
        let mut lines = tokio::io::BufReader::new(wrong_id.as_bytes());
        assert!(super::read_response(&mut lines, 1).await.is_err());
    }

    #[tokio::test]
    async fn completes_successful_json_rpc_exchange_and_cleans_child() {
        let mock = MockServer::new("ok");
        let client = CodexAppServerClient::new(mock.config(Duration::from_secs(30)));
        let limits = client
            .read_rate_limits(CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(limits[0].primary.as_ref().unwrap().used_percent, 23.0);
        mock.assert_child_exited().await;
    }

    #[tokio::test]
    async fn timeout_kills_child_and_returns_timeout_error() {
        let mock = MockServer::new("timeout");
        let client = CodexAppServerClient::new(mock.config(Duration::from_secs(20)));
        let error = client
            .read_rate_limits(CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "codex_timeout");
        assert!(mock.descendant_file.is_file());
        mock.assert_child_exited().await;
    }

    #[tokio::test]
    async fn cancellation_kills_child_and_returns_cancelled_error() {
        let mock = MockServer::new("timeout");
        let client = CodexAppServerClient::new(mock.config(Duration::from_secs(45)));
        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();
        let task = tokio::spawn(async move { client.read_rate_limits(trigger).await });
        mock.wait_until_descendant_started().await;
        cancellation.cancel();
        assert_eq!(task.await.unwrap().unwrap_err().code, "codex_cancelled");
        mock.assert_child_exited().await;
    }

    /// Folder names with spaces and characters cmd.exe treats specially
    /// outside quotes (`&`, `^`, `(`, `)`, `!`).
    #[cfg(windows)]
    const SHIM_FOLDERS: [&str; 2] = ["npm global (x86)", "a & b ^ c ! d"];

    #[cfg(windows)]
    #[tokio::test]
    async fn cmd_shim_runs_with_exact_arguments_in_unusual_folders() {
        for folder in SHIM_FOLDERS {
            let mock = MockServer::behind_cmd_shim("ok", folder);
            let client = CodexAppServerClient::new(mock.config(Duration::from_secs(30)));
            let limits = client
                .read_rate_limits(CancellationToken::new())
                .await
                .unwrap_or_else(|error| panic!("folder={folder}: {error:?}"));
            assert_eq!(limits[0].primary.as_ref().unwrap().used_percent, 23.0);
            assert_eq!(
                mock.received_arguments(),
                "app-server|--listen|stdio://",
                "folder={folder}"
            );
            mock.assert_child_exited().await;
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn cmd_shim_paths_cmd_cannot_pass_are_refused_without_a_process() {
        let mock = MockServer::behind_cmd_shim("ok", "100% npm");
        let client = CodexAppServerClient::new(mock.config(Duration::from_secs(5)));
        let error = client
            .read_rate_limits(CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "codex_spawn_invalid_input");
        assert!(!mock.pid_file.exists(), "no process may be started");
    }

    #[cfg(windows)]
    #[test]
    fn native_exe_and_script_commands_keep_their_arguments() {
        let native = super::app_server_command(r"C:\tools\codex.exe").unwrap();
        assert_eq!(native.as_std().get_program(), r"C:\tools\codex.exe");
        assert_eq!(
            native.as_std().get_args().collect::<Vec<_>>(),
            ["app-server", "--listen", "stdio://"]
        );
        let script = super::app_server_command(r"C:\npm dir\codex.ps1").unwrap();
        let program = script
            .as_std()
            .get_program()
            .to_string_lossy()
            .to_lowercase();
        assert!(
            program.ends_with(r"\windowspowershell\v1.0\powershell.exe"),
            "{program}"
        );
        assert_eq!(
            script.as_std().get_args().collect::<Vec<_>>(),
            [
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
                r"C:\npm dir\codex.ps1",
                "app-server",
                "--listen",
                "stdio://"
            ]
        );
        let shim = super::app_server_command(r"C:\npm (x86) & co\codex.cmd").unwrap();
        let program = shim.as_std().get_program().to_string_lossy().to_lowercase();
        assert!(program.ends_with(r"\system32\cmd.exe"), "{program}");
    }

    #[test]
    fn resolution_uses_absolute_path_entries_and_absolute_explicit_paths_only() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin with space & (x86)");
        std::fs::create_dir_all(&bin).unwrap();
        #[cfg(windows)]
        let file = bin.join("codex.exe");
        #[cfg(not(windows))]
        let file = bin.join("codex");
        std::fs::write(&file, b"").unwrap();
        let expected = Some(file.to_string_lossy().into_owned());
        let path = std::env::join_paths([".", "", "relative-bin", bin.to_str().unwrap()]).unwrap();
        // Relative and empty entries are skipped; the absolute one is found.
        assert_eq!(super::resolve_executable_in(Some(&path), "codex"), expected);
        let relative_only = std::env::join_paths([".", "", "relative-bin"]).unwrap();
        assert_eq!(
            super::resolve_executable_in(Some(&relative_only), "codex"),
            None
        );
        assert_eq!(super::resolve_executable_in(None, "codex"), None);
        // Explicit paths: absolute accepted, relative refused.
        assert_eq!(
            super::resolve_executable_in(None, &file.to_string_lossy()),
            expected
        );
        assert_eq!(
            super::resolve_executable_in(
                None,
                &format!("relative-bin{}codex", std::path::MAIN_SEPARATOR)
            ),
            None
        );
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn cmd_shim_timeout_and_cancellation_clean_up_the_process_tree() {
        let mock = MockServer::behind_cmd_shim("timeout", SHIM_FOLDERS[1]);
        let client = CodexAppServerClient::new(mock.config(Duration::from_secs(20)));
        let error = client
            .read_rate_limits(CancellationToken::new())
            .await
            .unwrap_err();
        assert_eq!(error.code, "codex_timeout");
        assert!(mock.descendant_file.is_file());
        mock.assert_child_exited().await;

        let mock = MockServer::behind_cmd_shim("timeout", SHIM_FOLDERS[0]);
        let client = CodexAppServerClient::new(mock.config(Duration::from_secs(45)));
        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();
        let task = tokio::spawn(async move { client.read_rate_limits(trigger).await });
        mock.wait_until_descendant_started().await;
        cancellation.cancel();
        assert_eq!(task.await.unwrap().unwrap_err().code, "codex_cancelled");
        mock.assert_child_exited().await;
    }

    #[tokio::test]
    async fn malformed_response_and_unexpected_exit_are_errors_with_cleanup() {
        for (mode, expected) in [
            ("malformed", "codex_protocol_error"),
            ("exit", "codex_process_error"),
            ("wrong_id", "codex_protocol_error"),
        ] {
            let mock = MockServer::new(mode);
            let client = CodexAppServerClient::new(mock.config(Duration::from_secs(30)));
            assert_eq!(
                client
                    .read_rate_limits(CancellationToken::new())
                    .await
                    .unwrap_err()
                    .code,
                expected,
                "mode={mode}"
            );
            mock.assert_child_exited().await;
        }
    }
}
