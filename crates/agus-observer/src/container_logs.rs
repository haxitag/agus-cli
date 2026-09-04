use agus_ssh::{SshClient, SshOutputStream, SshTarget};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerLogEntry {
    pub container_id: String,
    pub container_name: String,
    pub timestamp: u64,
    pub level: LogLevel,
    pub message: String,
    pub stream: LogStream,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
    Debug,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

pub trait ContainerLogMonitor: Send + Sync {
    /// Start background follow (`docker logs -f`) for a container.
    fn start_monitoring(
        &self,
        container_id: &str,
        since: Option<u64>,
        until: Option<u64>,
    ) -> Result<(), ContainerLogError>;

    /// Stop background follow for a container.
    fn stop_monitoring(&self, container_id: &str) -> Result<(), ContainerLogError>;

    /// Get recent logs for a container (one-shot).
    fn get_logs(
        &self,
        container_id: &str,
        lines: Option<usize>,
        since: Option<u64>,
    ) -> Result<Vec<ContainerLogEntry>, ContainerLogError>;

    /// Bounded follow (≤ ~20s). Prefer `start_monitoring` + `poll_monitoring` for long sessions.
    fn stream_logs<F>(&self, container_id: &str, callback: F) -> Result<(), ContainerLogError>
    where
        F: Fn(ContainerLogEntry) + Send + Sync + 'static;

    /// Drain buffered lines from an active `start_monitoring` session.
    fn poll_monitoring(&self, container_id: &str) -> Result<Vec<ContainerLogEntry>, ContainerLogError>;

    /// Whether a background follow session is active for this container.
    fn is_monitoring(&self, container_id: &str) -> bool;
}

#[derive(Debug)]
pub enum ContainerLogError {
    SshError { message: String },
    ContainerNotFound { container_id: String },
    ParseError { message: String },
    IoError { message: String },
    AlreadyRunning { container_id: String },
    NotRunning { container_id: String },
}

impl std::fmt::Display for ContainerLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerLogError::SshError { message } => write!(f, "SSH error: {message}"),
            ContainerLogError::ContainerNotFound { container_id } => {
                write!(f, "Container not found: {container_id}")
            }
            ContainerLogError::ParseError { message } => write!(f, "Parse error: {message}"),
            ContainerLogError::IoError { message } => write!(f, "IO error: {message}"),
            ContainerLogError::AlreadyRunning { container_id } => {
                write!(f, "log follow already running for {container_id}")
            }
            ContainerLogError::NotRunning { container_id } => {
                write!(f, "no active log follow for {container_id}")
            }
        }
    }
}

impl std::error::Error for ContainerLogError {}

const FOLLOW_BUFFER_CAP: usize = 2000;

struct FollowSession {
    stop: Arc<AtomicBool>,
    buffer: Arc<Mutex<Vec<ContainerLogEntry>>>,
    target: SshTarget,
}

/// Process-wide follow sessions shared by CLI trait + Tauri UI.
pub struct ContainerLogFollowRegistry {
    sessions: Mutex<HashMap<String, FollowSession>>,
}

impl ContainerLogFollowRegistry {
    pub fn global() -> &'static Self {
        static REGISTRY: OnceLock<ContainerLogFollowRegistry> = OnceLock::new();
        REGISTRY.get_or_init(|| Self {
            sessions: Mutex::new(HashMap::new()),
        })
    }

    pub fn session_key(target: &SshTarget, container_id: &str) -> String {
        format!(
            "{}@{}:{}::{container_id}",
            target.user, target.host, target.port
        )
    }

    pub fn start(
        &self,
        client: Arc<dyn SshClient + Send + Sync>,
        target: SshTarget,
        container_id: String,
        since: Option<u64>,
        _until: Option<u64>,
    ) -> Result<(), ContainerLogError> {
        let key = Self::session_key(&target, &container_id);
        {
            let sessions = self
                .sessions
                .lock()
                .map_err(|e| ContainerLogError::IoError {
                    message: e.to_string(),
                })?;
            if sessions.contains_key(&key) {
                return Err(ContainerLogError::AlreadyRunning {
                    container_id: container_id.clone(),
                });
            }
        }

        let stop = Arc::new(AtomicBool::new(false));
        let buffer = Arc::new(Mutex::new(Vec::new()));
        {
            let mut sessions = self
                .sessions
                .lock()
                .map_err(|e| ContainerLogError::IoError {
                    message: e.to_string(),
                })?;
            sessions.insert(
                key.clone(),
                FollowSession {
                    stop: stop.clone(),
                    buffer: buffer.clone(),
                    target: target.clone(),
                },
            );
        }

        let container_for_thread = container_id.clone();
        let manager_key = key.clone();
        thread::spawn(move || {
            let since_arg = since
                .map(|ts| format!("--since={ts}"))
                .unwrap_or_default();
            let container_arg = escape_shell_arg(&container_for_thread);
            let cmd = format!(
                "docker logs --tail=50 {since_arg} -f --timestamps -- {container_arg} 2>&1"
            );
            let mut on_output = |stream: SshOutputStream, line: &str| {
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                if let Some(entry) = parse_docker_log_line(line, &container_for_thread) {
                    let mut entry = entry;
                    entry.stream = match stream {
                        SshOutputStream::Stdout => LogStream::Stdout,
                        SshOutputStream::Stderr => LogStream::Stderr,
                    };
                    if let Ok(mut buf) = buffer.lock() {
                        if buf.len() >= FOLLOW_BUFFER_CAP {
                            let drop_n = buf.len() - FOLLOW_BUFFER_CAP + 1;
                            buf.drain(0..drop_n);
                        }
                        buf.push(entry);
                    }
                }
            };
            let _ = client.execute_streaming(&target, &cmd, &mut on_output);
            if let Ok(mut sessions) = ContainerLogFollowRegistry::global().sessions.lock() {
                sessions.remove(&manager_key);
            }
        });

        Ok(())
    }

    pub fn stop(&self, target: &SshTarget, container_id: &str) -> Result<(), ContainerLogError> {
        let key = Self::session_key(target, container_id);
        let (stop_flag, kill_target) = {
            let sessions = self
                .sessions
                .lock()
                .map_err(|e| ContainerLogError::IoError {
                    message: e.to_string(),
                })?;
            match sessions.get(&key) {
                Some(session) => (session.stop.clone(), session.target.clone()),
                None => {
                    return Err(ContainerLogError::NotRunning {
                        container_id: container_id.to_string(),
                    });
                }
            }
        };
        stop_flag.store(true, Ordering::SeqCst);

        // Best-effort: terminate remote `docker logs -f` so the SSH stream ends.
        let client = agus_ssh::ProcessSshClient::new();
        let container_arg = escape_shell_arg(container_id);
        let kill_cmd = format!(
            "pkill -f \"docker logs .*-- {container_arg}\" 2>/dev/null || pkill -f \"docker logs .*{container_id}\" 2>/dev/null || true"
        );
        let _ = client.execute(&kill_target, &kill_cmd);
        Ok(())
    }

    pub fn poll(
        &self,
        target: &SshTarget,
        container_id: &str,
    ) -> Result<Vec<ContainerLogEntry>, ContainerLogError> {
        let key = Self::session_key(target, container_id);
        let sessions = self
            .sessions
            .lock()
            .map_err(|e| ContainerLogError::IoError {
                message: e.to_string(),
            })?;
        let Some(session) = sessions.get(&key) else {
            return Ok(Vec::new());
        };
        let mut buf = session
            .buffer
            .lock()
            .map_err(|e| ContainerLogError::IoError {
                message: e.to_string(),
            })?;
        Ok(std::mem::take(&mut *buf))
    }

    pub fn is_active(&self, target: &SshTarget, container_id: &str) -> bool {
        let key = Self::session_key(target, container_id);
        self.sessions
            .lock()
            .map(|s| s.contains_key(&key))
            .unwrap_or(false)
    }

    /// Host-id keyed helpers for Tauri (maps host_id → target via caller).
    pub fn start_for_target(
        &self,
        client: Arc<dyn SshClient + Send + Sync>,
        target: SshTarget,
        container_id: String,
    ) -> Result<(), ContainerLogError> {
        self.start(client, target, container_id, None, None)
    }
}

pub struct SshContainerLogMonitor {
    client: Arc<dyn SshClient + Send + Sync>,
    target: SshTarget,
}

impl SshContainerLogMonitor {
    pub fn new(client: Arc<dyn SshClient + Send + Sync>, target: SshTarget) -> Self {
        Self { client, target }
    }

    pub fn target(&self) -> &SshTarget {
        &self.target
    }
}

impl ContainerLogMonitor for SshContainerLogMonitor {
    fn start_monitoring(
        &self,
        container_id: &str,
        since: Option<u64>,
        until: Option<u64>,
    ) -> Result<(), ContainerLogError> {
        ContainerLogFollowRegistry::global().start(
            self.client.clone(),
            self.target.clone(),
            container_id.to_string(),
            since,
            until,
        )
    }

    fn stop_monitoring(&self, container_id: &str) -> Result<(), ContainerLogError> {
        ContainerLogFollowRegistry::global().stop(&self.target, container_id)
    }

    fn get_logs(
        &self,
        container_id: &str,
        lines: Option<usize>,
        since: Option<u64>,
    ) -> Result<Vec<ContainerLogEntry>, ContainerLogError> {
        let lines_arg = lines.map(|n| format!("--tail={n}")).unwrap_or_default();
        let since_arg = since
            .map(|ts| format!("--since {ts}"))
            .unwrap_or_default();
        let container_arg = escape_shell_arg(container_id);

        let cmd = format!("docker logs {lines_arg} {since_arg} -- {container_arg} 2>&1");

        let result =
            self.client
                .execute(&self.target, &cmd)
                .map_err(|e| ContainerLogError::SshError {
                    message: format!("Failed to execute command: {e}"),
                })?;

        if result.exit_code != 0 {
            return Err(ContainerLogError::ContainerNotFound {
                container_id: container_id.to_string(),
            });
        }

        let mut entries = Vec::new();
        for line in result.stdout.lines() {
            if let Some(entry) = parse_docker_log_line(line, container_id) {
                entries.push(entry);
            }
        }

        Ok(entries)
    }

    fn stream_logs<F>(&self, container_id: &str, callback: F) -> Result<(), ContainerLogError>
    where
        F: Fn(ContainerLogEntry) + Send + Sync + 'static,
    {
        // Bounded follow for one-shot callers; long sessions use start_monitoring.
        let container_arg = escape_shell_arg(container_id);
        let follow_cmd = format!(
            "(command -v timeout >/dev/null 2>&1 && timeout 20s docker logs --tail=50 -f --timestamps -- {0} 2>&1) || docker logs --tail=100 --timestamps -- {0} 2>&1",
            container_arg
        );

        let result = self
            .client
            .execute(&self.target, &follow_cmd)
            .map_err(|e| ContainerLogError::SshError {
                message: format!("Failed to execute bounded log follow: {e}"),
            })?;

        let mut emitted = 0usize;
        for line in result.stdout.lines() {
            if let Some(entry) = parse_docker_log_line(line, container_id) {
                callback(entry);
                emitted += 1;
            }
        }
        if emitted == 0 && result.exit_code != 0 && result.exit_code != 124 {
            return Err(ContainerLogError::SshError {
                message: format!(
                    "bounded log follow failed (exit {}): {}",
                    result.exit_code,
                    result.stderr.chars().take(200).collect::<String>()
                ),
            });
        }
        Ok(())
    }

    fn poll_monitoring(
        &self,
        container_id: &str,
    ) -> Result<Vec<ContainerLogEntry>, ContainerLogError> {
        ContainerLogFollowRegistry::global().poll(&self.target, container_id)
    }

    fn is_monitoring(&self, container_id: &str) -> bool {
        ContainerLogFollowRegistry::global().is_active(&self.target, container_id)
    }
}

fn escape_shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parse_docker_log_line(line: &str, container_id: &str) -> Option<ContainerLogEntry> {
    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    let (timestamp, stream, message) = if parts.len() >= 3 {
        let stream_str = parts[1];
        let msg = parts[2..].join(" ");
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let stream = if stream_str == "stdout" {
            LogStream::Stdout
        } else if stream_str == "stderr" {
            LogStream::Stderr
        } else {
            // Timestamp may be first token without stream marker
            LogStream::Stdout
        };
        let message = if stream_str == "stdout" || stream_str == "stderr" {
            msg
        } else {
            line.to_string()
        };
        (timestamp, stream, message)
    } else {
        (
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            LogStream::Stdout,
            line.to_string(),
        )
    };

    let level = if message.to_lowercase().contains("error")
        || message.to_lowercase().contains("fatal")
    {
        LogLevel::Error
    } else if message.to_lowercase().contains("warning") || message.to_lowercase().contains("warn")
    {
        LogLevel::Warning
    } else if message.to_lowercase().contains("debug") {
        LogLevel::Debug
    } else {
        LogLevel::Info
    };

    Some(ContainerLogEntry {
        container_id: container_id.to_string(),
        container_name: container_id.to_string(),
        timestamp,
        level,
        message,
        stream,
    })
}
