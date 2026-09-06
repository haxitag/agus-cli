use std::collections::HashSet;
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

/// 内置默认集的“补齐版本”。每次扩充内置默认指令时 +1，
/// 用于对已存在 cli_shortcuts.json 的老用户做一次增量合并（不覆盖用户编辑/删除）。
const SHORTCUTS_SEED_VERSION: u32 = 2;

fn seed_version_path() -> Result<PathBuf, CliError> {
    Ok(config::agus_home()?.join("cli_shortcuts_seed_version"))
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
        CliShortcut {
            id: "docker-stats".to_string(),
            name: "Docker 实时统计".to_string(),
            command: "docker stats".to_string(),
            shortcut_type: ShortcutType::Shell,
            host_scope: vec![],
            auto_run: true,
            tags: vec!["docker".to_string()],
        },
        CliShortcut {
            id: "journal-xe".to_string(),
            name: "故障日志上下文".to_string(),
            command: "journalctl -xe".to_string(),
            shortcut_type: ShortcutType::Shell,
            host_scope: vec![],
            auto_run: true,
            tags: vec!["logs".to_string(), "systemd".to_string()],
        },
    ]
}

pub fn ensure_default_shortcuts() -> Result<(), CliError> {
    let path = shortcuts_path()?;
    let version_path = seed_version_path()?;
    // 读取已补齐的种子版本；无标记视为旧版本（v1 初始内置集）
    let seeded_version: u32 = fs::read_to_string(&version_path)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);

    // 已按最新版本补齐过：仅当指令文件被删除时重建默认集
    if seeded_version >= SHORTCUTS_SEED_VERSION {
        if !path.exists() {
            save_shortcuts(&default_shortcuts())?;
        }
        return Ok(());
    }

    // 版本落后：合并缺失的内置默认指令（按 id 去重，保留用户的编辑/自定义项）
    let mut shortcuts = if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .filter(|data| !data.trim().is_empty())
            .and_then(|data| serde_json::from_str::<Vec<CliShortcut>>(&data).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let existing_ids: HashSet<String> = shortcuts.iter().map(|s| s.id.clone()).collect();
    let mut changed = false;
    for default in default_shortcuts() {
        if !existing_ids.contains(&default.id) {
            shortcuts.push(default);
            changed = true;
        }
    }
    if changed {
        save_shortcuts(&shortcuts)?;
    }

    // 推进种子版本（无论是否合并成功，避免每次启动重复尝试；文件被清空/删除时按 load 逻辑回退默认集）
    if let Some(parent) = version_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&version_path, SHORTCUTS_SEED_VERSION.to_string())?;
    Ok(())
}
