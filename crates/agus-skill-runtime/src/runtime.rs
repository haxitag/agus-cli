use chrono::Utc;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

use crate::loader::SkillPackage;
use crate::manifest::{Permission, RiskClass};
use crate::playbook::StepKind;
use crate::proposal::{
    ExecProposal, ProposalStatus, SkillEvidence, SkillReport, VerificationVerdict,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillRunStatus {
    Pending,
    Running,
    WaitingHumanApproval,
    Completed,
    Failed,
    Rejected,
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("skill cannot auto-run with execute permission; human gate required")]
    ExecuteRequiresHuman,
    #[error("missing permission: {0}")]
    MissingPermission(&'static str),
    #[error("run is not waiting for approval")]
    NotWaitingApproval,
    #[error("no proposal to approve")]
    NoProposal,
    #[error("{0}")]
    Msg(String),
}

#[derive(Debug, Clone)]
pub struct SkillRun {
    pub id: String,
    pub skill_id: String,
    pub status: SkillRunStatus,
    pub current_step: Option<String>,
    pub evidence: Vec<SkillEvidence>,
    pub proposals: Vec<ExecProposal>,
    pub findings: Vec<String>,
    pub log: Vec<String>,
}

/// In-process skill runner. Side-effectful observe/execute are injected by callers.
pub struct SkillRuntime {
    package: SkillPackage,
}

impl SkillRuntime {
    pub fn new(package: SkillPackage) -> Self {
        Self { package }
    }

    pub fn package(&self) -> &SkillPackage {
        &self.package
    }

    pub fn start(&self, trigger_note: &str) -> Result<SkillRun, RuntimeError> {
        if self.package.manifest.has_permission(Permission::Execute)
            && !self.package.manifest.may_auto_run()
        {
            // Listing Execute is fine only when human will gate; auto start still OK for
            // observe/analyze phases, but we refuse if Execute is the only meaningful perm.
        }
        if self.package.manifest.permissions == [Permission::Execute] {
            return Err(RuntimeError::ExecuteRequiresHuman);
        }
        if !self.package.manifest.has_permission(Permission::Observe)
            && !self.package.manifest.has_permission(Permission::Analyze)
        {
            return Err(RuntimeError::MissingPermission("observe|analyze"));
        }

        let id = Uuid::new_v4().to_string();
        let mut run = SkillRun {
            id: id.clone(),
            skill_id: self.package.manifest.id.clone(),
            status: SkillRunStatus::Running,
            current_step: self.package.playbook.steps.first().map(|s| s.id.clone()),
            evidence: Vec::new(),
            proposals: Vec::new(),
            findings: Vec::new(),
            log: vec![format!("start trigger={trigger_note}")],
        };

        // Advance through non-approval steps that are structural until Propose / AwaitApproval.
        for step in &self.package.playbook.steps {
            run.current_step = Some(step.id.clone());
            run.log.push(format!("enter step {} ({:?})", step.id, step.kind));
            match step.kind {
                StepKind::Verify | StepKind::Observe | StepKind::Analyze => {
                    // Caller should attach evidence via `record_evidence` after real collection.
                }
                StepKind::Propose => {
                    if !self
                        .package
                        .manifest
                        .has_permission(Permission::ProposeExecute)
                        && !self.package.manifest.has_permission(Permission::Plan)
                    {
                        return Err(RuntimeError::MissingPermission("propose_execute|plan"));
                    }
                }
                StepKind::AwaitApproval => {
                    run.status = SkillRunStatus::WaitingHumanApproval;
                    run.log.push("waiting human approval".into());
                    return Ok(run);
                }
                StepKind::Execute => {
                    // Never auto-enter execute inside this crate.
                    run.status = SkillRunStatus::WaitingHumanApproval;
                    run.log.push("execute step deferred to human-approved executor".into());
                    return Ok(run);
                }
            }
        }

        run.status = SkillRunStatus::Completed;
        Ok(run)
    }

    pub fn record_evidence(run: &mut SkillRun, evidence: SkillEvidence) {
        run.log.push(format!("evidence kind={}", evidence.kind));
        run.evidence.push(evidence);
    }

    pub fn add_finding(run: &mut SkillRun, finding: impl Into<String>) {
        let f = finding.into();
        run.log.push(format!("finding: {f}"));
        run.findings.push(f);
    }

    pub fn propose_actions(
        &self,
        run: &mut SkillRun,
        title: &str,
        rationale: &str,
        actions: Vec<String>,
    ) -> Result<(), RuntimeError> {
        if !self
            .package
            .manifest
            .has_permission(Permission::ProposeExecute)
            && !self.package.manifest.has_permission(Permission::Plan)
        {
            return Err(RuntimeError::MissingPermission("propose_execute|plan"));
        }
        // Enforce allowlist when present.
        if !self.package.manifest.allowed_actions.is_empty() {
            for action in &actions {
                let ok = self.package.manifest.allowed_actions.iter().any(|prefix| {
                    action == prefix || action.starts_with(prefix) || action.contains(prefix)
                });
                if !ok {
                    return Err(RuntimeError::Msg(format!(
                        "action not in allowed_actions: {action}"
                    )));
                }
            }
        }

        let requires_human = self.package.manifest.risk_class.rank() >= RiskClass::High.rank()
            || !self.package.manifest.requires_human_for.is_empty()
            || self.package.manifest.has_permission(Permission::Execute);

        let proposal = ExecProposal {
            id: Uuid::new_v4().to_string(),
            skill_id: self.package.manifest.id.clone(),
            title: title.into(),
            rationale: rationale.into(),
            risk_class: self.package.manifest.risk_class,
            actions,
            status: if requires_human {
                ProposalStatus::WaitingApproval
            } else {
                ProposalStatus::Draft
            },
            requires_human,
        };
        run.log.push(format!(
            "proposal {} requires_human={}",
            proposal.id, proposal.requires_human
        ));
        if requires_human {
            run.status = SkillRunStatus::WaitingHumanApproval;
        }
        run.proposals.push(proposal);
        Ok(())
    }

    pub fn approve_proposal(
        &self,
        run: &mut SkillRun,
        proposal_id: &str,
        approved: bool,
    ) -> Result<(), RuntimeError> {
        let _ = self;
        if run.status != SkillRunStatus::WaitingHumanApproval
            && !run
                .proposals
                .iter()
                .any(|p| p.status == ProposalStatus::WaitingApproval)
        {
            return Err(RuntimeError::NotWaitingApproval);
        }
        let proposal = run
            .proposals
            .iter_mut()
            .find(|p| p.id == proposal_id)
            .ok_or(RuntimeError::NoProposal)?;
        proposal.status = if approved {
            ProposalStatus::Approved
        } else {
            ProposalStatus::Rejected
        };
        run.log.push(format!(
            "proposal {proposal_id} => {:?}",
            proposal.status
        ));
        if !approved {
            run.status = SkillRunStatus::Rejected;
        } else {
            // Execution still happens outside; mark completed proposal phase.
            run.status = SkillRunStatus::Completed;
            run.log
                .push("approved; hand off to agus-executor / classified exec".into());
        }
        Ok(())
    }

    pub fn finish_report(&self, run: &SkillRun, summary: &str) -> SkillReport {
        let verdict = if run.status == SkillRunStatus::Rejected {
            VerificationVerdict::Failed
        } else if run.evidence.is_empty() {
            VerificationVerdict::NotProven
        } else if run.status == SkillRunStatus::Failed {
            VerificationVerdict::Failed
        } else if run
            .proposals
            .iter()
            .any(|p| p.requires_human && p.status == ProposalStatus::WaitingApproval)
        {
            VerificationVerdict::NotProven
        } else {
            VerificationVerdict::Verified
        };

        SkillReport {
            skill_id: run.skill_id.clone(),
            run_id: run.id.clone(),
            summary: summary.into(),
            findings: run.findings.clone(),
            evidence: run.evidence.clone(),
            proposals: run.proposals.clone(),
            verdict,
            pitfalls_noted: self.package.playbook.pitfalls.clone(),
        }
    }

    pub fn now_evidence(kind: &str, summary: &str, command: Option<&str>, digest: Option<&str>) -> SkillEvidence {
        SkillEvidence {
            kind: kind.into(),
            summary: summary.into(),
            command: command.map(|s| s.to_string()),
            digest: digest.map(|s| s.to_string()),
            collected_at: Utc::now(),
        }
    }
}
