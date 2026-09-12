use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::manifest::RiskClass;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEvidence {
    pub kind: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub command: Option<String>,
    /// Short digest / truncated stdout — never store secrets.
    #[serde(default)]
    pub digest: Option<String>,
    pub collected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalStatus {
    Draft,
    WaitingApproval,
    Approved,
    Rejected,
}

/// A proposed write action. Never executed by this crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecProposal {
    pub id: String,
    pub skill_id: String,
    pub title: String,
    pub rationale: String,
    pub risk_class: RiskClass,
    /// Commands or deployment step sketches for human review.
    pub actions: Vec<String>,
    pub status: ProposalStatus,
    #[serde(default)]
    pub requires_human: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationVerdict {
    /// Fresh evidence supports success claims.
    Verified,
    Failed,
    /// AgentOps lesson: missing / stale evidence is not success.
    NotProven,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillReport {
    pub skill_id: String,
    pub run_id: String,
    pub summary: String,
    pub findings: Vec<String>,
    pub evidence: Vec<SkillEvidence>,
    pub proposals: Vec<ExecProposal>,
    pub verdict: VerificationVerdict,
    #[serde(default)]
    pub pitfalls_noted: Vec<String>,
}
