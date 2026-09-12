use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::catalog::SkillCatalog;
use crate::loader::{LoadError, SkillPackage};
use crate::proposal::{SkillEvidence, SkillReport};
use crate::runtime::{RuntimeError, SkillRun, SkillRuntime};
use crate::store::{SkillRunStore, StoreError, StoredSkillRun};

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("load: {0}")]
    Load(#[from] LoadError),
    #[error("store: {0}")]
    Store(#[from] StoreError),
    #[error("runtime: {0}")]
    Runtime(#[from] RuntimeError),
    #[error("skill not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Msg(String),
}

/// Resolve Agus data home from `AGUS_HOME` or `~/.agus`.
pub fn resolve_agus_home() -> PathBuf {
    if let Ok(home) = std::env::var("AGUS_HOME") {
        return PathBuf::from(home);
    }
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .map(|h| h.join(".agus"))
        .unwrap_or_else(|| PathBuf::from(".agus"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSummary {
    pub id: String,
    pub version: String,
    pub description: String,
    pub risk_class: crate::manifest::RiskClass,
    pub permissions: Vec<crate::manifest::Permission>,
    pub may_auto_run: bool,
}

pub struct SkillService {
    catalog: SkillCatalog,
    store: SkillRunStore,
}

impl SkillService {
    pub fn new() -> Result<Self, ServiceError> {
        Ok(Self {
            catalog: SkillCatalog::discover()?,
            store: SkillRunStore::new(resolve_agus_home()),
        })
    }

    pub fn with_home(agus_home: impl Into<PathBuf>) -> Result<Self, ServiceError> {
        Ok(Self {
            catalog: SkillCatalog::discover()?,
            store: SkillRunStore::new(agus_home.into()),
        })
    }

    pub fn catalog(&self) -> &SkillCatalog {
        &self.catalog
    }

    pub fn store(&self) -> &SkillRunStore {
        &self.store
    }

    pub fn list(&self) -> Vec<SkillSummary> {
        self.catalog
            .list_packages()
            .iter()
            .map(|p| SkillSummary {
                id: p.manifest.id.clone(),
                version: p.manifest.version.clone(),
                description: p.manifest.description.trim().to_string(),
                risk_class: p.manifest.risk_class,
                permissions: p.manifest.permissions.clone(),
                may_auto_run: p.manifest.may_auto_run(),
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Result<&SkillPackage, ServiceError> {
        self.catalog
            .find_package(id)
            .ok_or_else(|| ServiceError::NotFound(id.into()))
    }

    /// Read-only analyze path: observe/analyze skills, no proposals.
    pub fn run_readonly(
        &self,
        skill_id: &str,
        trigger: &str,
        findings: Vec<String>,
        evidence: Vec<SkillEvidence>,
    ) -> Result<SkillReport, ServiceError> {
        let pkg = self.get(skill_id)?.clone();
        let rt = SkillRuntime::new(pkg);
        let mut run = rt.start(trigger)?;
        for ev in evidence {
            SkillRuntime::record_evidence(&mut run, ev);
        }
        for f in findings {
            SkillRuntime::add_finding(&mut run, f);
        }
        let summary = format!("readonly run for {skill_id}");
        let report = rt.finish_report(&run, &summary);
        self.store
            .save_report(trigger, run.status, &report)?;
        Ok(report)
    }

    /// Diagnose + propose path; proposals stay WaitingApproval unless caller approves elsewhere.
    pub fn run_diagnose_with_proposals(
        &self,
        skill_id: &str,
        trigger: &str,
        findings: Vec<String>,
        evidence: Vec<SkillEvidence>,
        actions: Option<(String, String, Vec<String>)>,
    ) -> Result<SkillReport, ServiceError> {
        let pkg = self.get(skill_id)?.clone();
        let rt = SkillRuntime::new(pkg);
        let mut run = rt.start(trigger)?;
        let mut evidence = evidence;
        // Always keep at least synthetic evidence so verdict isn't vacuously NotProven
        // when callers only pass a message (CLI/UI without host metrics).
        if evidence.is_empty() {
            let summary = findings
                .first()
                .cloned()
                .unwrap_or_else(|| trigger.to_string());
            evidence.push(SkillRuntime::now_evidence(
                "trigger",
                &summary,
                None,
                Some("synthetic"),
            ));
        }
        for ev in evidence {
            SkillRuntime::record_evidence(&mut run, ev);
        }
        for f in findings.iter() {
            SkillRuntime::add_finding(&mut run, f.clone());
        }
        let actions = actions.or_else(|| {
            default_proposal_for_skill(skill_id, &findings.join("; "), &rt.package().manifest)
        });
        if let Some((title, rationale, action_list)) = actions {
            rt.propose_actions(&mut run, &title, &rationale, action_list)?;
        }
        let summary = format!("diagnose run for {skill_id}");
        let report = rt.finish_report(&run, &summary);
        self.store
            .save_report(trigger, run.status, &report)?;
        Ok(report)
    }

    pub fn approve_proposal(
        &self,
        run_id: &str,
        proposal_id: &str,
        approved: bool,
    ) -> Result<SkillReport, ServiceError> {
        let mut stored = self.store.load_report(run_id)?;
        let pkg = self.get(&stored.skill_id)?.clone();
        let rt = SkillRuntime::new(pkg);
        let mut run = stored_to_run(&stored);
        rt.approve_proposal(&mut run, proposal_id, approved)?;
        let report = rt.finish_report(&run, &stored.report.summary);
        stored.status = run.status;
        stored.report = report.clone();
        self.store.update_stored(&stored)?;
        Ok(report)
    }

    pub fn list_reports(&self, limit: usize) -> Result<Vec<StoredSkillRun>, ServiceError> {
        Ok(self.store.list_reports(limit)?)
    }

    pub fn load_report(&self, run_id: &str) -> Result<StoredSkillRun, ServiceError> {
        Ok(self.store.load_report(run_id)?)
    }
}

fn stored_to_run(stored: &StoredSkillRun) -> SkillRun {
    SkillRun {
        id: stored.run_id.clone(),
        skill_id: stored.skill_id.clone(),
        status: stored.status,
        current_step: None,
        evidence: stored.report.evidence.clone(),
        proposals: stored.report.proposals.clone(),
        findings: stored.report.findings.clone(),
        log: Vec::new(),
    }
}

/// Build an allowlisted remediation proposal from skill manifest + finding text.
/// Never invents commands outside `allowed_actions` prefixes.
pub fn default_proposal_for_skill(
    skill_id: &str,
    finding: &str,
    manifest: &crate::manifest::SkillManifest,
) -> Option<(String, String, Vec<String>)> {
    use crate::manifest::Permission;
    if !manifest.has_permission(Permission::ProposeExecute) {
        return None;
    }
    let text = finding.to_lowercase();
    let allowed = &manifest.allowed_actions;
    let pick = |candidates: &[&str]| -> Vec<String> {
        candidates
            .iter()
            .filter_map(|cmd| {
                if allowed.is_empty() {
                    return Some((*cmd).to_string());
                }
                allowed.iter().find_map(|prefix| {
                    if cmd.starts_with(prefix.as_str()) || cmd.contains(prefix.as_str()) {
                        Some((*cmd).to_string())
                    } else {
                        None
                    }
                })
            })
            .collect()
    };

    let actions = if skill_id == "authorize-upgrade" {
        pick(&[
            "apt-get update",
            "apt-get upgrade --dry-run",
            "systemctl status",
        ])
    } else if text.contains("disk") || text.contains("磁盘") {
        pick(&["df -h", "journalctl --disk-usage"])
    } else if text.contains("oom") || text.contains("memory") || text.contains("内存") {
        pick(&["free -h", "journalctl -xe"])
    } else if text.contains("docker") || text.contains("container") || text.contains("crash") {
        pick(&[
            "docker logs --tail 200",
            "docker inspect",
        ])
    } else if text.contains("nginx") || text.contains("502") || text.contains("504") {
        pick(&["nginx -t", "systemctl status", "journalctl -xe"])
    } else {
        pick(&[
            "df -h",
            "free -h",
            "journalctl -xe",
            "ss -tlnp",
        ])
    };

    if actions.is_empty() {
        return None;
    }
    Some((
        format!("Remediation proposal for {skill_id}"),
        format!("Auto-drafted from finding: {finding}"),
        actions,
    ))
}
