//! CLI 稳定退出码契约（GUI/脚本/CI 共用）
//!
//! | Code | Meaning |
//! |------|---------|
//! | 0 | 成功 |
//! | 1 | 通用内部/IO/存储/JSON 失败 |
//! | 2 | 配置错误或非法输入（非 not-found） |
//! | 3 | 鉴权失败 / admin 未配置 |
//! | 4 | 资源未找到（host/project/execution 等） |
//! | 10 | SSH 连接失败 |
//! | 11 | SSH 远端命令失败 |
//! | 124 | SSH/命令超时 |

use crate::CliError;

pub const EXIT_OK: i32 = 0;
pub const EXIT_INTERNAL: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_AUTH: i32 = 3;
pub const EXIT_NOT_FOUND: i32 = 4;
pub const EXIT_SSH_CONNECT: i32 = 10;
pub const EXIT_SSH_COMMAND: i32 = 11;
pub const EXIT_TIMEOUT: i32 = 124;

pub fn exit_code_from_error(err: &CliError) -> i32 {
    match err {
        CliError::InvalidInput(message) => {
            if message.to_lowercase().contains("not found") {
                EXIT_NOT_FOUND
            } else {
                EXIT_USAGE
            }
        }
        CliError::AdminMissing | CliError::AuthFailed => EXIT_AUTH,
        CliError::Config(_) => EXIT_USAGE,
        CliError::Ssh(ssh) => match ssh {
            agus_ssh::SshError::Timeout { .. } => EXIT_TIMEOUT,
            agus_ssh::SshError::Connection { .. } => EXIT_SSH_CONNECT,
            agus_ssh::SshError::Command { .. } => EXIT_SSH_COMMAND,
        },
        CliError::Storage(_) | CliError::Io(_) | CliError::Json(_) => EXIT_INTERNAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_not_found_to_4() {
        let err = CliError::InvalidInput("host not found".into());
        assert_eq!(exit_code_from_error(&err), EXIT_NOT_FOUND);
    }

    #[test]
    fn maps_ssh_timeout_to_124() {
        let err = CliError::Ssh(agus_ssh::SshError::Timeout {
            message: "timeout".into(),
        });
        assert_eq!(exit_code_from_error(&err), EXIT_TIMEOUT);
    }
}
