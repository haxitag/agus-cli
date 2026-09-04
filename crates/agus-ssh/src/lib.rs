use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub host: String,
    pub user: String,
    pub port: u16,
    pub identity_file: Option<PathBuf>,
    pub password: Option<String>, // SSH密码（可选，与identity_file互斥）
}

#[derive(Debug, Clone)]
pub struct SshConfig {
    pub connect_timeout: Duration,
    pub command_timeout: Duration, // 命令执行超时
    pub max_retries: u32,
    pub retry_delay: Duration,
}

impl Default for SshConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            command_timeout: Duration::from_secs(60), // 默认 60 秒命令超时
            max_retries: 3,
            retry_delay: Duration::from_secs(2),
        }
    }
}

impl SshConfig {
    fn parse_env_u64(key: &str) -> Option<u64> {
        std::env::var(key)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .and_then(|v| v.parse::<u64>().ok())
    }

    fn parse_env_u32(key: &str) -> Option<u32> {
        std::env::var(key)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .and_then(|v| v.parse::<u32>().ok())
    }

    /// 从环境变量覆盖默认值（用于 CLI/UI 一致的生产级超时与重试策略）
    ///
    /// - `AGUS_SSH_CONNECT_TIMEOUT_SECS`
    /// - `AGUS_SSH_COMMAND_TIMEOUT_SECS`
    /// - `AGUS_SSH_MAX_RETRIES`
    /// - `AGUS_SSH_RETRY_DELAY_MS`
    pub fn from_env_or_default() -> Self {
        let mut cfg = Self::default();
        if let Some(secs) = Self::parse_env_u64("AGUS_SSH_CONNECT_TIMEOUT_SECS") {
            if secs > 0 {
                cfg.connect_timeout = Duration::from_secs(secs);
            }
        }
        if let Some(secs) = Self::parse_env_u64("AGUS_SSH_COMMAND_TIMEOUT_SECS") {
            if secs > 0 {
                cfg.command_timeout = Duration::from_secs(secs);
            }
        }
        if let Some(retries) = Self::parse_env_u32("AGUS_SSH_MAX_RETRIES") {
            cfg.max_retries = retries.max(1);
        }
        if let Some(ms) = Self::parse_env_u64("AGUS_SSH_RETRY_DELAY_MS") {
            cfg.retry_delay = Duration::from_millis(ms);
        }
        cfg
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshCommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshOutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SshError {
    Connection { message: String },
    Command { exit_code: i32, stderr: String },
    Timeout { message: String }, // 超时错误
}

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SshError::Connection { message } => write!(f, "ssh connection error: {message}"),
            SshError::Command { exit_code, stderr } => {
                write!(f, "ssh command failed (exit {exit_code}): {stderr}")
            }
            SshError::Timeout { message } => write!(f, "ssh command timeout: {message}"),
        }
    }
}

impl std::error::Error for SshError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn ssh_config_from_env_or_default_overrides() {
        let _guard = ENV_LOCK.lock().expect("env lock");
        std::env::set_var("AGUS_SSH_CONNECT_TIMEOUT_SECS", "3");
        std::env::set_var("AGUS_SSH_COMMAND_TIMEOUT_SECS", "7");
        std::env::set_var("AGUS_SSH_MAX_RETRIES", "5");
        std::env::set_var("AGUS_SSH_RETRY_DELAY_MS", "1200");

        let cfg = SshConfig::from_env_or_default();
        assert_eq!(cfg.connect_timeout, Duration::from_secs(3));
        assert_eq!(cfg.command_timeout, Duration::from_secs(7));
        assert_eq!(cfg.max_retries, 5);
        assert_eq!(cfg.retry_delay, Duration::from_millis(1200));

        std::env::remove_var("AGUS_SSH_CONNECT_TIMEOUT_SECS");
        std::env::remove_var("AGUS_SSH_COMMAND_TIMEOUT_SECS");
        std::env::remove_var("AGUS_SSH_MAX_RETRIES");
        std::env::remove_var("AGUS_SSH_RETRY_DELAY_MS");
    }
}

#[derive(Debug, Clone)]
struct SshControlConfig {
    path: PathBuf,
    persist: Duration,
}

fn ssh_control_base_dir() -> Option<PathBuf> {
    if std::env::var_os("AGUS_SSH_DISABLE_MULTIPLEX").is_some() {
        return None;
    }
    if let Ok(path) = std::env::var("AGUS_SSH_CONTROL_DIR") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    #[cfg(unix)]
    {
        return Some(PathBuf::from("/tmp/agus-ssh"));
    }
    #[cfg(not(unix))]
    {
        return Some(std::env::temp_dir().join("agus-ssh"));
    }
}

fn control_path_for_target(base_dir: &PathBuf, target: &SshTarget) -> PathBuf {
    let local_user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let connection_id = format!(
        "{}:{}@{}:{}",
        local_user, target.user, target.host, target.port
    );
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    connection_id.hash(&mut hasher);
    let hash = hasher.finish();
    base_dir.join(format!("agus-ssh-{:x}", hash))
}

fn build_control_config(
    base_dir: PathBuf,
    target: &SshTarget,
    persist: Duration,
) -> Option<SshControlConfig> {
    if std::fs::create_dir_all(&base_dir).is_err() {
        return None;
    }
    Some(SshControlConfig {
        path: control_path_for_target(&base_dir, target),
        persist,
    })
}

/// 供交互式终端与命令执行共享 ControlMaster 套接字，避免重复 TCP 连接与 session 耗尽。
#[derive(Debug, Clone)]
pub struct SshMultiplexConfig {
    pub control_path: PathBuf,
    pub persist_secs: u64,
}

pub fn ssh_multiplex_config_for(target: &SshTarget, persist: Duration) -> Option<SshMultiplexConfig> {
    let base = ssh_control_base_dir()?;
    let control = build_control_config(base, target, persist)?;
    Some(SshMultiplexConfig {
        control_path: control.path,
        persist_secs: control.persist.as_secs(),
    })
}

pub trait SshClient {
    fn execute(&self, target: &SshTarget, command: &str) -> Result<SshCommandResult, SshError>;

    fn execute_streaming(
        &self,
        target: &SshTarget,
        command: &str,
        on_output: &mut dyn FnMut(SshOutputStream, &str),
    ) -> Result<SshCommandResult, SshError> {
        let result = self.execute(target, command)?;
        for line in result.stdout.lines() {
            on_output(SshOutputStream::Stdout, line);
        }
        for line in result.stderr.lines() {
            on_output(SshOutputStream::Stderr, line);
        }
        Ok(result)
    }
}

#[derive(Debug)]
pub struct ProcessSshClient {
    config: SshConfig,
}

impl ProcessSshClient {
    pub fn new() -> Self {
        Self {
            config: SshConfig::from_env_or_default(),
        }
    }

    pub fn with_config(config: SshConfig) -> Self {
        Self { config }
    }

    fn run_command_with_timeout(
        &self,
        mut cmd: Command,
        timeout: Duration,
        label: &'static str,
    ) -> Result<std::process::Output, SshError> {
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| SshError::Connection {
            message: format!("failed to launch {label}: {e}"),
        })?;

        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();

        let stdout_handle = std::thread::spawn(move || -> Vec<u8> {
            let mut buf = Vec::new();
            if let Some(mut out) = stdout.take() {
                let _ = out.read_to_end(&mut buf);
            }
            buf
        });
        let stderr_handle = std::thread::spawn(move || -> Vec<u8> {
            let mut buf = Vec::new();
            if let Some(mut err) = stderr.take() {
                let _ = err.read_to_end(&mut buf);
            }
            buf
        });

        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let stdout = stdout_handle.join().unwrap_or_default();
                    let stderr = stderr_handle.join().unwrap_or_default();
                    return Ok(std::process::Output {
                        status,
                        stdout,
                        stderr,
                    });
                }
                Ok(None) => {
                    if started.elapsed() >= timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = stdout_handle.join();
                        let _ = stderr_handle.join();
                        return Err(SshError::Timeout {
                            message: format!(
                                "command execution timeout after {} seconds",
                                timeout.as_secs()
                            ),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_handle.join();
                    let _ = stderr_handle.join();
                    return Err(SshError::Connection {
                        message: format!("failed to wait for {label}: {e}"),
                    });
                }
            }
        }
    }

    fn execute_with_control(
        &self,
        target: &SshTarget,
        command: &str,
        control: Option<&SshControlConfig>,
    ) -> Result<SshCommandResult, SshError> {
        let mut last_error = None;
        for attempt in 0..self.config.max_retries {
            match self.try_execute_with_control(target, command, control) {
                Ok(result) => return Ok(result),
                Err(err) => {
                    // 超时多为远端挂死（如 docker info）；重试只会把卡顿放大数倍
                    let timed_out = matches!(&err, SshError::Timeout { .. });
                    last_error = Some(err);
                    if timed_out || attempt + 1 >= self.config.max_retries {
                        break;
                    }
                    std::thread::sleep(self.config.retry_delay);
                }
            }
        }
        Err(last_error.unwrap())
    }

    fn execute_streaming_with_control(
        &self,
        target: &SshTarget,
        command: &str,
        on_output: &mut dyn FnMut(SshOutputStream, &str),
        control: Option<&SshControlConfig>,
    ) -> Result<SshCommandResult, SshError> {
        let mut last_error = None;
        for attempt in 0..self.config.max_retries {
            match self.try_execute_streaming_with_control(target, command, on_output, control) {
                Ok(result) => return Ok(result),
                Err(err) => {
                    let timed_out = matches!(&err, SshError::Timeout { .. });
                    last_error = Some(err);
                    if timed_out || attempt + 1 >= self.config.max_retries {
                        break;
                    }
                    std::thread::sleep(self.config.retry_delay);
                }
            }
        }
        Err(last_error.unwrap())
    }

    /// 获取 sshpass 可执行文件路径
    /// 优先使用应用 bundle 内的 sshpass，如果不存在则使用系统的
    fn get_sshpass_path() -> Option<String> {
        // 首先尝试使用应用 bundle 内的 sshpass
        if let Ok(exe_path) = std::env::current_exe() {
            // 获取应用 bundle 的 Resources 目录
            if let Some(app_path) = exe_path.parent() {
                // macOS: 应用在 .app/Contents/MacOS/ 目录下
                // Resources 目录在 .app/Contents/Resources/
                let resources_path = app_path
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.join("Resources"));

                if let Some(resources) = resources_path {
                    // Tauri 会将 resources/bin/sshpass 扁平化复制到 Resources/resources/sshpass
                    // 注意：Tauri 会保持目录结构，但实际测试发现可能被扁平化
                    // 尝试多种可能的路径
                    let bundled_sshpass_bin =
                        resources.join("resources").join("bin").join("sshpass");
                    let bundled_sshpass = resources.join("resources").join("sshpass");
                    let bundled_sshpass_direct = resources.join("sshpass");

                    // 静默查找，不输出调试信息（避免重复日志）
                    // 如果需要调试，可以通过环境变量 RUST_LOG=debug 启用

                    // 优先使用 resources/sshpass（Tauri 扁平化后的路径），然后尝试其他路径
                    let sshpass_path = if bundled_sshpass.exists() {
                        Some(bundled_sshpass)
                    } else if bundled_sshpass_bin.exists() {
                        Some(bundled_sshpass_bin)
                    } else if bundled_sshpass_direct.exists() {
                        Some(bundled_sshpass_direct)
                    } else {
                        None
                    };

                    if let Some(sshpass_path) = sshpass_path {
                        if let Some(path_str) = sshpass_path.to_str() {
                            // 检查文件是否可执行
                            if std::fs::metadata(&sshpass_path)
                                .map(|m| {
                                    #[cfg(unix)]
                                    {
                                        use std::os::unix::fs::PermissionsExt;
                                        m.permissions().mode() & 0o111 != 0
                                    }
                                    #[cfg(not(unix))]
                                    {
                                        true
                                    }
                                })
                                .unwrap_or(false)
                            {
                                return Some(path_str.to_string());
                            }
                        }
                    }
                }

                // 也尝试直接从 MacOS 目录向上查找（处理不同的打包方式）
                // 有些情况下 Resources 可能在不同的位置
                let mut check_path = app_path.to_path_buf();
                for _ in 0..5 {
                    // 尝试多种可能的路径
                    let paths_to_check = vec![
                        check_path
                            .join("Resources")
                            .join("resources")
                            .join("bin")
                            .join("sshpass"),
                        check_path
                            .join("Resources")
                            .join("resources")
                            .join("sshpass"),
                        check_path.join("Resources").join("sshpass"),
                        check_path.join("resources").join("bin").join("sshpass"),
                        check_path.join("resources").join("sshpass"),
                    ];

                    for path in paths_to_check {
                        if path.exists() {
                            if let Some(path_str) = path.to_str() {
                                // 检查文件是否可执行
                                if std::fs::metadata(&path)
                                    .map(|m| {
                                        #[cfg(unix)]
                                        {
                                            use std::os::unix::fs::PermissionsExt;
                                            m.permissions().mode() & 0o111 != 0
                                        }
                                        #[cfg(not(unix))]
                                        {
                                            true
                                        }
                                    })
                                    .unwrap_or(false)
                                {
                                    return Some(path_str.to_string());
                                }
                            }
                        }
                    }

                    if let Some(parent) = check_path.parent() {
                        check_path = parent.to_path_buf();
                    } else {
                        break;
                    }
                }

                // 开发模式：尝试在 src-tauri/resources/bin 目录查找
                // 从 exe_path 向上查找 src-tauri/resources/bin/sshpass
                let mut current = app_path.to_path_buf();
                let mut last_path = current.clone();
                loop {
                    // 尝试 resources/bin/sshpass（新路径）
                    let resources_bin = current.join("resources").join("bin").join("sshpass");
                    if resources_bin.exists() {
                        if let Some(path_str) = resources_bin.to_str() {
                            return Some(path_str.to_string());
                        }
                    }
                    // 也尝试 resources/sshpass（旧路径，向后兼容）
                    let resources = current.join("resources").join("sshpass");
                    if resources.exists() {
                        if let Some(path_str) = resources.to_str() {
                            return Some(path_str.to_string());
                        }
                    }
                    if let Some(parent) = current.parent() {
                        let parent_path = parent.to_path_buf();
                        // 防止无限循环，如果路径没有变化就停止
                        if parent_path == last_path {
                            break;
                        }
                        last_path = current.clone();
                        current = parent_path;
                    } else {
                        break;
                    }
                }
            }
        }

        // 回退到系统的 sshpass
        if Self::is_sshpass_available() {
            return Some("sshpass".to_string());
        }

        None
    }

    /// 检查 sshpass 是否可用（检查系统的）
    fn is_sshpass_available() -> bool {
        // 尝试运行 sshpass -V，如果命令不存在或执行失败，返回 false
        match Command::new("sshpass")
            .arg("-V")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            Ok(status) => status.success(),
            Err(_) => false, // 命令不存在或无法执行
        }
    }

    fn try_execute(&self, target: &SshTarget, command: &str) -> Result<SshCommandResult, SshError> {
        let control = ssh_control_base_dir()
            .and_then(|base| build_control_config(base, target, Duration::from_secs(300)));
        self.try_execute_with_control(target, command, control.as_ref())
    }

    fn control_master_active(&self, target: &SshTarget, control: &SshControlConfig) -> bool {
        if !control.path.exists() {
            return false;
        }
        let destination = format!("{}@{}", target.user, target.host);
        let mut cmd = Command::new("ssh");

        // 添加SSH选项
        cmd.arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-o")
            .arg(format!(
                "ConnectTimeout={}",
                self.config.connect_timeout.as_secs()
            ))
            .arg("-p")
            .arg(target.port.to_string())
            .arg("-o")
            .arg("ControlMaster=auto")
            .arg("-o")
            .arg(format!("ControlPersist={}s", control.persist.as_secs()))
            .arg("-o")
            .arg(format!("ControlPath={}", control.path.to_string_lossy()));

        if let Some(ref identity) = target.identity_file {
            cmd.arg("-i").arg(identity);
        }
        cmd.arg("-O").arg("check").arg(&destination);
        // ControlMaster check 绝不可无限阻塞：socket 僵死时 cmd.output() 会卡死调用线程
        match self.run_command_with_timeout(cmd, Duration::from_secs(8), "ssh-control-check") {
            Ok(output) => output.status.success(),
            Err(_) => false,
        }
    }

    fn spawn_control_master(
        &self,
        target: &SshTarget,
        control: &SshControlConfig,
    ) -> Result<(), SshError> {
        let destination = format!("{}@{}", target.user, target.host);
        let mut cmd = if let Some(ref password) = target.password {
            let sshpass_path = Self::get_sshpass_path().ok_or_else(|| SshError::Connection {
                message: "sshpass is not available for control master setup".to_string(),
            })?;
            let mut cmd = Command::new(&sshpass_path);
            cmd.arg("-p").arg(password).arg("ssh");

            // 添加SSH选项
            cmd.arg("-o")
                .arg("PreferredAuthentications=password")
                .arg("-o")
                .arg("NumberOfPasswordPrompts=1")
                .arg("-o")
                .arg("StrictHostKeyChecking=accept-new")
                .arg("-o")
                .arg("LogLevel=ERROR")
                .arg("-o")
                .arg(format!(
                    "ConnectTimeout={}",
                    self.config.connect_timeout.as_secs()
                ))
                .arg("-p")
                .arg(target.port.to_string())
                .arg("-o")
                .arg("ControlMaster=auto")
                .arg("-o")
                .arg(format!("ControlPersist={}s", control.persist.as_secs()))
                .arg("-o")
                .arg(format!("ControlPath={}", control.path.to_string_lossy()));

            cmd
        } else {
            let mut cmd = Command::new("ssh");

            // 添加SSH选项
            cmd.arg("-o")
                .arg("StrictHostKeyChecking=accept-new")
                .arg("-o")
                .arg(format!(
                    "ConnectTimeout={}",
                    self.config.connect_timeout.as_secs()
                ))
                .arg("-p")
                .arg(target.port.to_string())
                .arg("-o")
                .arg("ControlMaster=auto")
                .arg("-o")
                .arg(format!("ControlPersist={}s", control.persist.as_secs()))
                .arg("-o")
                .arg(format!("ControlPath={}", control.path.to_string_lossy()));

            if let Some(ref identity) = target.identity_file {
                cmd.arg("-i").arg(identity);
            }
            cmd
        };

        cmd.arg("-o")
            .arg("ControlMaster=yes")
            .arg("-N")
            .arg("-f")
            .arg(&destination);

        let output = self.run_command_with_timeout(
            cmd,
            Duration::from_secs(15),
            "ssh-control-master",
        )?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(SshError::Connection {
                message: if stderr.trim().is_empty() {
                    "failed to establish ssh control master".to_string()
                } else {
                    format!("failed to establish ssh control master: {}", stderr.trim())
                },
            })
        }
    }

    fn ensure_control_master(&self, target: &SshTarget, control: &SshControlConfig) {
        if self.control_master_active(target, control) {
            return;
        }
        let _ = self.spawn_control_master(target, control);
    }

    fn try_execute_with_control(
        &self,
        target: &SshTarget,
        command: &str,
        control: Option<&SshControlConfig>,
    ) -> Result<SshCommandResult, SshError> {
        let destination = format!("{}@{}", target.user, target.host);

        if let Some(control) = control {
            self.ensure_control_master(target, control);
        }

        // 如果提供了密码，使用 sshpass；否则使用密钥文件或系统默认密钥
        if let Some(ref password) = target.password {
            // 获取 sshpass 路径（优先使用应用内的）
            let sshpass_path = Self::get_sshpass_path().ok_or_else(|| {
                // 提供更详细的错误信息，帮助调试
                let exe_path = std::env::current_exe()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                // 尝试获取 Resources 路径用于调试
                let resources_info = if let Ok(exe) = std::env::current_exe() {
                    if let Some(app_path) = exe.parent() {
                        let resources = app_path
                            .parent()
                            .and_then(|p| p.parent())
                            .map(|p| p.join("Resources"));
                        if let Some(res) = resources {
                            format!("Resources path: {}", res.display())
                        } else {
                            "Resources path: not found".to_string()
                        }
                    } else {
                        "App path: not found".to_string()
                    }
                } else {
                    "Executable path: unknown".to_string()
                };
                
                let error_msg = format!(
                    "sshpass is not available. The application requires sshpass for password authentication.\n\
                    Executable path: {}\n\
                    {}\n\
                    \n\
                    Please ensure:\n\
                    1. The application was built with the latest version that includes sshpass\n\
                    2. sshpass is included in the application bundle at Resources/resources/sshpass\n\
                    3. Or install sshpass on your system: brew install hudochenkov/sshpass/sshpass",
                    exe_path, resources_info
                );
                SshError::Connection { message: error_msg }
            })?;

            // 使用 sshpass 进行密码认证
            let mut cmd = Command::new(&sshpass_path);
            cmd.arg("-p").arg(password).arg("ssh");

            // 调试：记录ssh命令（不记录密码）
            // Log reduced to avoid spam
            // eprintln!(
            //     "[SSH] 使用sshpass连接: {}@{}:{} (密码长度: {})",
            //     target.user,
            //     target.host,
            //     target.port,
            //     password.len()
            // );

            // 添加SSH选项
            cmd.arg("-o")
                .arg("PreferredAuthentications=password")
                .arg("-o")
                .arg("NumberOfPasswordPrompts=1")
                .arg("-o")
                .arg("StrictHostKeyChecking=accept-new")
                .arg("-o")
                .arg("LogLevel=ERROR")
                .arg("-o")
                .arg(format!(
                    "ConnectTimeout={}",
                    self.config.connect_timeout.as_secs()
                ))
                .arg("-p")
                .arg(target.port.to_string());

            // 如果有control配置，添加ControlMaster选项
            if let Some(control) = control {
                if let Some(path) = control.path.to_str() {
                    cmd.arg("-o")
                        .arg("ControlMaster=auto")
                        .arg("-o")
                        .arg(format!("ControlPersist={}s", control.persist.as_secs()))
                        .arg("-o")
                        .arg(format!("ControlPath={}", path));
                }
            }

            cmd.arg(&destination).arg(command);

            // 使用超时控制执行命令（超时后会 kill 子进程，避免“超时但仍卡死”）
            let timeout = self.config.command_timeout;
            let output = match self.run_command_with_timeout(cmd, timeout, "sshpass/ssh") {
                Ok(output) => output,
                Err(SshError::Connection { message }) => {
                    // 兼容旧提示：sshpass 不存在时给出安装指引
                    if message.contains("No such file")
                        || message.contains("not found")
                        || message.contains("failed to launch sshpass")
                    {
                        return Err(SshError::Connection {
                            message: "sshpass is not installed. Please install it using: brew install hudochenkov/sshpass/sshpass (on macOS) or your system's package manager (on Linux)".to_string(),
                        });
                    }
                    return Err(SshError::Connection { message });
                }
                Err(other) => return Err(other),
            };

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            if output.status.success() {
                return Ok(SshCommandResult {
                    stdout,
                    stderr,
                    exit_code,
                });
            }

            if exit_code == 255 {
                let message = if stderr.trim().is_empty() {
                    "ssh connection failed".to_string()
                } else {
                    stderr.trim().to_string()
                };
                return Err(SshError::Connection { message });
            }

            return Err(SshError::Command { exit_code, stderr });
        }

        // 使用密钥文件或系统默认密钥
        let mut cmd = Command::new("ssh");

        // 自动化执行必须 BatchMode：禁止卡在 passphrase/密码交互提示
        cmd.arg("-o")
            .arg("BatchMode=yes")
            .arg("-o")
            .arg("NumberOfPasswordPrompts=0")
            .arg("-o")
            .arg("StrictHostKeyChecking=accept-new")
            .arg("-o")
            .arg(format!(
                "ConnectTimeout={}",
                self.config.connect_timeout.as_secs()
            ))
            .arg("-p")
            .arg(target.port.to_string());

        // 如果有control配置，添加ControlMaster选项
        if let Some(control) = control {
            if let Some(path) = control.path.to_str() {
                cmd.arg("-o")
                    .arg("ControlMaster=auto")
                    .arg("-o")
                    .arg(format!("ControlPersist={}s", control.persist.as_secs()))
                    .arg("-o")
                    .arg(format!("ControlPath={}", path));
            }
        }

        // Add identity file if specified
        if let Some(ref identity) = target.identity_file {
            cmd.arg("-i").arg(identity);
        }

        cmd.arg(&destination).arg(command);

        // 使用超时控制执行命令（超时后会 kill 子进程，避免“超时但仍卡死”）
        let timeout = self.config.command_timeout;
        let output = self.run_command_with_timeout(cmd, timeout, "ssh")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);

        if output.status.success() {
            return Ok(SshCommandResult {
                stdout,
                stderr,
                exit_code,
            });
        }

        if exit_code == 255 {
            let message = if stderr.trim().is_empty() {
                "ssh connection failed".to_string()
            } else {
                stderr.trim().to_string()
            };
            return Err(SshError::Connection { message });
        }

        Err(SshError::Command { exit_code, stderr })
    }

    fn try_execute_streaming(
        &self,
        target: &SshTarget,
        command: &str,
        on_output: &mut dyn FnMut(SshOutputStream, &str),
    ) -> Result<SshCommandResult, SshError> {
        let control = ssh_control_base_dir()
            .and_then(|base| build_control_config(base, target, Duration::from_secs(300)));
        self.try_execute_streaming_with_control(target, command, on_output, control.as_ref())
    }

    fn try_execute_streaming_with_control(
        &self,
        target: &SshTarget,
        command: &str,
        on_output: &mut dyn FnMut(SshOutputStream, &str),
        control: Option<&SshControlConfig>,
    ) -> Result<SshCommandResult, SshError> {
        let destination = format!("{}@{}", target.user, target.host);

        if let Some(control) = control {
            self.ensure_control_master(target, control);
        }

        let mut cmd = if let Some(ref password) = target.password {
            // 获取 sshpass 路径（优先使用应用内的）
            let sshpass_path = Self::get_sshpass_path().ok_or_else(|| {
                return SshError::Connection {
                    message: "sshpass is not available. The application requires sshpass for password authentication, but it was not found in the application bundle or system PATH.".to_string(),
                };
            })?;

            let mut cmd = Command::new(&sshpass_path);
            cmd.arg("-p").arg(password).arg("ssh");

            // 添加SSH选项
            cmd.arg("-o")
                .arg("PreferredAuthentications=password")
                .arg("-o")
                .arg("NumberOfPasswordPrompts=1")
                .arg("-o")
                .arg("StrictHostKeyChecking=accept-new")
                .arg("-o")
                .arg("LogLevel=ERROR")
                .arg("-o")
                .arg(format!(
                    "ConnectTimeout={}",
                    self.config.connect_timeout.as_secs()
                ))
                .arg("-p")
                .arg(target.port.to_string());

            // 如果有control配置，添加ControlMaster选项
            if let Some(control) = control {
                if let Some(path) = control.path.to_str() {
                    cmd.arg("-o")
                        .arg("ControlMaster=auto")
                        .arg("-o")
                        .arg(format!("ControlPersist={}s", control.persist.as_secs()))
                        .arg("-o")
                        .arg(format!("ControlPath={}", path));
                }
            }

            cmd.arg(&destination).arg(command);
            cmd
        } else {
            let mut cmd = Command::new("ssh");

            // 自动化流式执行同样禁止交互卡死
            cmd.arg("-o")
                .arg("BatchMode=yes")
                .arg("-o")
                .arg("NumberOfPasswordPrompts=0")
                .arg("-o")
                .arg("StrictHostKeyChecking=accept-new")
                .arg("-o")
                .arg(format!(
                    "ConnectTimeout={}",
                    self.config.connect_timeout.as_secs()
                ))
                .arg("-p")
                .arg(target.port.to_string());

            // 如果有control配置，添加ControlMaster选项
            if let Some(control) = control {
                if let Some(path) = control.path.to_str() {
                    cmd.arg("-o")
                        .arg("ControlMaster=auto")
                        .arg("-o")
                        .arg(format!("ControlPersist={}s", control.persist.as_secs()))
                        .arg("-o")
                        .arg(format!("ControlPath={}", path));
                }
            }

            if let Some(ref identity) = target.identity_file {
                cmd.arg("-i").arg(identity);
            }
            cmd.arg(&destination).arg(command);
            cmd
        };

        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|err| {
            let base_message = if target.password.is_some() {
                // 如果 sshpass 不存在，提供更友好的错误信息
                if err.kind() == std::io::ErrorKind::NotFound {
                    "sshpass is not installed. Please install it using: brew install hudochenkov/sshpass/sshpass (on macOS) or your system's package manager (on Linux)".to_string()
                } else {
                    format!("failed to launch sshpass: {err}. Note: sshpass may need to be installed (brew install hudochenkov/sshpass/sshpass on macOS)")
                }
            } else {
                format!("failed to launch ssh: {err}")
            };
            SshError::Connection {
                message: base_message,
            }
        })?;

        let stdout = child.stdout.take().ok_or_else(|| SshError::Connection {
            message: "failed to capture ssh stdout".to_string(),
        })?;
        let stderr = child.stderr.take().ok_or_else(|| SshError::Connection {
            message: "failed to capture ssh stderr".to_string(),
        })?;

        let (tx, rx) = mpsc::channel::<(SshOutputStream, String)>();
        let tx_out = tx.clone();
        let stdout_handle = thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().flatten() {
                let _ = tx_out.send((SshOutputStream::Stdout, line));
            }
        });
        let tx_err = tx.clone();
        let stderr_handle = thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().flatten() {
                let _ = tx_err.send((SshOutputStream::Stderr, line));
            }
        });
        drop(tx);

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();
        for (kind, line) in rx {
            on_output(kind, &line);
            match kind {
                SshOutputStream::Stdout => {
                    stdout_buf.push_str(&line);
                    stdout_buf.push('\n');
                }
                SshOutputStream::Stderr => {
                    stderr_buf.push_str(&line);
                    stderr_buf.push('\n');
                }
            }
        }

        let _ = stdout_handle.join();
        let _ = stderr_handle.join();

        let status = child.wait().map_err(|err| SshError::Connection {
            message: format!("failed to wait on ssh process: {err}"),
        })?;
        let exit_code = status.code().unwrap_or(-1);

        if status.success() {
            return Ok(SshCommandResult {
                stdout: stdout_buf,
                stderr: stderr_buf,
                exit_code,
            });
        }

        if exit_code == 255 {
            let message = if stderr_buf.trim().is_empty() {
                "ssh connection failed".to_string()
            } else {
                stderr_buf.trim().to_string()
            };
            return Err(SshError::Connection { message });
        }

        Err(SshError::Command {
            exit_code,
            stderr: stderr_buf,
        })
    }
}

impl SshClient for ProcessSshClient {
    fn execute(&self, target: &SshTarget, command: &str) -> Result<SshCommandResult, SshError> {
        let mut last_error = None;

        for attempt in 0..self.config.max_retries {
            match self.try_execute(target, command) {
                Ok(result) => return Ok(result),
                Err(err) => {
                    let timed_out = matches!(&err, SshError::Timeout { .. });
                    last_error = Some(err);
                    if timed_out || attempt + 1 >= self.config.max_retries {
                        break;
                    }
                    std::thread::sleep(self.config.retry_delay);
                }
            }
        }

        Err(last_error.unwrap())
    }

    fn execute_streaming(
        &self,
        target: &SshTarget,
        command: &str,
        on_output: &mut dyn FnMut(SshOutputStream, &str),
    ) -> Result<SshCommandResult, SshError> {
        let mut last_error = None;

        for attempt in 0..self.config.max_retries {
            match self.try_execute_streaming(target, command, on_output) {
                Ok(result) => return Ok(result),
                Err(err) => {
                    let timed_out = matches!(&err, SshError::Timeout { .. });
                    last_error = Some(err);
                    if timed_out || attempt + 1 >= self.config.max_retries {
                        break;
                    }
                    std::thread::sleep(self.config.retry_delay);
                }
            }
        }

        Err(last_error.unwrap())
    }
}

impl Default for ProcessSshClient {
    fn default() -> Self {
        Self::new()
    }
}

// SSH 连接池配置
#[derive(Debug, Clone)]
pub struct SshPoolConfig {
    pub max_connections: usize,
    pub idle_timeout: Duration,
    pub connection_timeout: Duration,
    pub control_path: Option<PathBuf>, // SSH ControlMaster socket 基础目录
}

impl Default for SshPoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            idle_timeout: Duration::from_secs(300), // 5分钟
            connection_timeout: Duration::from_secs(10),
            control_path: None,
        }
    }
}

// 连接池中的连接信息
#[derive(Debug, Clone)]
struct PooledConnection {
    last_used: Instant,
    is_active: bool,
}

// SSH 连接池
#[derive(Debug)]
pub struct SshConnectionPool {
    config: SshPoolConfig,
    connections: Arc<Mutex<HashMap<String, PooledConnection>>>, // key: connection_id
    client: ProcessSshClient,
}

impl SshConnectionPool {
    pub fn new(config: SshPoolConfig) -> Self {
        Self {
            config,
            connections: Arc::new(Mutex::new(HashMap::new())),
            client: ProcessSshClient::new(),
        }
    }

    /// 生成连接 ID（用于标识唯一的连接）
    fn connection_id(target: &SshTarget) -> String {
        format!("{}@{}:{}", target.user, target.host, target.port)
    }

    /// 检查连接是否健康（仅检测 ControlMaster，避免额外占用 MaxSessions）
    pub fn check_connection_health(&self, target: &SshTarget) -> bool {
        let control = self.control_config_for(target);
        if let Some(control) = control.as_ref() {
            return self.client.control_master_active(target, control);
        }
        false
    }

    /// 获取或创建连接
    ///
    /// 注意：绝不在持有 `connections` 锁时做 SSH 网络 I/O。
    /// 否则并行扫描/监控会把其它线程堵在锁上，表现为「远程扫描一直转圈」。
    pub fn get_connection(&self, target: &SshTarget) -> Result<(), SshError> {
        let connection_id = Self::connection_id(target);

        let tracked = {
            let mut connections = self.connections.lock().unwrap();
            if connections.len() >= self.config.max_connections {
                self.cleanup_idle_connections(&mut connections);
                if connections.len() >= self.config.max_connections
                    && !connections.contains_key(&connection_id)
                {
                    return Err(SshError::Connection {
                        message: format!(
                            "Connection pool is full (max: {})",
                            self.config.max_connections
                        ),
                    });
                }
            }
            connections.contains_key(&connection_id)
        };

        if tracked {
            if self.check_connection_health(target) {
                let mut connections = self.connections.lock().unwrap();
                if let Some(conn) = connections.get_mut(&connection_id) {
                    conn.last_used = Instant::now();
                    conn.is_active = true;
                    return Ok(());
                }
            } else {
                let mut connections = self.connections.lock().unwrap();
                connections.remove(&connection_id);
            }
        }

        // 网络建连在锁外完成
        let control = self.control_config_for(target);
        self.client
            .execute_with_control(target, "echo 'connection_test'", control.as_ref())?;

        let mut connections = self.connections.lock().unwrap();
        if connections.len() >= self.config.max_connections
            && !connections.contains_key(&connection_id)
        {
            self.cleanup_idle_connections(&mut connections);
            if connections.len() >= self.config.max_connections {
                return Err(SshError::Connection {
                    message: format!(
                        "Connection pool is full (max: {})",
                        self.config.max_connections
                    ),
                });
            }
        }
        connections.insert(
            connection_id,
            PooledConnection {
                last_used: Instant::now(),
                is_active: true,
            },
        );
        Ok(())
    }

    /// 清理空闲连接
    fn cleanup_idle_connections(&self, connections: &mut HashMap<String, PooledConnection>) {
        let now = Instant::now();
        connections.retain(|_, conn| now.duration_since(conn.last_used) < self.config.idle_timeout);
    }

    /// 执行命令（带连接池管理）
    pub fn execute(&self, target: &SshTarget, command: &str) -> Result<SshCommandResult, SshError> {
        // 确保连接在池中
        self.get_connection(target)?;

        // 执行命令
        let control = self.control_config_for(target);
        let result = self
            .client
            .execute_with_control(target, command, control.as_ref());

        // 更新连接的最后使用时间
        if result.is_ok() {
            let connection_id = Self::connection_id(target);
            if let Ok(mut connections) = self.connections.lock() {
                if let Some(conn) = connections.get_mut(&connection_id) {
                    conn.last_used = Instant::now();
                }
            }
        }

        result
    }

    /// 移除连接
    pub fn remove_connection(&self, target: &SshTarget) {
        let connection_id = Self::connection_id(target);
        let mut connections = self.connections.lock().unwrap();
        connections.remove(&connection_id);
    }

    /// 清理所有空闲连接
    pub fn cleanup_all_idle(&self) {
        let mut connections = self.connections.lock().unwrap();
        self.cleanup_idle_connections(&mut connections);
    }

    fn control_config_for(&self, target: &SshTarget) -> Option<SshControlConfig> {
        if std::env::var_os("AGUS_SSH_DISABLE_MULTIPLEX").is_some() {
            return None;
        }
        let base = self
            .config
            .control_path
            .clone()
            .or_else(ssh_control_base_dir)?;
        build_control_config(base, target, self.config.idle_timeout)
    }

    /// 获取连接池状态
    pub fn get_pool_status(&self) -> PoolStatus {
        let connections = self.connections.lock().unwrap();
        PoolStatus {
            total_connections: connections.len(),
            max_connections: self.config.max_connections,
            active_connections: connections.values().filter(|c| c.is_active).count(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PoolStatus {
    pub total_connections: usize,
    pub max_connections: usize,
    pub active_connections: usize,
}

impl SshClient for SshConnectionPool {
    fn execute(&self, target: &SshTarget, command: &str) -> Result<SshCommandResult, SshError> {
        SshConnectionPool::execute(self, target, command)
    }

    fn execute_streaming(
        &self,
        target: &SshTarget,
        command: &str,
        on_output: &mut dyn FnMut(SshOutputStream, &str),
    ) -> Result<SshCommandResult, SshError> {
        self.get_connection(target)?;
        let control = self.control_config_for(target);
        let result = self.client.execute_streaming_with_control(
            target,
            command,
            on_output,
            control.as_ref(),
        );
        if result.is_ok() {
            let connection_id = Self::connection_id(target);
            if let Ok(mut connections) = self.connections.lock() {
                if let Some(conn) = connections.get_mut(&connection_id) {
                    conn.last_used = Instant::now();
                }
            }
        }
        result
    }
}
