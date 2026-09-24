use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::config;
use crate::CliError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalHistoryEntry {
    pub host_id: String,
    pub command: String,
    #[serde(default)]
    pub cwd: Option<String>,
    pub timestamp: u64,
    #[serde(default = "default_source")]
    pub source: String,
    #[serde(default)]
    pub use_count: u32,
}

fn default_source() -> String {
    "web_terminal".to_string()
}

fn history_path() -> Result<PathBuf, CliError> {
    Ok(config::agus_home()?.join("terminal_history.jsonl"))
}

fn ensure_parent(path: &PathBuf) -> Result<(), CliError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn append_terminal_history(entry: TerminalHistoryEntry) -> Result<(), CliError> {
    let command = entry.command.trim().to_string();
    if command.is_empty() || command.len() > 4096 {
        return Ok(());
    }
    let path = history_path()?;
    ensure_parent(&path)?;
    let mut record = entry;
    record.command = command;
    if record.timestamp == 0 {
        record.timestamp = Utc::now().timestamp() as u64;
    }
    let line = serde_json::to_string(&record)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")?;
    Ok(())
}

/// 按主机 + 命令文本删除历史（列表已按这两项去重，一次删掉所有重复行）。
pub fn delete_terminal_history(host_id: &str, command: &str) -> Result<(), CliError> {
    let host_id = host_id.trim();
    let command = command.trim();
    if host_id.is_empty() || command.is_empty() {
        return Ok(());
    }
    let path = history_path()?;
    if !path.exists() {
        return Ok(());
    }
    let file = fs::File::open(&path)?;
    let reader = BufReader::new(file);
    let mut kept = Vec::new();
    let mut removed = 0usize;
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<TerminalHistoryEntry>(&line) {
            if entry.host_id == host_id && entry.command.trim() == command {
                removed += 1;
                continue;
            }
        }
        kept.push(line);
    }
    if removed == 0 {
        return Ok(());
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_file_name(format!(
        "{}.{nanos}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("terminal_history.jsonl")
    ));
    {
        let mut out = fs::File::create(&tmp)?;
        for line in &kept {
            writeln!(out, "{line}")?;
        }
    }
    fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn list_terminal_history(
    host_id: Option<&str>,
    limit: usize,
) -> Result<Vec<TerminalHistoryEntry>, CliError> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<TerminalHistoryEntry>(&line) {
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let mut deduped: Vec<TerminalHistoryEntry> = Vec::new();
    for entry in entries {
        if deduped
            .iter()
            .any(|existing| existing.command == entry.command && existing.host_id == entry.host_id)
        {
            continue;
        }
        deduped.push(entry);
    }

    if let Some(host_id) = host_id.filter(|value| !value.is_empty()) {
        let (same_host, other): (Vec<_>, Vec<_>) = deduped
            .into_iter()
            .partition(|entry| entry.host_id == host_id);
        deduped = same_host.into_iter().chain(other).collect();
    }

    deduped.truncate(limit.max(1).min(500));
    Ok(deduped)
}

pub fn default_suggested_commands() -> Vec<TerminalHistoryEntry> {
    let now = Utc::now().timestamp() as u64;
    [
        "df -h",
        "free -h",
        "docker ps -a",
        "systemctl --failed",
        "journalctl -p err -n 100 --no-pager",
        "ss -tulpn",
        "top -bn1 | head -20",
        "uptime",
    ]
    .into_iter()
    .enumerate()
    .map(|(index, command)| TerminalHistoryEntry {
        host_id: "*".to_string(),
        command: command.to_string(),
        cwd: None,
        timestamp: now.saturating_sub(index as u64),
        source: "suggested".to_string(),
        use_count: 0,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    fn entry(host_id: &str, command: &str, timestamp: u64) -> TerminalHistoryEntry {
        TerminalHistoryEntry {
            host_id: host_id.to_string(),
            command: command.to_string(),
            cwd: None,
            timestamp,
            source: "web_terminal".to_string(),
            use_count: 1,
        }
    }

    #[test]
    fn delete_removes_matching_host_command_and_keeps_the_rest() {
        let _guard = crate::test_support::env_lock();
        let temp_dir = TempDir::new().expect("temp dir");
        env::set_var("AGUS_HOME", temp_dir.path().to_string_lossy().to_string());

        append_terminal_history(entry("host-a", "docker compose up -d", 1)).expect("append");
        append_terminal_history(entry("host-a", "docker compose up -d", 2)).expect("append");
        append_terminal_history(entry("host-b", "docker compose up -d", 3)).expect("append");
        append_terminal_history(entry("host-a", "df -h", 4)).expect("append");

        delete_terminal_history("host-a", "docker compose up -d").expect("delete");

        let left = list_terminal_history(None, 20).expect("list");
        let commands: Vec<_> = left
            .iter()
            .map(|item| (item.host_id.as_str(), item.command.as_str()))
            .collect();
        assert!(commands.contains(&("host-b", "docker compose up -d")));
        assert!(commands.contains(&("host-a", "df -h")));
        assert!(!commands.contains(&("host-a", "docker compose up -d")));

        let leftovers: Vec<_> = std::fs::read_dir(temp_dir.path())
            .expect("read home")
            .filter_map(|item| item.ok())
            .map(|item| item.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");

        env::remove_var("AGUS_HOME");
    }
}
