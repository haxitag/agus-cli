use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config;
use crate::CliError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShortcutType {
    Shell,
    Script,
    Copilot,
}

impl Default for ShortcutType {
    fn default() -> Self {
        Self::Shell
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliShortcut {
    pub id: String,
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub shortcut_type: ShortcutType,
    #[serde(default)]
    pub host_scope: Vec<String>,
    #[serde(default = "default_auto_run")]
    pub auto_run: bool,
    #[serde(default)]
    pub tags: Vec<String>,
}

fn default_auto_run() -> bool {
    true
}

fn shortcuts_path() -> Result<PathBuf, CliError> {
    Ok(config::agus_home()?.join("cli_shortcuts.json"))
}

pub fn load_shortcuts() -> Result<Vec<CliShortcut>, CliError> {
    let path = shortcuts_path()?;
    if !path.exists() {
        return Ok(default_shortcuts());
    }
    let data = fs::read_to_string(path)?;
    if data.trim().is_empty() {
        return Ok(default_shortcuts());
    }
    Ok(serde_json::from_str(&data)?)
}

pub fn save_shortcuts(shortcuts: &[CliShortcut]) -> Result<(), CliError> {
    let path = shortcuts_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let data = serde_json::to_string_pretty(shortcuts)?;
    fs::write(path, data)?;
    Ok(())
}

pub fn upsert_shortcut(shortcut: CliShortcut) -> Result<Vec<CliShortcut>, CliError> {
    let mut shortcuts = load_shortcuts()?;
    if let Some(existing) = shortcuts.iter_mut().find(|item| item.id == shortcut.id) {
        *existing = shortcut;
    } else {
        shortcuts.push(shortcut);
    }
    save_shortcuts(&shortcuts)?;
    Ok(shortcuts)
}

pub fn remove_shortcut(shortcut_id: &str) -> Result<Vec<CliShortcut>, CliError> {
    let mut shortcuts = load_shortcuts()?;
    shortcuts.retain(|item| item.id != shortcut_id);
    save_shortcuts(&shortcuts)?;
    Ok(shortcuts)
}

pub fn shortcuts_for_host(host_id: &str) -> Result<Vec<CliShortcut>, CliError> {
    Ok(load_shortcuts()?
        .into_iter()
        .filter(|item| {
            item.host_scope.is_empty()
                || item.host_scope.iter().any(|scope| scope == host_id || scope == "*")
        })
        .collect())
}

pub fn default_shortcuts() -> Vec<CliShortcut> {
    vec![
        CliShortcut {
            id: "disk-check".to_string(),
            name: "磁盘检查".to_string(),
            command: "df -h && du -sh /var/log/* 2>/dev/null | sort -rh | head -10".to_string(),
            shortcut_type: ShortcutType::Shell,
            host_scope: vec![],
            auto_run: true,
            tags: vec!["sre".to_string(), "disk".to_string()],
        },
        CliShortcut {
            id: "docker-status".to_string(),
            name: "Docker 状态".to_string(),
            command: "docker ps -a --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'".to_string(),
            shortcut_type: ShortcutType::Shell,
            host_scope: vec![],
            auto_run: true,
            tags: vec!["docker".to_string()],
        },
        CliShortcut {
            id: "service-failed".to_string(),
            name: "失败服务".to_string(),
            command: "systemctl --failed --no-pager".to_string(),
            shortcut_type: ShortcutType::Shell,
            host_scope: vec![],
            auto_run: true,
            tags: vec!["systemd".to_string()],
        },
        CliShortcut {
            id: "log-errors".to_string(),
            name: "最近错误日志".to_string(),
            command: "journalctl -p err -n 200 --no-pager".to_string(),
            shortcut_type: ShortcutType::Shell,
            host_scope: vec![],
            auto_run: true,
            tags: vec!["logs".to_string()],
        },
    ]
}

pub fn ensure_default_shortcuts() -> Result<(), CliError> {
    let path = shortcuts_path()?;
    if path.exists() {
        return Ok(());
    }
    save_shortcuts(&default_shortcuts())
}
