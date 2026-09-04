use agus_ssh::{SshClient, SshTarget};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
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
    /// Start monitoring container logs
    fn start_monitoring(
        &self,
        container_id: &str,
        since: Option<u64>,
        until: Option<u64>,
    ) -> Result<(), ContainerLogError>;

    /// Stop monitoring container logs
    fn stop_monitoring(&self, container_id: &str) -> Result<(), ContainerLogError>;

    /// Get recent logs for a container
    fn get_logs(
        &self,
        container_id: &str,
        lines: Option<usize>,
        since: Option<u64>,
    ) -> Result<Vec<ContainerLogEntry>, ContainerLogError>;

    /// Stream logs in real-time (callback-based)
    fn stream_logs<F>(&self, container_id: &str, callback: F) -> Result<(), ContainerLogError>
    where
        F: Fn(ContainerLogEntry) + Send + Sync + 'static;
}

#[derive(Debug)]
pub enum ContainerLogError {
    SshError { message: String },
    ContainerNotFound { container_id: String },
    ParseError { message: String },
    IoError { message: String },
}

impl std::fmt::Display for ContainerLogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContainerLogError::SshError { message } => {
                write!(f, "SSH error: {}", message)
            }
            ContainerLogError::ContainerNotFound { container_id } => {
                write!(f, "Container not found: {}", container_id)
            }
            ContainerLogError::ParseError { message } => {
                write!(f, "Parse error: {}", message)
            }
            ContainerLogError::IoError { message } => {
                write!(f, "IO error: {}", message)
            }
        }
    }
}

impl std::error::Error for ContainerLogError {}

pub struct SshContainerLogMonitor {
    client: Arc<dyn SshClient + Send + Sync>,
    target: SshTarget,
}

impl SshContainerLogMonitor {
    pub fn new(client: Arc<dyn SshClient + Send + Sync>, target: SshTarget) -> Self {
        Self { client, target }
    }
}

impl ContainerLogMonitor for SshContainerLogMonitor {
    fn start_monitoring(
        &self,
        _container_id: &str,
        _since: Option<u64>,
        _until: Option<u64>,
    ) -> Result<(), ContainerLogError> {
        Err(ContainerLogError::SshError {
            message: "持续日志监控未实现：请使用 get_logs / 一次性拉取，勿假装已启动监控".to_string(),
        })
    }

    fn stop_monitoring(&self, _container_id: &str) -> Result<(), ContainerLogError> {
        Err(ContainerLogError::SshError {
            message: "持续日志监控未实现：无后台监控任务可停止".to_string(),
        })
    }

    fn get_logs(
        &self,
        container_id: &str,
        lines: Option<usize>,
        since: Option<u64>,
    ) -> Result<Vec<ContainerLogEntry>, ContainerLogError> {
        let lines_arg = lines.map(|n| format!("--tail={}", n)).unwrap_or_default();
        let since_arg = since
            .map(|ts| format!("--since {}", ts))
            .unwrap_or_default();
        let container_arg = escape_shell_arg(container_id);

        let cmd = format!(
            "docker logs {} {} -- {} 2>&1",
            lines_arg, since_arg, container_arg
        );

        let result =
            self.client
                .execute(&self.target, &cmd)
                .map_err(|e| ContainerLogError::SshError {
                    message: format!("Failed to execute command: {}", e),
                })?;

        if result.exit_code != 0 {
            return Err(ContainerLogError::ContainerNotFound {
                container_id: container_id.to_string(),
            });
        }

        // Parse Docker logs (format: timestamp stream message)
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
        // 非持续流：有界 follow（最多约 20s），避免假装常驻 -f 监控。
        // 优先 timeout(1)；若无 timeout 命令则退回一次性拉取。
        let container_arg = escape_shell_arg(container_id);
        let follow_cmd = format!(
            "(command -v timeout >/dev/null 2>&1 && timeout 20s docker logs --tail=50 -f --timestamps -- {0} 2>&1) || docker logs --tail=100 --timestamps -- {0} 2>&1",
            container_arg
        );

        let result = self
            .client
            .execute(&self.target, &follow_cmd)
            .map_err(|e| ContainerLogError::SshError {
                message: format!("Failed to execute bounded log follow: {}", e),
            })?;

        let mut emitted = 0usize;
        for line in result.stdout.lines() {
            if let Some(entry) = parse_docker_log_line(line, container_id) {
                callback(entry);
                emitted += 1;
            }
        }
        if emitted == 0 && result.exit_code != 0 && result.exit_code != 124 {
            // 124 = timeout 正常结束；其它非零视为失败
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
}

fn escape_shell_arg(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn parse_docker_log_line(line: &str, container_id: &str) -> Option<ContainerLogEntry> {
    // Docker log format: "2024-01-01T12:00:00.000000000Z stdout This is a log message"
    // Or simpler format without timestamp: "This is a log message"

    let parts: Vec<&str> = line.splitn(3, ' ').collect();
    let (timestamp, stream, message) = if parts.len() >= 3 {
        // Try to parse timestamp
        let _ts_str = parts[0];
        let stream_str = parts[1];
        let msg = parts[2..].join(" ");

        // Try to parse timestamp (simplified - just use current time if parsing fails)
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let stream = if stream_str == "stdout" {
            LogStream::Stdout
        } else {
            LogStream::Stderr
        };

        (timestamp, stream, msg)
    } else {
        // No timestamp, assume current time and stdout
        (
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            LogStream::Stdout,
            line.to_string(),
        )
    };

    // Determine log level from message content
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
        container_name: container_id.to_string(), // Would need to look up actual name
        timestamp,
        level,
        message,
        stream,
    })
}
