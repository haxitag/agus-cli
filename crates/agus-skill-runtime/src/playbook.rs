use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// Define or run a verification / baseline (TDO RED).
    Verify,
    /// Read-only collection from observer / host.
    Observe,
    /// LLM or deterministic analysis.
    Analyze,
    /// Produce a deployment / exec proposal (no side effects).
    Propose,
    /// Human gate — runtime pauses here.
    AwaitApproval,
    /// Reserved: only after approval, performed by agus-executor outside this crate.
    Execute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybookStep {
    pub id: String,
    pub kind: StepKind,
    pub title: String,
    #[serde(default)]
    pub description: String,
    /// Optional shell/observer template (never executed here for Execute kind).
    #[serde(default)]
    pub command_template: Option<String>,
    #[serde(default)]
    pub requires_evidence: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playbook {
    pub id: String,
    #[serde(default)]
    pub summary: String,
    pub steps: Vec<PlaybookStep>,
    #[serde(default)]
    pub pitfalls: Vec<String>,
}

impl Playbook {
    pub fn step(&self, id: &str) -> Option<&PlaybookStep> {
        self.steps.iter().find(|s| s.id == id)
    }
}
