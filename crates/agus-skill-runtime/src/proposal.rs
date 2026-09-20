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
    /// Fresh execution/observe evidence supports success claims.
    Verified,
    Failed,
    /// AgentOps lesson: missing / stale evidence is not success.
    NotProven,
    /// Human approved a proposal; remediation commands have NOT been verified yet.
    ApprovedPendingExecution,
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

/// Render a skill report as shared LLM/Copilot evidence context (multi-model delivery).
/// Does not invent facts — only serializes stored findings/evidence/proposals.
pub fn format_skill_report_for_copilot(report: &SkillReport) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "=== Agus Skill 证据 (skill={}, run={}, verdict={:?}) ===\n",
        report.skill_id, report.run_id, report.verdict
    ));
    if !report.summary.trim().is_empty() {
        out.push_str(&format!("摘要: {}\n", report.summary.trim()));
    }
    if !report.findings.is_empty() {
        out.push_str("发现:\n");
        for (i, f) in report.findings.iter().take(12).enumerate() {
            out.push_str(&format!("  {}. {}\n", i + 1, f));
        }
    }
    if !report.evidence.is_empty() {
        out.push_str("证据:\n");
        for ev in report.evidence.iter().rev().take(12) {
            out.push_str(&format!(
                "  - [{}] {}{}\n",
                ev.kind,
                ev.summary.chars().take(240).collect::<String>(),
                ev.digest
                    .as_ref()
                    .map(|d| format!(" digest={d}"))
                    .unwrap_or_default()
            ));
        }
    }
    if !report.proposals.is_empty() {
        out.push_str("提案（须人工审批后才可执行，勿当作已批准）:\n");
        for p in &report.proposals {
            out.push_str(&format!(
                "  - [{}] {} | status={:?} requires_human={} risk={:?}\n",
                p.id, p.title, p.status, p.requires_human, p.risk_class
            ));
            out.push_str(&format!("    理由: {}\n", p.rationale.chars().take(200).collect::<String>()));
            for a in p.actions.iter().take(8) {
                out.push_str(&format!("    • {a}\n"));
            }
        }
    }
    if !report.pitfalls_noted.is_empty() {
        out.push_str("已知坑:\n");
        for tip in report.pitfalls_noted.iter().take(5) {
            out.push_str(&format!("  - {tip}\n"));
        }
    }
    out.push_str("=== Skill 证据结束 ===\n");
    out
}

/// Fill `prompts/analyze.md` placeholders for multi-model diagnose delivery.
pub fn render_analyze_prompt_template(
    template: &str,
    findings: &[String],
    evidence: &[SkillEvidence],
) -> String {
    let findings_block = if findings.is_empty() {
        "(none)".to_string()
    } else {
        findings
            .iter()
            .enumerate()
            .map(|(i, f)| format!("{}. {f}", i + 1))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let evidence_block = if evidence.is_empty() {
        "(none)".to_string()
    } else {
        evidence
            .iter()
            .map(|e| {
                format!(
                    "- [{}] {}{}",
                    e.kind,
                    e.summary,
                    e.digest
                        .as_ref()
                        .map(|d| format!(" ({d})"))
                        .unwrap_or_default()
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    template
        .replace("{{findings}}", &findings_block)
        .replace("{{alert}}", &findings_block)
        .replace("{{evidence}}", &evidence_block)
        .replace("{{context}}", &format!("{findings_block}\n{evidence_block}"))
}
