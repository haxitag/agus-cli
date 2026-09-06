use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

// Remove Copy from LogLevel and LogStream if they contain non-Copy types

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub name: String,
    pub metric_type: MetricType,
    pub threshold: f64,
    pub comparison: Comparison,
    pub severity: AlertSeverity,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MetricType {
    CpuUsage,
    MemoryUsage,
    DiskUsage,
    NetworkErrorRate,
    ContainerRestartCount,
    ContainerUptime,
    Custom(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Comparison {
    GreaterThan,
    LessThan,
    Equal,
    NotEqual,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AlertSeverity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub rule_id: String,
    pub rule_name: String,
    pub severity: AlertSeverity,
    pub message: String,
    pub metric_value: f64,
    pub threshold: f64,
    pub timestamp: u64,
    pub resource_id: Option<String>,
    pub resource_type: Option<String>,
    pub acknowledged: bool,
}

pub trait AlertManager: Send + Sync {
    /// Add or update an alert rule
    fn add_rule(&mut self, rule: AlertRule) -> Result<(), AlertError>;

    /// Remove an alert rule
    fn remove_rule(&mut self, rule_id: &str) -> Result<(), AlertError>;

    /// Get all alert rules
    fn get_rules(&self) -> Vec<&AlertRule>;

    /// Evaluate metrics against rules and generate alerts
    fn evaluate(&mut self, metrics: &AlertMetrics) -> Result<Vec<Alert>, AlertError>;

    /// Get active alerts
    fn get_active_alerts(&self) -> Vec<&Alert>;

    /// Acknowledge an alert
    fn acknowledge_alert(&mut self, alert_id: &str) -> Result<(), AlertError>;

    /// Clear an alert
    fn clear_alert(&mut self, alert_id: &str) -> Result<(), AlertError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertMetrics {
    pub cpu_usage_percent: Option<f64>,
    pub memory_usage_percent: Option<f64>,
    pub disk_usage_percent: Option<f64>,
    pub network_error_rate: Option<f64>,
    pub container_restart_count: Option<u32>,
    pub container_uptime_seconds: Option<u64>,
    pub custom_metrics: HashMap<String, f64>,
    pub resource_id: Option<String>,
    pub resource_type: Option<String>,
}

#[derive(Debug)]
pub enum AlertError {
    RuleNotFound { rule_id: String },
    AlertNotFound { alert_id: String },
    InvalidRule { message: String },
    IoError { message: String },
}

impl std::fmt::Display for AlertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AlertError::RuleNotFound { rule_id } => {
                write!(f, "Alert rule not found: {}", rule_id)
            }
            AlertError::AlertNotFound { alert_id } => {
                write!(f, "Alert not found: {}", alert_id)
            }
            AlertError::InvalidRule { message } => {
                write!(f, "Invalid alert rule: {}", message)
            }
            AlertError::IoError { message } => {
                write!(f, "IO error: {}", message)
            }
        }
    }
}

impl std::error::Error for AlertError {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AlertStoreState {
    pub rules: Vec<AlertRule>,
    #[serde(default)]
    pub active: Vec<Alert>,
}

pub struct InMemoryAlertManager {
    rules: HashMap<String, AlertRule>,
    active_alerts: HashMap<String, Alert>,
}

impl InMemoryAlertManager {
    pub fn new() -> Self {
        Self {
            rules: HashMap::new(),
            active_alerts: HashMap::new(),
        }
    }

    /// Snapshot compatible with CLI `alert_store.json` (`rules` + `active`).
    pub fn to_store_state(&self) -> AlertStoreState {
        AlertStoreState {
            rules: self.rules.values().cloned().collect(),
            active: self.active_alerts.values().cloned().collect(),
        }
    }

    pub fn from_store_state(state: AlertStoreState) -> Self {
        let mut manager = Self::new();
        for rule in state.rules {
            manager.rules.insert(rule.id.clone(), rule);
        }
        for alert in state.active {
            let key = format!("{}_{:?}", alert.rule_id, alert.resource_id);
            manager.active_alerts.insert(key, alert);
        }
        manager
    }

    fn evaluate_rule(&self, rule: &AlertRule, metrics: &AlertMetrics) -> Option<Alert> {
        if !rule.enabled {
            return None;
        }

        let value = match rule.metric_type {
            MetricType::CpuUsage => metrics.cpu_usage_percent?,
            MetricType::MemoryUsage => metrics.memory_usage_percent?,
            MetricType::DiskUsage => metrics.disk_usage_percent?,
            MetricType::NetworkErrorRate => metrics.network_error_rate?,
            MetricType::ContainerRestartCount => metrics.container_restart_count? as f64,
            MetricType::ContainerUptime => metrics.container_uptime_seconds? as f64,
            MetricType::Custom(ref key) => metrics.custom_metrics.get(key)?.clone(),
        };

        let triggered = match rule.comparison {
            Comparison::GreaterThan => value > rule.threshold,
            Comparison::LessThan => value < rule.threshold,
            Comparison::Equal => (value - rule.threshold).abs() < 0.001,
            Comparison::NotEqual => (value - rule.threshold).abs() >= 0.001,
        };

        if triggered {
            Some(Alert {
                id: format!(
                    "alert_{}",
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_nanos()
                        .to_string()
                ),
                rule_id: rule.id.clone(),
                rule_name: rule.name.clone(),
                severity: rule.severity,
                message: format!(
                    "{}: {} threshold {} (value: {:.2}, threshold: {:.2})",
                    rule.name,
                    match rule.comparison {
                        Comparison::GreaterThan => "exceeded",
                        Comparison::LessThan => "below",
                        Comparison::Equal => "equals",
                        Comparison::NotEqual => "not equals",
                    },
                    rule.threshold,
                    value,
                    rule.threshold
                ),
                metric_value: value,
                threshold: rule.threshold,
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                resource_id: metrics.resource_id.clone(),
                resource_type: metrics.resource_type.clone(),
                acknowledged: false,
            })
        } else {
            None
        }
    }
}

impl AlertManager for InMemoryAlertManager {
    fn add_rule(&mut self, rule: AlertRule) -> Result<(), AlertError> {
        if rule.threshold < 0.0 {
            return Err(AlertError::InvalidRule {
                message: "Threshold must be non-negative".to_string(),
            });
        }
        self.rules.insert(rule.id.clone(), rule);
        Ok(())
    }

    fn remove_rule(&mut self, rule_id: &str) -> Result<(), AlertError> {
        self.rules
            .remove(rule_id)
            .ok_or_else(|| AlertError::RuleNotFound {
                rule_id: rule_id.to_string(),
            })?;
        // Also remove any active alerts for this rule
        self.active_alerts
            .retain(|_, alert| alert.rule_id != rule_id);
        Ok(())
    }

    fn get_rules(&self) -> Vec<&AlertRule> {
        self.rules.values().collect()
    }

    fn evaluate(&mut self, metrics: &AlertMetrics) -> Result<Vec<Alert>, AlertError> {
        let mut new_alerts = Vec::new();
        // 本次评估中恢复（不再触发）的规则，评估后从 active 中清除，
        // 使同一 rule+resource 的"故障→恢复→再次故障"能够再次产生告警，
        // 而不是只告警一次后被永久吞掉。
        let mut recovered_keys: Vec<String> = Vec::new();

        for rule in self.rules.values() {
            let alert_key = format!("{}_{:?}", rule.id, metrics.resource_id);
            if let Some(alert) = self.evaluate_rule(rule, metrics) {
                // 已处于 active 的同类告警不重复产生，保持去重
                if !self.active_alerts.contains_key(&alert_key) {
                    self.active_alerts.insert(alert_key, alert.clone());
                    new_alerts.push(alert);
                }
            } else if self.active_alerts.contains_key(&alert_key) {
                // 阈值回落/规则禁用 => 自动清除 active 状态（自恢复）
                recovered_keys.push(alert_key);
            }
        }

        for key in recovered_keys {
            self.active_alerts.remove(&key);
        }

        Ok(new_alerts)
    }

    fn get_active_alerts(&self) -> Vec<&Alert> {
        self.active_alerts.values().collect()
    }

    fn acknowledge_alert(&mut self, alert_id: &str) -> Result<(), AlertError> {
        for alert in self.active_alerts.values_mut() {
            if alert.id == alert_id {
                alert.acknowledged = true;
                return Ok(());
            }
        }
        Err(AlertError::AlertNotFound {
            alert_id: alert_id.to_string(),
        })
    }

    fn clear_alert(&mut self, alert_id: &str) -> Result<(), AlertError> {
        let key = self
            .active_alerts
            .iter()
            .find(|(_, alert)| alert.id == alert_id)
            .map(|(k, _)| k.clone())
            .ok_or_else(|| AlertError::AlertNotFound {
                alert_id: alert_id.to_string(),
            })?;
        self.active_alerts.remove(&key);
        Ok(())
    }
}

impl Default for InMemoryAlertManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu_rule(threshold: f64) -> AlertRule {
        AlertRule {
            id: "rule-cpu".to_string(),
            name: "CPU 高负载".to_string(),
            metric_type: MetricType::CpuUsage,
            threshold,
            comparison: Comparison::GreaterThan,
            severity: AlertSeverity::Critical,
            enabled: true,
        }
    }

    fn metrics(cpu: f64) -> AlertMetrics {
        AlertMetrics {
            cpu_usage_percent: Some(cpu),
            memory_usage_percent: None,
            disk_usage_percent: None,
            network_error_rate: None,
            container_restart_count: None,
            container_uptime_seconds: None,
            custom_metrics: HashMap::new(),
            resource_id: Some("host1".to_string()),
            resource_type: Some("host".to_string()),
        }
    }

    #[test]
    fn repeated_breach_after_recovery_fires_again() {
        let mut m = InMemoryAlertManager::new();
        m.add_rule(cpu_rule(90.0)).unwrap();

        // 1) 首次超限 -> 1 条新告警
        assert_eq!(m.evaluate(&metrics(95.0)).unwrap().len(), 1);
        assert_eq!(m.get_active_alerts().len(), 1);

        // 2) 持续超限 -> 去重，不重复告警
        assert_eq!(m.evaluate(&metrics(96.0)).unwrap().len(), 0);

        // 3) 回落恢复 -> 不产生新告警，且 active 被清除
        assert_eq!(m.evaluate(&metrics(50.0)).unwrap().len(), 0);
        assert_eq!(m.get_active_alerts().len(), 0);

        // 4) 恢复后再次超限 -> 必须能再次告警（修复前此步永远丢失）
        assert_eq!(m.evaluate(&metrics(97.0)).unwrap().len(), 1);
        assert_eq!(m.get_active_alerts().len(), 1);
    }

    #[test]
    fn disabled_rule_is_auto_cleared() {
        let mut m = InMemoryAlertManager::new();
        let mut rule = cpu_rule(90.0);
        m.add_rule(rule.clone()).unwrap();
        assert_eq!(m.evaluate(&metrics(95.0)).unwrap().len(), 1);

        // 禁用规则后，下一轮评估将其 active 告警清除
        rule.enabled = false;
        m.add_rule(rule).unwrap();
        assert_eq!(m.evaluate(&metrics(95.0)).unwrap().len(), 0);
        assert_eq!(m.get_active_alerts().len(), 0);
    }
}
