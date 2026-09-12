use serde::{Deserialize, Serialize};

/// Capability granted to a skill. `Execute` never auto-runs without human approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Observe,
    Analyze,
    Plan,
    ProposeExecute,
    Execute,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observe => "observe",
            Self::Analyze => "analyze",
            Self::Plan => "plan",
            Self::ProposeExecute => "propose_execute",
            Self::Execute => "execute",
        }
    }

    pub fn allows_auto_start(self) -> bool {
        !matches!(self, Self::Execute)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskClass {
    Low,
    Medium,
    High,
    Critical,
}

impl RiskClass {
    pub fn rank(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
            Self::Critical => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillMaturity {
    Experimental,
    Stable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    Alert,
    Inspection,
    Schedule,
    User,
    Incident,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTrigger {
    pub kinds: Vec<TriggerKind>,
    #[serde(default)]
    pub phrases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillManifest {
    pub id: String,
    pub version: String,
    pub description: String,
    #[serde(default = "default_maturity")]
    pub maturity: SkillMaturity,
    #[serde(default)]
    pub permissions: Vec<Permission>,
    #[serde(default = "default_risk")]
    pub risk_class: RiskClass,
    pub triggers: SkillTrigger,
    /// Whitelisted action ids / command prefixes (fail-closed if empty for write paths).
    #[serde(default)]
    pub allowed_actions: Vec<String>,
    /// Situations that always require a human (cannot loosen global policy).
    #[serde(default)]
    pub requires_human_for: Vec<String>,
    /// Smallest useful outcome — prevents scope creep (codex-workflows lesson).
    #[serde(default)]
    pub outcome: String,
}

fn default_maturity() -> SkillMaturity {
    SkillMaturity::Experimental
}

fn default_risk() -> RiskClass {
    RiskClass::Medium
}

impl SkillManifest {
    pub fn has_permission(&self, need: Permission) -> bool {
        self.permissions.contains(&need)
    }

    /// Execute is never an auto permission even if listed — higher layers must approve.
    pub fn may_auto_run(&self) -> bool {
        !self.permissions.is_empty()
            && self.permissions.iter().all(|p| p.allows_auto_start())
            && !self.has_permission(Permission::Execute)
    }

    pub fn matches_phrase(&self, text: &str) -> bool {
        let lower = text.to_lowercase();
        self.triggers
            .phrases
            .iter()
            .any(|p| lower.contains(&p.to_lowercase()))
    }
}
