use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::proposal::SkillReport;
use crate::runtime::SkillRunStatus;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredSkillRun {
    pub run_id: String,
    pub skill_id: String,
    pub created_at: DateTime<Utc>,
    pub trigger: String,
    pub status: SkillRunStatus,
    pub report: SkillReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RunIndexEntry {
    run_id: String,
    skill_id: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RunIndex {
    runs: Vec<RunIndexEntry>,
}

pub struct SkillRunStore {
    root: PathBuf,
}

impl SkillRunStore {
    pub fn new(agus_home: impl AsRef<Path>) -> Self {
        Self {
            root: agus_home.as_ref().join("skill_runs"),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn run_path(&self, run_id: &str) -> PathBuf {
        self.root.join(format!("{run_id}.json"))
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    fn load_index(&self) -> Result<RunIndex, StoreError> {
        let path = self.index_path();
        if !path.is_file() {
            return Ok(RunIndex::default());
        }
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw).unwrap_or_default())
    }

    fn save_index(&self, index: &RunIndex) -> Result<(), StoreError> {
        fs::create_dir_all(&self.root)?;
        let raw = serde_json::to_string_pretty(index)?;
        fs::write(self.index_path(), raw)?;
        Ok(())
    }

    fn upsert_index(&self, stored: &StoredSkillRun) -> Result<(), StoreError> {
        let mut index = self.load_index()?;
        if let Some(entry) = index.runs.iter_mut().find(|e| e.run_id == stored.run_id) {
            entry.skill_id = stored.skill_id.clone();
            entry.created_at = stored.created_at;
        } else {
            index.runs.push(RunIndexEntry {
                run_id: stored.run_id.clone(),
                skill_id: stored.skill_id.clone(),
                created_at: stored.created_at,
            });
        }
        index.runs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        self.save_index(&index)
    }

    pub fn save_report(
        &self,
        trigger: &str,
        status: SkillRunStatus,
        report: &SkillReport,
    ) -> Result<StoredSkillRun, StoreError> {
        fs::create_dir_all(&self.root)?;
        let stored = StoredSkillRun {
            run_id: report.run_id.clone(),
            skill_id: report.skill_id.clone(),
            created_at: Utc::now(),
            trigger: trigger.into(),
            status,
            report: report.clone(),
        };
        let raw = serde_json::to_string_pretty(&stored)?;
        fs::write(self.run_path(&stored.run_id), raw)?;
        self.upsert_index(&stored)?;
        Ok(stored)
    }

    pub fn update_stored(&self, stored: &StoredSkillRun) -> Result<(), StoreError> {
        let raw = serde_json::to_string_pretty(stored)?;
        fs::write(self.run_path(&stored.run_id), raw)?;
        self.upsert_index(stored)
    }

    pub fn list_reports(&self, limit: usize) -> Result<Vec<StoredSkillRun>, StoreError> {
        fs::create_dir_all(&self.root)?;
        let index = self.load_index()?;
        if !index.runs.is_empty() {
            let mut out = Vec::new();
            for entry in index.runs.iter().take(limit) {
                if let Ok(stored) = self.load_report(&entry.run_id) {
                    out.push(stored);
                }
            }
            return Ok(out);
        }

        // Fallback: scan directory when index missing.
        let mut entries: Vec<_> = fs::read_dir(&self.root)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path().extension().is_some_and(|ext| ext == "json")
                    && e.file_name().to_string_lossy() != "index.json"
            })
            .collect();
        entries.sort_by_key(|e| e.file_name());
        let mut out = Vec::new();
        for entry in entries.into_iter().rev().take(limit) {
            if let Ok(stored) = self.load_report_from_path(&entry.path()) {
                out.push(stored);
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(out.into_iter().take(limit).collect())
    }

    pub fn load_report(&self, run_id: &str) -> Result<StoredSkillRun, StoreError> {
        let path = self.run_path(run_id);
        if !path.is_file() {
            return Err(StoreError::NotFound(run_id.into()));
        }
        self.load_report_from_path(&path)
    }

    fn load_report_from_path(&self, path: &Path) -> Result<StoredSkillRun, StoreError> {
        let raw = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }
}
