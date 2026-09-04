pub mod log_stream;

use agus_core_domain::{
    render_nginx_config, ApprovalDecision, DatabaseType, DbConnectionConfig, DeploymentAction,
    DeploymentPlan, DeploymentStep, Environment, ExecutionRecord, ExecutionStatus, HostContext,
    NginxProjectConfig, PlanValidationError, SslProjectConfig,
};
use agus_planner::llm::LlmProvider;
use agus_ssh::{SshClient, SshError, SshOutputStream, SshTarget};
use log_stream::LogStreamSender;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum ExecutionError {
    InvalidPlan { message: String },
    UnknownStep { step_id: String },
    InvalidState { message: String },
    SessionError { message: String },
    RollbackNotSupported { message: String },
    PermissionDenied { message: String },
}

impl std::fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionError::InvalidPlan { message } => write!(f, "invalid plan: {message}"),
            ExecutionError::UnknownStep { step_id } => write!(f, "unknown step: {step_id}"),
            ExecutionError::InvalidState { message } => write!(f, "invalid state: {message}"),
            ExecutionError::SessionError { message } => write!(f, "session error: {message}"),
            ExecutionError::RollbackNotSupported { message } => {
                write!(f, "rollback not supported: {message}")
            }
            ExecutionError::PermissionDenied { message } => {
                write!(f, "permission denied: {message}")
            }
        }
    }
}

impl std::error::Error for ExecutionError {}

impl From<PlanValidationError> for ExecutionError {
    fn from(err: PlanValidationError) -> Self {
        ExecutionError::InvalidPlan {
            message: err.to_string(),
        }
    }
}

pub struct Executor {
    failure_steps: HashSet<String>,
}

#[derive(Debug, Clone)]
pub struct DbMigrationSpec {
    pub plan_id: String,
    pub connection: DbConnectionConfig,
    pub sql: String,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            failure_steps: HashSet::new(),
        }
    }

    pub fn with_failure_steps<I>(steps: I) -> Self
    where
        I: IntoIterator<Item = String>,
    {
        Self {
            failure_steps: steps.into_iter().collect(),
        }
    }

    pub fn start_dry_run(&self, plan: DeploymentPlan) -> Result<ExecutionSession, ExecutionError> {
        ExecutionSession::new(
            plan,
            ExecutionMode::DryRun {
                failure_steps: self.failure_steps.clone(),
            },
        )
    }

    pub fn resume_dry_run<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<ExecutionSession, ExecutionError> {
        let checkpoint_path = path.as_ref().to_path_buf();
        ExecutionSession::load_checkpoint(
            &checkpoint_path,
            ExecutionMode::DryRun {
                failure_steps: self.failure_steps.clone(),
            },
        )
    }

    pub fn start_ssh_readonly<C: SshClient + Send + Sync + 'static>(
        &self,
        plan: DeploymentPlan,
        host: HostContext,
        client: C,
    ) -> Result<ExecutionSession, ExecutionError> {
        ExecutionSession::new(
            plan,
            ExecutionMode::SshReadonly {
                host,
                client: Box::new(client),
            },
        )
    }

    pub fn resume_ssh_readonly<C: SshClient + Send + Sync + 'static, P: AsRef<Path>>(
        &self,
        path: P,
        host: HostContext,
        client: C,
    ) -> Result<ExecutionSession, ExecutionError> {
        let checkpoint_path = path.as_ref().to_path_buf();
        ExecutionSession::load_checkpoint(
            &checkpoint_path,
            ExecutionMode::SshReadonly {
                host,
                client: Box::new(client),
            },
        )
    }

    pub fn start_docker_readonly<C: SshClient + Send + Sync + 'static>(
        &self,
        plan: DeploymentPlan,
        host: HostContext,
        client: C,
    ) -> Result<ExecutionSession, ExecutionError> {
        ExecutionSession::new(
            plan,
            ExecutionMode::DockerReadonly {
                host,
                client: Box::new(client),
            },
        )
    }

    pub fn resume_docker_readonly<C: SshClient + Send + Sync + 'static, P: AsRef<Path>>(
        &self,
        path: P,
        host: HostContext,
        client: C,
    ) -> Result<ExecutionSession, ExecutionError> {
        let checkpoint_path = path.as_ref().to_path_buf();
        ExecutionSession::load_checkpoint(
            &checkpoint_path,
            ExecutionMode::DockerReadonly {
                host,
                client: Box::new(client),
            },
        )
    }

    pub fn start_compose<C: SshClient + Send + Sync + 'static>(
        &self,
        plan: DeploymentPlan,
        host: HostContext,
        client: C,
    ) -> Result<ExecutionSession, ExecutionError> {
        ExecutionSession::new(
            plan,
            ExecutionMode::Compose {
                host,
                client: Box::new(client),
            },
        )
    }

    pub fn start_compose_with_log_stream<C: SshClient + Send + Sync + 'static>(
        &self,
        plan: DeploymentPlan,
        host: HostContext,
        client: C,
        log_stream: Option<LogStreamSender>,
    ) -> Result<ExecutionSession, ExecutionError> {
        ExecutionSession::new_with_log_stream(
            plan,
            ExecutionMode::Compose {
                host,
                client: Box::new(client),
            },
            log_stream,
        )
    }

    pub fn resume_compose<C: SshClient + Send + Sync + 'static, P: AsRef<Path>>(
        &self,
        path: P,
        host: HostContext,
        client: C,
        log_stream: Option<LogStreamSender>,
    ) -> Result<ExecutionSession, ExecutionError> {
        let checkpoint_path = path.as_ref().to_path_buf();
        ExecutionSession::load_checkpoint(
            &checkpoint_path,
            ExecutionMode::Compose {
                host,
                client: Box::new(client),
            },
        )
        .map(|session| session.with_log_stream(log_stream))
    }

    pub fn start_nginx<C: SshClient + Send + Sync + 'static>(
        &self,
        plan: DeploymentPlan,
        host: HostContext,
        client: C,
        configs: Vec<NginxProjectConfig>,
    ) -> Result<ExecutionSession, ExecutionError> {
        let mut config_map = HashMap::new();
        for config in configs {
            if config_map.contains_key(&config.project) {
                return Err(ExecutionError::InvalidPlan {
                    message: format!("duplicate nginx project {}", config.project),
                });
            }
            config_map.insert(config.project.clone(), config);
        }
        ExecutionSession::new(
            plan,
            ExecutionMode::Nginx {
                host,
                client: Box::new(client),
                configs: config_map,
            },
        )
    }

    pub fn start_ssl<C: SshClient + Send + Sync + 'static>(
        &self,
        plan: DeploymentPlan,
        host: HostContext,
        client: C,
        configs: Vec<SslProjectConfig>,
    ) -> Result<ExecutionSession, ExecutionError> {
        let mut config_map = HashMap::new();
        for config in configs {
            if config_map.contains_key(&config.project) {
                return Err(ExecutionError::InvalidPlan {
                    message: format!("duplicate ssl project {}", config.project),
                });
            }
            config_map.insert(config.project.clone(), config);
        }
        ExecutionSession::new(
            plan,
            ExecutionMode::Ssl {
                host,
                client: Box::new(client),
                configs: config_map,
            },
        )
    }

    pub fn start_db_migration<C: SshClient + Send + Sync + 'static>(
        &self,
        plan: DeploymentPlan,
        host: HostContext,
        client: C,
        migrations: Vec<DbMigrationSpec>,
    ) -> Result<ExecutionSession, ExecutionError> {
        let mut migration_map = HashMap::new();
        for migration in migrations {
            if migration_map.contains_key(&migration.plan_id) {
                return Err(ExecutionError::InvalidPlan {
                    message: format!("duplicate migration plan {}", migration.plan_id),
                });
            }
            migration_map.insert(migration.plan_id.clone(), migration);
        }

        ExecutionSession::new(
            plan,
            ExecutionMode::DbMigration {
                host,
                client: Box::new(client),
                migrations: migration_map,
            },
        )
    }
}

pub type ProgressCallback = Box<dyn Fn(&ExecutionRecord) + Send + Sync>;

pub struct ExecutionSession {
    steps_by_id: HashMap<String, DeploymentStep>,
    order: Vec<String>,
    index: usize,
    halted: bool,
    statuses: HashMap<String, ExecutionStatus>,
    records: Vec<ExecutionRecord>,
    mode: ExecutionMode,
    progress_callback: Option<ProgressCallback>,
    checkpoint_path: Option<PathBuf>,
    log_stream: Option<LogStreamSender>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExecutionCheckpoint {
    steps_by_id: HashMap<String, DeploymentStep>,
    order: Vec<String>,
    index: usize,
    halted: bool,
    statuses: HashMap<String, ExecutionStatus>,
    records: Vec<ExecutionRecord>,
    execution_id: String,
    host_id: String,
    environment: Environment,
    mode_type: String, // "DryRun", "Compose", etc.
}

pub(crate) enum ExecutionMode {
    DryRun {
        failure_steps: HashSet<String>,
    },
    SshReadonly {
        host: HostContext,
        client: Box<dyn SshClient + Send + Sync>,
    },
    DockerReadonly {
        host: HostContext,
        client: Box<dyn SshClient + Send + Sync>,
    },
    Compose {
        host: HostContext,
        client: Box<dyn SshClient + Send + Sync>,
    },
    Nginx {
        host: HostContext,
        client: Box<dyn SshClient + Send + Sync>,
        configs: HashMap<String, NginxProjectConfig>,
    },
    Ssl {
        host: HostContext,
        client: Box<dyn SshClient + Send + Sync>,
        configs: HashMap<String, SslProjectConfig>,
    },
    DbMigration {
        host: HostContext,
        client: Box<dyn SshClient + Send + Sync>,
        migrations: HashMap<String, DbMigrationSpec>,
    },
}

#[derive(Debug, Clone)]
struct StepOutcome {
    status: ExecutionStatus,
    logs: Vec<String>,
    error: Option<String>,
}

impl ExecutionSession {
    fn new(plan: DeploymentPlan, mode: ExecutionMode) -> Result<Self, ExecutionError> {
        Self::new_with_log_stream(plan, mode, None)
    }

    pub(crate) fn new_with_log_stream(
        plan: DeploymentPlan,
        mode: ExecutionMode,
        log_stream: Option<LogStreamSender>,
    ) -> Result<Self, ExecutionError> {
        let mut plan = plan;
        plan.materialize_symbolic_compose_actions()
            .map_err(|message| ExecutionError::InvalidPlan { message })?;
        if matches!(mode, ExecutionMode::Compose { .. }) && plan.has_symbolic_compose_actions() {
            return Err(ExecutionError::InvalidPlan {
                message: "plan still contains DeployService/VerifyService after materialization"
                    .to_string(),
            });
        }
        if let ExecutionMode::Compose { .. } = &mode {
            for step in &plan.steps {
                match &step.action {
                    DeploymentAction::RunSshCommand { .. }
                    | DeploymentAction::ComposeUp { .. }
                    | DeploymentAction::ComposeDown { .. } => {}
                    other => {
                        return Err(ExecutionError::InvalidPlan {
                            message: format!(
                                "action {other:?} is not supported in compose mode (refusing to start)"
                            ),
                        });
                    }
                }
            }
        }
        plan.validate()?;
        let mut steps_by_id = HashMap::new();
        for step in plan.steps {
            if steps_by_id.contains_key(&step.id) {
                return Err(ExecutionError::InvalidPlan {
                    message: format!("duplicate step id {}", step.id),
                });
            }
            steps_by_id.insert(step.id.clone(), step);
        }

        let order = topo_sort_steps(&steps_by_id)?;

        Ok(Self {
            steps_by_id,
            order,
            index: 0,
            halted: false,
            statuses: HashMap::new(),
            records: Vec::new(),
            mode,
            progress_callback: None,
            checkpoint_path: None,
            log_stream,
        })
    }

    pub fn with_progress_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(&ExecutionRecord) + Send + Sync + 'static,
    {
        self.progress_callback = Some(Box::new(callback));
        self
    }

    pub fn with_log_stream(mut self, log_stream: Option<LogStreamSender>) -> Self {
        self.log_stream = log_stream;
        self
    }

    pub fn with_checkpoint(mut self, path: PathBuf) -> Self {
        self.checkpoint_path = Some(path);
        self
    }

    pub fn save_checkpoint(
        &self,
        execution_id: &str,
        host_id: &str,
        environment: Environment,
    ) -> Result<(), ExecutionError> {
        if let Some(ref checkpoint_path) = self.checkpoint_path {
            let mode_type = match &self.mode {
                ExecutionMode::DryRun { .. } => "DryRun",
                ExecutionMode::SshReadonly { .. } => "SshReadonly",
                ExecutionMode::DockerReadonly { .. } => "DockerReadonly",
                ExecutionMode::Compose { .. } => "Compose",
                ExecutionMode::Nginx { .. } => "Nginx",
                ExecutionMode::Ssl { .. } => "Ssl",
                ExecutionMode::DbMigration { .. } => "DbMigration",
            };

            let checkpoint = ExecutionCheckpoint {
                steps_by_id: self.steps_by_id.clone(),
                order: self.order.clone(),
                index: self.index,
                halted: self.halted,
                statuses: self.statuses.clone(),
                records: self.records.clone(),
                execution_id: execution_id.to_string(),
                host_id: host_id.to_string(),
                environment,
                mode_type: mode_type.to_string(),
            };

            if let Some(parent) = checkpoint_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| ExecutionError::SessionError {
                    message: format!("failed to create checkpoint directory: {}", e),
                })?;
            }

            let content = serde_json::to_string_pretty(&checkpoint).map_err(|e| {
                ExecutionError::SessionError {
                    message: format!("failed to serialize checkpoint: {}", e),
                }
            })?;

            std::fs::write(checkpoint_path, content).map_err(|e| ExecutionError::SessionError {
                message: format!("failed to write checkpoint: {}", e),
            })?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn load_checkpoint(
        path: &PathBuf,
        mode: ExecutionMode,
    ) -> Result<Self, ExecutionError> {
        let content = std::fs::read_to_string(path).map_err(|e| ExecutionError::SessionError {
            message: format!("failed to read checkpoint: {}", e),
        })?;

        let checkpoint: ExecutionCheckpoint =
            serde_json::from_str(&content).map_err(|e| ExecutionError::SessionError {
                message: format!("failed to parse checkpoint: {}", e),
            })?;

        // Verify mode type matches
        let expected_mode_type = match &mode {
            ExecutionMode::DryRun { .. } => "DryRun",
            ExecutionMode::SshReadonly { .. } => "SshReadonly",
            ExecutionMode::DockerReadonly { .. } => "DockerReadonly",
            ExecutionMode::Compose { .. } => "Compose",
            ExecutionMode::Nginx { .. } => "Nginx",
            ExecutionMode::Ssl { .. } => "Ssl",
            ExecutionMode::DbMigration { .. } => "DbMigration",
        };

        if checkpoint.mode_type != expected_mode_type {
            return Err(ExecutionError::SessionError {
                message: format!(
                    "checkpoint mode type mismatch: expected {}, got {}",
                    expected_mode_type, checkpoint.mode_type
                ),
            });
        }

        Ok(Self {
            steps_by_id: checkpoint.steps_by_id,
            order: checkpoint.order,
            index: checkpoint.index,
            halted: checkpoint.halted,
            statuses: checkpoint.statuses,
            records: checkpoint.records,
            mode,
            progress_callback: None,
            checkpoint_path: Some(path.clone()),
            log_stream: None, // Checkpoint doesn't preserve log stream
        })
    }

    pub fn run(&mut self) -> Result<Vec<ExecutionRecord>, ExecutionError> {
        self.run_until_pause(false)?;
        Ok(self.records.clone())
    }

    /// 逐步执行：每完成一步或进入 WaitingApproval 即暂停。
    pub fn run_one_step(&mut self) -> Result<Vec<ExecutionRecord>, ExecutionError> {
        self.run_until_pause(true)?;
        Ok(self.records.clone())
    }

    pub fn is_halted(&self) -> bool {
        self.halted
    }

    pub fn is_complete(&self) -> bool {
        self.halted || self.index >= self.order.len()
    }

    pub fn pending_approval_step(&self) -> Option<String> {
        self.records
            .iter()
            .rev()
            .find(|r| r.status == ExecutionStatus::WaitingApproval)
            .map(|r| r.step_id.clone())
            .or_else(|| {
                if self.index < self.order.len() {
                    let id = &self.order[self.index];
                    if self.statuses.get(id) == Some(&ExecutionStatus::WaitingApproval) {
                        return Some(id.clone());
                    }
                }
                None
            })
    }

    pub fn last_record(&self) -> Option<&ExecutionRecord> {
        self.records.last()
    }

    pub fn records(&self) -> &[ExecutionRecord] {
        &self.records
    }

    pub fn approve_step(&mut self, step_id: &str) -> Result<Vec<ExecutionRecord>, ExecutionError> {
        self.resolve_approval(step_id, ApprovalDecision::Approved)?;
        self.run_until_pause(false)?;
        Ok(self.records.clone())
    }

    pub fn reject_step(&mut self, step_id: &str) -> Result<Vec<ExecutionRecord>, ExecutionError> {
        self.resolve_approval(step_id, ApprovalDecision::Rejected)?;
        self.run_until_pause(false)?;
        Ok(self.records.clone())
    }

    pub fn rollback_step(&mut self, step_id: &str) -> Result<Vec<ExecutionRecord>, ExecutionError> {
        let status = self.status_for(step_id)?;
        if status != ExecutionStatus::Succeeded {
            return Err(ExecutionError::InvalidState {
                message: format!("step {step_id} is not completed"),
            });
        }

        let step = self
            .steps_by_id
            .get(step_id)
            .ok_or_else(|| ExecutionError::UnknownStep {
                step_id: step_id.to_string(),
            })?;

        match &step.action {
            DeploymentAction::ComposeUp { project_dir } => match &self.mode {
                ExecutionMode::Compose { host, client } => {
                    let command = build_compose_command(project_dir, ComposeCommand::Down)
                        .map_err(|message| ExecutionError::RollbackNotSupported { message })?;
                    let target = host_target(host);
                    let mut logs = vec![format!("Rollback step {step_id}")];
                    if let Some(ref stream) = self.log_stream {
                        stream.warning(&step_id, &format!("Rollback step {}", step_id));
                    }
                    let outcome = run_ssh_command_with_log(
                        client.as_ref(),
                        &target,
                        &command,
                        self.log_stream.as_ref(),
                        &step_id,
                    );

                    logs.extend(outcome.logs);
                    let final_status = if outcome.status == ExecutionStatus::Succeeded {
                        ExecutionStatus::RolledBack
                    } else {
                        ExecutionStatus::Failed
                    };
                    let record = ExecutionRecord {
                        step_id: step_id.to_string(),
                        status: final_status.clone(),
                        started_at: None,
                        finished_at: None,
                        logs,
                        error: outcome.error,
                    };
                    self.records.push(record);
                    self.statuses.insert(step_id.to_string(), final_status);
                    Ok(self.records.clone())
                }
                _ => Err(ExecutionError::RollbackNotSupported {
                    message: "compose rollback requires compose execution mode".to_string(),
                }),
            },
            _ => Err(ExecutionError::RollbackNotSupported {
                message: format!("rollback not supported for step {step_id}"),
            }),
        }
    }

    fn run_until_pause(&mut self, pause_after_each_step: bool) -> Result<(), ExecutionError> {
        while self.index < self.order.len() {
            if self.halted {
                break;
            }
            let step_id = self.order[self.index].clone();
            let status = self.status_for(&step_id)?;

            if status == ExecutionStatus::WaitingApproval {
                break;
            }

            if matches!(
                status,
                ExecutionStatus::Succeeded
                    | ExecutionStatus::Simulated
                    | ExecutionStatus::Failed
                    | ExecutionStatus::Skipped
                    | ExecutionStatus::RolledBack
            ) {
                self.index = self.index.saturating_add(1);
                continue;
            }

            let step = self
                .steps_by_id
                .get(&step_id)
                .ok_or_else(|| ExecutionError::UnknownStep {
                    step_id: step_id.clone(),
                })?
                .clone();

            if self.dependencies_block_step(&step)? {
                let record = ExecutionRecord {
                    step_id: step_id.clone(),
                    status: ExecutionStatus::Skipped,
                    started_at: None,
                    finished_at: None,
                    logs: vec![format!("Skipping step {step_id} due to failed dependency")],
                    error: None,
                };
                self.records.push(record);
                self.statuses
                    .insert(step_id.clone(), ExecutionStatus::Skipped);
                self.index = self.index.saturating_add(1);
                if pause_after_each_step {
                    break;
                }
                continue;
            }

            if matches!(step.action, DeploymentAction::NginxApply { .. }) && !step.approval_required
            {
                return Err(ExecutionError::InvalidPlan {
                    message: format!("nginx apply step {step_id} requires approval"),
                });
            }

            if matches!(
                step.action,
                DeploymentAction::SslIssue { .. }
                    | DeploymentAction::SslRenew { .. }
                    | DeploymentAction::SslRollback { .. }
                    | DeploymentAction::DbMigrate { .. }
            ) && !step.approval_required
            {
                return Err(ExecutionError::InvalidPlan {
                    message: format!("step {step_id} requires approval"),
                });
            }

            if self.requires_approval(&step) {
                let record = ExecutionRecord {
                    step_id: step_id.clone(),
                    status: ExecutionStatus::WaitingApproval,
                    started_at: None,
                    finished_at: None,
                    logs: vec![format!("Waiting approval for step {step_id}")],
                    error: None,
                };
                self.records.push(record);
                self.statuses
                    .insert(step_id.clone(), ExecutionStatus::WaitingApproval);
                break;
            }

            let outcome = self.execute_step(&step);
            let failed = outcome.status == ExecutionStatus::Failed
                && matches!(
                    step.action,
                    DeploymentAction::NginxApply { .. }
                        | DeploymentAction::NginxRollback { .. }
                        | DeploymentAction::SslIssue { .. }
                        | DeploymentAction::SslRenew { .. }
                        | DeploymentAction::SslRollback { .. }
                        | DeploymentAction::DbMigrate { .. }
                );
            self.record_execution(&step_id, outcome);
            self.index = self.index.saturating_add(1);
            if failed {
                self.halted = true;
                break;
            }
            if pause_after_each_step {
                break;
            }
        }

        Ok(())
    }

    fn requires_approval(&self, step: &DeploymentStep) -> bool {
        if step.approval_required {
            return true;
        }
        if !self.is_prod_environment() {
            return false;
        }
        Self::is_state_changing_action(&step.action)
    }

    fn is_prod_environment(&self) -> bool {
        matches!(self.environment(), Some(Environment::Prod))
    }

    fn environment(&self) -> Option<Environment> {
        match &self.mode {
            ExecutionMode::DryRun { .. } => None,
            ExecutionMode::SshReadonly { host, .. }
            | ExecutionMode::DockerReadonly { host, .. }
            | ExecutionMode::Compose { host, .. }
            | ExecutionMode::Nginx { host, .. }
            | ExecutionMode::Ssl { host, .. }
            | ExecutionMode::DbMigration { host, .. } => Some(host.environment),
        }
    }

    fn is_state_changing_action(action: &DeploymentAction) -> bool {
        matches!(
            action,
            DeploymentAction::ComposeUp { .. }
                | DeploymentAction::ComposeDown { .. }
                | DeploymentAction::NginxApply { .. }
                | DeploymentAction::NginxRollback { .. }
                | DeploymentAction::SslIssue { .. }
                | DeploymentAction::SslRenew { .. }
                | DeploymentAction::SslRollback { .. }
                | DeploymentAction::DbMigrate { .. }
        )
    }

    fn resolve_approval(
        &mut self,
        step_id: &str,
        decision: ApprovalDecision,
    ) -> Result<(), ExecutionError> {
        let status = self.status_for(step_id)?;
        if status != ExecutionStatus::WaitingApproval {
            return Err(ExecutionError::InvalidState {
                message: format!("step {step_id} is not waiting approval"),
            });
        }

        let current_id =
            self.order
                .get(self.index)
                .ok_or_else(|| ExecutionError::InvalidState {
                    message: "execution index is out of bounds".to_string(),
                })?;

        if current_id != step_id {
            return Err(ExecutionError::InvalidState {
                message: format!("step {step_id} is not next pending step"),
            });
        }

        let step = self
            .steps_by_id
            .get(step_id)
            .ok_or_else(|| ExecutionError::UnknownStep {
                step_id: step_id.to_string(),
            })?
            .clone();

        if !self.requires_approval(&step) {
            return Err(ExecutionError::InvalidState {
                message: format!("step {step_id} does not require approval"),
            });
        }

        let mut halt_on_failure = false;
        match decision {
            ApprovalDecision::Approved => {
                let outcome = self.execute_step(&step);
                if outcome.status == ExecutionStatus::Failed
                    && matches!(
                        step.action,
                        DeploymentAction::NginxApply { .. }
                            | DeploymentAction::NginxRollback { .. }
                            | DeploymentAction::SslIssue { .. }
                            | DeploymentAction::SslRenew { .. }
                            | DeploymentAction::SslRollback { .. }
                            | DeploymentAction::DbMigrate { .. }
                    )
                {
                    halt_on_failure = true;
                }
                self.record_execution(step_id, outcome);
            }
            ApprovalDecision::Rejected => {
                let record = ExecutionRecord {
                    step_id: step_id.to_string(),
                    status: ExecutionStatus::Failed,
                    started_at: None,
                    finished_at: None,
                    logs: {
                        let logs = vec![format!("Step {step_id} rejected")];
                        if let Some(ref stream) = self.log_stream {
                            stream.error(&step_id, &format!("Step {} rejected", step_id));
                        }
                        logs
                    },
                    error: Some("approval rejected".to_string()),
                };
                self.records.push(record);
                self.statuses
                    .insert(step_id.to_string(), ExecutionStatus::Failed);
                if matches!(
                    step.action,
                    DeploymentAction::NginxApply { .. }
                        | DeploymentAction::NginxRollback { .. }
                        | DeploymentAction::SslIssue { .. }
                        | DeploymentAction::SslRenew { .. }
                        | DeploymentAction::SslRollback { .. }
                        | DeploymentAction::DbMigrate { .. }
                ) {
                    halt_on_failure = true;
                }
            }
        }

        if halt_on_failure {
            self.halted = true;
        }

        self.index = self.index.saturating_add(1);
        Ok(())
    }

    fn record_execution(&mut self, step_id: &str, outcome: StepOutcome) {
        let mut logs = vec![format!("Executing step {step_id}")];
        logs.extend(outcome.logs);
        let record = ExecutionRecord {
            step_id: step_id.to_string(),
            status: outcome.status.clone(),
            started_at: None,
            finished_at: None,
            logs,
            error: outcome.error,
        };
        self.records.push(record.clone());
        self.statuses.insert(step_id.to_string(), outcome.status);

        if let Some(ref stream) = self.log_stream {
            for line in &record.logs {
                if line.starts_with("error:") {
                    stream.error(step_id, line);
                } else if line.starts_with("stderr:") {
                    stream.warning(step_id, line);
                } else {
                    stream.info(step_id, line);
                }
            }
            if let Some(err) = record.error.as_ref() {
                stream.error(step_id, err);
            }
        }

        // Call progress callback if set
        if let Some(ref callback) = self.progress_callback {
            callback(&record);
        }
    }

    fn dependencies_block_step(&self, step: &DeploymentStep) -> Result<bool, ExecutionError> {
        for dependency in &step.depends_on {
            let status = self.status_for(dependency)?;
            match status {
                ExecutionStatus::Succeeded | ExecutionStatus::Simulated => {}
                ExecutionStatus::Failed
                | ExecutionStatus::Skipped
                | ExecutionStatus::RolledBack => return Ok(true),
                ExecutionStatus::Pending
                | ExecutionStatus::Running
                | ExecutionStatus::WaitingApproval => {
                    return Err(ExecutionError::InvalidState {
                        message: format!("dependency {dependency} is not resolved"),
                    })
                }
            }
        }
        Ok(false)
    }

    fn status_for(&self, step_id: &str) -> Result<ExecutionStatus, ExecutionError> {
        if !self.steps_by_id.contains_key(step_id) {
            return Err(ExecutionError::UnknownStep {
                step_id: step_id.to_string(),
            });
        }
        Ok(self
            .statuses
            .get(step_id)
            .cloned()
            .unwrap_or(ExecutionStatus::Pending))
    }

    fn execute_step(&mut self, step: &DeploymentStep) -> StepOutcome {
        match &mut self.mode {
            ExecutionMode::DryRun { failure_steps } => {
                if failure_steps.contains(&step.id) {
                    StepOutcome {
                        status: ExecutionStatus::Failed,
                        logs: Vec::new(),
                        error: Some("simulated failure".to_string()),
                    }
                } else if matches!(
                    step.action,
                    DeploymentAction::DeployService | DeploymentAction::VerifyService
                ) {
                    // Should have been materialized in session start; never green-light symbols.
                    StepOutcome {
                        status: ExecutionStatus::Failed,
                        logs: Vec::new(),
                        error: Some(format!(
                            "dry-run refused symbolic action {:?}; materialize to RunSshCommand first",
                            step.action
                        )),
                    }
                } else {
                    let preview = match &step.action {
                        DeploymentAction::RunSshCommand { command } => {
                            format!("dry-run (no remote): would run `{command}`")
                        }
                        DeploymentAction::ComposeUp { project_dir } => {
                            format!("dry-run (no remote): would compose up in `{project_dir}`")
                        }
                        DeploymentAction::ComposeDown { project_dir } => {
                            format!("dry-run (no remote): would compose down in `{project_dir}`")
                        }
                        other => format!("dry-run (no remote): simulated {other:?}"),
                    };
                    StepOutcome {
                        status: ExecutionStatus::Simulated,
                        logs: vec![preview],
                        error: None,
                    }
                }
            }
            ExecutionMode::SshReadonly { host, client } => match &step.action {
                DeploymentAction::RunSshCommand { command } => {
                    if is_agus_stub_command(command) {
                        return invalid_action_outcome(
                            "refusing to execute Agus stub script (AGUS_SCRIPT_KIND=stub); replace with a real script",
                        );
                    }
                    let target = host_target(host);
                    run_ssh_command(client.as_ref(), &target, command)
                }
                other => unsupported_mode_action("ssh_readonly", other),
            },
            ExecutionMode::DockerReadonly { host, client } => match &step.action {
                DeploymentAction::DockerPs => {
                    let target = host_target(host);
                    run_ssh_command(client.as_ref(), &target, "docker ps")
                }
                DeploymentAction::DockerInspect {
                    target: inspect_target,
                } => {
                    let target = host_target(host);
                    match build_docker_inspect_command(inspect_target) {
                        Ok(command) => run_ssh_command(client.as_ref(), &target, &command),
                        Err(message) => invalid_action_outcome(&message),
                    }
                }
                DeploymentAction::DockerLogs { container, tail } => {
                    let target = host_target(host);
                    match build_docker_logs_command(container, *tail) {
                        Ok(command) => run_ssh_command(client.as_ref(), &target, &command),
                        Err(message) => invalid_action_outcome(&message),
                    }
                }
                other => unsupported_mode_action("docker_readonly", other),
            },
            ExecutionMode::Compose { host, client } => match &step.action {
                DeploymentAction::ComposeUp { project_dir } => {
                    let target = host_target(host);
                    match build_compose_command(project_dir, ComposeCommand::Up) {
                        Ok(command) => run_ssh_command_with_log(
                            client.as_ref(),
                            &target,
                            &command,
                            self.log_stream.as_ref(),
                            &step.id,
                        ),
                        Err(message) => invalid_action_outcome(&message),
                    }
                }
                DeploymentAction::ComposeDown { project_dir } => {
                    let target = host_target(host);
                    match build_compose_command(project_dir, ComposeCommand::Down) {
                        Ok(command) => run_ssh_command_with_log(
                            client.as_ref(),
                            &target,
                            &command,
                            self.log_stream.as_ref(),
                            &step.id,
                        ),
                        Err(message) => invalid_action_outcome(&message),
                    }
                }
                DeploymentAction::RunSshCommand { command } => {
                    if is_agus_stub_command(command) {
                        return invalid_action_outcome(
                            "refusing to execute Agus stub script (AGUS_SCRIPT_KIND=stub); replace with a real script",
                        );
                    }
                    let target = host_target(host);
                    run_ssh_command_with_log(
                        client.as_ref(),
                        &target,
                        command,
                        self.log_stream.as_ref(),
                        &step.id,
                    )
                }
                other => unsupported_mode_action("compose", other),
            },
            ExecutionMode::Nginx {
                host,
                client,
                configs,
            } => match &step.action {
                DeploymentAction::NginxApply { project } => {
                    let config = match configs.get(project) {
                        Some(config) => config,
                        None => {
                            return invalid_action_outcome("missing nginx project config");
                        }
                    };
                    match validate_nginx_project_config(config) {
                        Ok(()) => execute_nginx_apply(client.as_ref(), host, config),
                        Err(message) => invalid_action_outcome(&message),
                    }
                }
                DeploymentAction::NginxRollback { project } => {
                    match validate_nginx_project_name(project) {
                        Ok(()) => execute_nginx_rollback(client.as_ref(), host, project),
                        Err(message) => invalid_action_outcome(&message),
                    }
                }
                other => unsupported_mode_action("nginx", other),
            },
            ExecutionMode::Ssl {
                host,
                client,
                configs,
            } => match &step.action {
                DeploymentAction::SslIssue { project } => match configs.get(project) {
                    Some(config) => execute_ssl_issue(client.as_ref(), host, config),
                    None => invalid_action_outcome("missing ssl project config"),
                },
                DeploymentAction::SslRenew { project } => match configs.get(project) {
                    Some(config) => execute_ssl_renew(client.as_ref(), host, config),
                    None => invalid_action_outcome("missing ssl project config"),
                },
                DeploymentAction::SslRollback { project } => match configs.get(project) {
                    Some(config) => execute_ssl_rollback(client.as_ref(), host, config),
                    None => invalid_action_outcome("missing ssl project config"),
                },
                other => unsupported_mode_action("ssl", other),
            },
            ExecutionMode::DbMigration {
                host,
                client,
                migrations,
            } => match &step.action {
                DeploymentAction::DbMigrate { plan_id } => match migrations.get(plan_id) {
                    Some(migration) => execute_db_migration(client.as_ref(), host, migration),
                    None => invalid_action_outcome("missing db migration plan"),
                },
                other => unsupported_mode_action("db_migration", other),
            },
        }
    }
}

fn topo_sort_steps(
    steps_by_id: &HashMap<String, DeploymentStep>,
) -> Result<Vec<String>, ExecutionError> {
    let mut indegree: HashMap<String, usize> = HashMap::new();
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();

    for step_id in steps_by_id.keys() {
        indegree.insert(step_id.clone(), 0);
        adjacency.insert(step_id.clone(), Vec::new());
    }

    for (step_id, step) in steps_by_id {
        for dependency in &step.depends_on {
            let degree = indegree
                .get_mut(step_id)
                .ok_or_else(|| ExecutionError::InvalidPlan {
                    message: format!("missing step {step_id}"),
                })?;
            *degree += 1;
            let neighbors =
                adjacency
                    .get_mut(dependency)
                    .ok_or_else(|| ExecutionError::InvalidPlan {
                        message: format!("missing dependency step {dependency}"),
                    })?;
            neighbors.push(step_id.clone());
        }
    }

    let mut ready: BTreeSet<String> = indegree
        .iter()
        .filter_map(|(id, degree)| if *degree == 0 { Some(id.clone()) } else { None })
        .collect();

    let mut order = Vec::new();
    while let Some(id) = ready.iter().next().cloned() {
        ready.remove(&id);
        order.push(id.clone());
        if let Some(neighbors) = adjacency.get(&id) {
            for neighbor in neighbors {
                let degree =
                    indegree
                        .get_mut(neighbor)
                        .ok_or_else(|| ExecutionError::InvalidPlan {
                            message: format!("missing step {neighbor}"),
                        })?;
                *degree = degree.saturating_sub(1);
                if *degree == 0 {
                    ready.insert(neighbor.clone());
                }
            }
        }
    }

    if order.len() != indegree.len() {
        return Err(ExecutionError::InvalidPlan {
            message: "cycle detected in execution plan".to_string(),
        });
    }

    Ok(order)
}

fn host_target(host: &HostContext) -> SshTarget {
    SshTarget {
        host: host.host.clone(),
        user: host.user.clone(),
        port: host.port,
        identity_file: None,
        password: None,
    }
}

fn append_output_logs(logs: &mut Vec<String>, stdout: &str, stderr: &str) {
    let stdout_trimmed = stdout.trim_end();
    if !stdout_trimmed.is_empty() {
        logs.push(format!("stdout: {stdout_trimmed}"));
    }
    let stderr_trimmed = stderr.trim_end();
    if !stderr_trimmed.is_empty() {
        logs.push(format!("stderr: {stderr_trimmed}"));
    }
}

fn append_error_logs(logs: &mut Vec<String>, err: &SshError) {
    match err {
        SshError::Command { stderr, .. } => append_output_logs(logs, "", stderr),
        SshError::Connection { message } => {
            if !message.trim().is_empty() {
                logs.push(format!("error: {message}"));
            }
        }
        SshError::Timeout { message } => {
            if !message.trim().is_empty() {
                logs.push(format!("timeout: {message}"));
            }
        }
    }
}

fn run_ssh_command(client: &dyn SshClient, target: &SshTarget, command: &str) -> StepOutcome {
    match client.execute(target, command) {
        Ok(result) => {
            let mut logs = Vec::new();
            append_output_logs(&mut logs, &result.stdout, &result.stderr);
            StepOutcome {
                status: ExecutionStatus::Succeeded,
                logs,
                error: None,
            }
        }
        Err(err) => {
            let mut logs = Vec::new();
            append_error_logs(&mut logs, &err);
            StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            }
        }
    }
}

fn run_ssh_command_with_log(
    client: &dyn SshClient,
    target: &SshTarget,
    command: &str,
    stream: Option<&LogStreamSender>,
    step_id: &str,
) -> StepOutcome {
    let mut logs = vec![format!("command: {command}")];
    if let Some(sender) = stream {
        sender.info(step_id, &format!("command: {command}"));
    }
    let mut on_output = |kind: SshOutputStream, line: &str| {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            return;
        }
        match kind {
            SshOutputStream::Stdout => {
                logs.push(trimmed.to_string());
                if let Some(sender) = stream {
                    sender.info(step_id, trimmed);
                }
            }
            SshOutputStream::Stderr => {
                logs.push(format!("stderr: {trimmed}"));
                if let Some(sender) = stream {
                    sender.warning(step_id, trimmed);
                }
            }
        }
    };

    match client.execute_streaming(target, command, &mut on_output) {
        Ok(_) => StepOutcome {
            status: ExecutionStatus::Succeeded,
            logs,
            error: None,
        },
        Err(err) => {
            append_error_logs(&mut logs, &err);
            if let Some(sender) = stream {
                sender.error(step_id, &err.to_string());
            }
            StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            }
        }
    }
}

fn invalid_action_outcome(message: &str) -> StepOutcome {
    StepOutcome {
        status: ExecutionStatus::Failed,
        logs: vec![format!("error: {message}")],
        error: Some(message.to_string()),
    }
}

/// Agus Forge 占位脚本标记（与 agus-forge-handoff::is_agus_stub_script 对齐）
fn is_agus_stub_command(content: &str) -> bool {
    let lower = content.to_ascii_lowercase();
    content.contains("AGUS_SCRIPT_KIND=stub")
        || content.contains("# Agus stub")
        || lower.contains("agus stub from forge")
        || lower.contains("[build] stub")
        || lower.contains("[deploy.dev] stub")
        || lower.contains("[deploy.prod] stub")
}

fn unsupported_mode_action(mode: &str, action: &DeploymentAction) -> StepOutcome {
    invalid_action_outcome(&format!(
        "action {action:?} is not supported in {mode} mode (refusing silent success)"
    ))
}

fn build_docker_inspect_command(target: &str) -> Result<String, String> {
    validate_docker_identifier(target)?;
    Ok(format!("docker inspect -- {target}"))
}

fn build_docker_logs_command(container: &str, tail: u32) -> Result<String, String> {
    validate_docker_identifier(container)?;
    Ok(format!("docker logs --tail {tail} -- {container}"))
}

fn validate_docker_identifier(value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("docker target is empty".to_string());
    }
    if value.starts_with('-') {
        return Err("docker target cannot start with '-'".to_string());
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | ':' | '/' | '@'))
    {
        return Err("docker target contains invalid characters".to_string());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum ComposeCommand {
    Up,
    Down,
}

fn build_compose_command(project_dir: &str, command: ComposeCommand) -> Result<String, String> {
    validate_compose_dir(project_dir)?;
    let compose = match command {
        ComposeCommand::Up => "docker compose up -d",
        ComposeCommand::Down => "docker compose down",
    };
    Ok(format!("cd -- {project_dir} && {compose}"))
}

fn validate_compose_dir(project_dir: &str) -> Result<(), String> {
    let trimmed = project_dir.trim();
    if trimmed.is_empty() {
        return Err("compose project_dir is empty".to_string());
    }

    let without_trailing = trimmed.trim_end_matches('/');
    if without_trailing.is_empty() || trimmed == "/" {
        return Err("compose project_dir cannot be root".to_string());
    }

    if trimmed == "." || trimmed == ".." {
        return Err("compose project_dir cannot be '.' or '..'".to_string());
    }

    if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("compose project_dir contains invalid characters".to_string());
    }

    if trimmed.split('/').any(|segment| segment == "..") {
        return Err("compose project_dir cannot contain '..'".to_string());
    }

    if trimmed.chars().any(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                ';' | '&' | '|' | '`' | '$' | '>' | '<' | '"' | '\'' | '\\'
            )
    }) {
        return Err("compose project_dir contains invalid characters".to_string());
    }

    Ok(())
}

fn execute_nginx_apply(
    client: &dyn SshClient,
    host: &HostContext,
    config: &NginxProjectConfig,
) -> StepOutcome {
    let target = host_target(host);
    let config_text = render_nginx_config(config);
    let mut logs = Vec::new();

    if let Some(domain) = &config.ssl_domain {
        let check_command = build_ssl_cert_check_command(domain);
        logs.push(format!("command: {check_command}"));
        if let Err(err) = client.execute(&target, &check_command) {
            append_error_logs(&mut logs, &err);
            return StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            };
        }
    }

    let mkdir_command = "mkdir -p /etc/nginx/agus/projects";
    logs.push(format!("command: {mkdir_command}"));
    if let Err(err) = client.execute(&target, mkdir_command) {
        append_error_logs(&mut logs, &err);
        return StepOutcome {
            status: ExecutionStatus::Failed,
            logs,
            error: Some(err.to_string()),
        };
    }

    let config_path = nginx_config_path(&config.project);
    let write_command = build_nginx_write_command(&config_path, &config_text);
    logs.push(format!("command: write nginx config {config_path}"));
    if let Err(err) = client.execute(&target, &write_command) {
        append_error_logs(&mut logs, &err);
        return StepOutcome {
            status: ExecutionStatus::Failed,
            logs,
            error: Some(err.to_string()),
        };
    }

    let test_command = "nginx -t";
    logs.push(format!("command: {test_command}"));
    match client.execute(&target, test_command) {
        Ok(result) => {
            append_output_logs(&mut logs, &result.stdout, &result.stderr);
        }
        Err(err) => {
            append_error_logs(&mut logs, &err);
            return StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            };
        }
    }

    let reload_command = "nginx reload";
    logs.push(format!("command: {reload_command}"));
    match client.execute(&target, reload_command) {
        Ok(result) => {
            append_output_logs(&mut logs, &result.stdout, &result.stderr);
            StepOutcome {
                status: ExecutionStatus::Succeeded,
                logs,
                error: None,
            }
        }
        Err(err) => {
            append_error_logs(&mut logs, &err);
            StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            }
        }
    }
}

fn execute_nginx_rollback(
    client: &dyn SshClient,
    host: &HostContext,
    project: &str,
) -> StepOutcome {
    let target = host_target(host);
    let mut logs = Vec::new();
    let config_path = nginx_config_path(project);
    let remove_command = format!("rm -f -- {config_path}");
    logs.push(format!("command: {remove_command}"));
    if let Err(err) = client.execute(&target, &remove_command) {
        append_error_logs(&mut logs, &err);
        return StepOutcome {
            status: ExecutionStatus::Failed,
            logs,
            error: Some(err.to_string()),
        };
    }

    let reload_command = "nginx reload";
    logs.push(format!("command: {reload_command}"));
    match client.execute(&target, reload_command) {
        Ok(result) => {
            append_output_logs(&mut logs, &result.stdout, &result.stderr);
            StepOutcome {
                status: ExecutionStatus::Succeeded,
                logs,
                error: None,
            }
        }
        Err(err) => {
            append_error_logs(&mut logs, &err);
            StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            }
        }
    }
}

fn execute_ssl_issue(
    client: &dyn SshClient,
    host: &HostContext,
    config: &SslProjectConfig,
) -> StepOutcome {
    if let Err(message) = validate_ssl_project_identity(config) {
        return invalid_action_outcome(&message);
    }
    if let Err(message) = validate_webroot(&config.webroot) {
        return invalid_action_outcome(&message);
    }
    let target = host_target(host);
    let mut logs = Vec::new();

    let check_command = match build_webroot_check_command(&config.webroot) {
        Ok(command) => command,
        Err(message) => return invalid_action_outcome(&message),
    };
    logs.push(format!("command: {check_command}"));
    if let Err(err) = client.execute(&target, &check_command) {
        append_error_logs(&mut logs, &err);
        return StepOutcome {
            status: ExecutionStatus::Failed,
            logs,
            error: Some(err.to_string()),
        };
    }

    let issue_command = match build_certbot_issue_command(config) {
        Ok(command) => command,
        Err(message) => return invalid_action_outcome(&message),
    };
    logs.push(format!("command: {issue_command}"));
    match client.execute(&target, &issue_command) {
        Ok(result) => {
            append_output_logs(&mut logs, &result.stdout, &result.stderr);
            StepOutcome {
                status: ExecutionStatus::Succeeded,
                logs,
                error: None,
            }
        }
        Err(err) => {
            append_error_logs(&mut logs, &err);
            StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            }
        }
    }
}

fn execute_ssl_renew(
    client: &dyn SshClient,
    host: &HostContext,
    config: &SslProjectConfig,
) -> StepOutcome {
    if let Err(message) = validate_ssl_project_identity(config) {
        return invalid_action_outcome(&message);
    }
    if let Err(message) = validate_webroot(&config.webroot) {
        return invalid_action_outcome(&message);
    }
    let target = host_target(host);
    let mut logs = Vec::new();

    let check_command = match build_webroot_check_command(&config.webroot) {
        Ok(command) => command,
        Err(message) => return invalid_action_outcome(&message),
    };
    logs.push(format!("command: {check_command}"));
    if let Err(err) = client.execute(&target, &check_command) {
        append_error_logs(&mut logs, &err);
        return StepOutcome {
            status: ExecutionStatus::Failed,
            logs,
            error: Some(err.to_string()),
        };
    }

    let renew_command = match build_certbot_renew_command(config) {
        Ok(command) => command,
        Err(message) => return invalid_action_outcome(&message),
    };
    logs.push(format!("command: {renew_command}"));
    match client.execute(&target, &renew_command) {
        Ok(result) => {
            append_output_logs(&mut logs, &result.stdout, &result.stderr);
            StepOutcome {
                status: ExecutionStatus::Succeeded,
                logs,
                error: None,
            }
        }
        Err(err) => {
            append_error_logs(&mut logs, &err);
            StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            }
        }
    }
}

fn execute_ssl_rollback(
    client: &dyn SshClient,
    host: &HostContext,
    config: &SslProjectConfig,
) -> StepOutcome {
    if let Err(message) = validate_ssl_project_identity(config) {
        return invalid_action_outcome(&message);
    }
    let target = host_target(host);
    let mut logs = Vec::new();

    let rollback_command = match build_ssl_rollback_command(&config.project) {
        Ok(command) => command,
        Err(message) => return invalid_action_outcome(&message),
    };
    logs.push(format!("command: {rollback_command}"));
    if let Err(err) = client.execute(&target, &rollback_command) {
        append_error_logs(&mut logs, &err);
        return StepOutcome {
            status: ExecutionStatus::Failed,
            logs,
            error: Some(err.to_string()),
        };
    }

    let reload_command = "nginx reload";
    logs.push(format!("command: {reload_command}"));
    match client.execute(&target, reload_command) {
        Ok(result) => {
            append_output_logs(&mut logs, &result.stdout, &result.stderr);
            StepOutcome {
                status: ExecutionStatus::Succeeded,
                logs,
                error: None,
            }
        }
        Err(err) => {
            append_error_logs(&mut logs, &err);
            StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            }
        }
    }
}

fn execute_db_migration(
    client: &dyn SshClient,
    host: &HostContext,
    migration: &DbMigrationSpec,
) -> StepOutcome {
    if let Err(message) = validate_db_migration_spec(migration) {
        return invalid_action_outcome(&message);
    }

    let target = host_target(host);
    let mut logs = Vec::new();
    let sql_path = db_migration_sql_path(&migration.plan_id);
    let write_command = build_sql_write_command(&sql_path, &migration.sql);
    logs.push(format!("command: write migration sql {sql_path}"));
    if let Err(err) = client.execute(&target, &write_command) {
        append_error_logs(&mut logs, &err);
        return StepOutcome {
            status: ExecutionStatus::Failed,
            logs,
            error: Some(err.to_string()),
        };
    }

    let exec_command = match migration.connection.db_type {
        DatabaseType::Postgres => {
            match build_postgres_migration_command(&migration.connection, &sql_path) {
                Ok(command) => command,
                Err(message) => return invalid_action_outcome(&message),
            }
        }
        DatabaseType::MySQL => {
            match build_mysql_migration_command(&migration.connection, &sql_path) {
                Ok(command) => command,
                Err(message) => return invalid_action_outcome(&message),
            }
        }
    };

    logs.push(format!("command: {exec_command}"));
    match client.execute(&target, &exec_command) {
        Ok(result) => {
            append_output_logs(&mut logs, &result.stdout, &result.stderr);
            StepOutcome {
                status: ExecutionStatus::Succeeded,
                logs,
                error: None,
            }
        }
        Err(err) => {
            append_error_logs(&mut logs, &err);
            StepOutcome {
                status: ExecutionStatus::Failed,
                logs,
                error: Some(err.to_string()),
            }
        }
    }
}

fn nginx_config_path(project: &str) -> String {
    format!("/etc/nginx/agus/projects/{project}.conf")
}

fn build_nginx_write_command(path: &str, contents: &str) -> String {
    let delimiter = "AGUS_NGINX_EOF";
    format!("cat <<'{delimiter}' > {path}\n{contents}\n{delimiter}")
}

fn build_sql_write_command(path: &str, contents: &str) -> String {
    let delimiter = "AGUS_SQL_EOF";
    format!("cat <<'{delimiter}' > {path}\n{contents}\n{delimiter}")
}

fn db_migration_sql_path(plan_id: &str) -> String {
    format!("/tmp/agus_migration_{plan_id}.sql")
}

fn validate_db_migration_spec(spec: &DbMigrationSpec) -> Result<(), String> {
    validate_migration_plan_id(&spec.plan_id)?;
    validate_db_connection_config(&spec.connection)?;
    if spec.sql.trim().is_empty() {
        return Err("migration sql is empty".to_string());
    }
    Ok(())
}

fn validate_migration_plan_id(plan_id: &str) -> Result<(), String> {
    let trimmed = plan_id.trim();
    if trimmed.is_empty() {
        return Err("migration plan id is empty".to_string());
    }
    if trimmed != plan_id {
        return Err("migration plan id contains whitespace".to_string());
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err("migration plan id contains invalid characters".to_string());
    }
    Ok(())
}

fn build_postgres_migration_command(
    config: &DbConnectionConfig,
    sql_path: &str,
) -> Result<String, String> {
    validate_db_connection_config(config)?;
    Ok(format!(
        "psql -h {} -p {} -U {} -d {} -v ON_ERROR_STOP=1 -f {}",
        config.host, config.port, config.user, config.database, sql_path
    ))
}

fn build_mysql_migration_command(
    config: &DbConnectionConfig,
    sql_path: &str,
) -> Result<String, String> {
    validate_db_connection_config(config)?;
    Ok(format!(
        "mysql -h {} -P {} -u {} {} < {}",
        config.host, config.port, config.user, config.database, sql_path
    ))
}

fn validate_db_connection_config(config: &DbConnectionConfig) -> Result<(), String> {
    if config.port == 0 {
        return Err("db port is invalid".to_string());
    }
    validate_db_host(&config.host)?;
    validate_db_identifier("db user", &config.user)?;
    validate_db_identifier("database", &config.database)?;
    Ok(())
}

fn validate_db_host(host: &str) -> Result<(), String> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err("db host is empty".to_string());
    }
    if trimmed
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_')))
    {
        return Err("db host contains invalid characters".to_string());
    }
    Ok(())
}

fn validate_db_identifier(label: &str, value: &str) -> Result<(), String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("{label} is empty"));
    }
    if trimmed
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')))
    {
        return Err(format!("{label} contains invalid characters"));
    }
    Ok(())
}

fn ssl_certificate_paths(domain: &str) -> (String, String) {
    (
        format!("/etc/letsencrypt/live/{domain}/fullchain.pem"),
        format!("/etc/letsencrypt/live/{domain}/privkey.pem"),
    )
}

fn build_ssl_cert_check_command(domain: &str) -> String {
    let (fullchain, privkey) = ssl_certificate_paths(domain);
    format!("test -f -- {fullchain} && test -f -- {privkey}")
}

fn validate_nginx_project_config(config: &NginxProjectConfig) -> Result<(), String> {
    validate_nginx_project_name(&config.project)?;
    validate_server_name(&config.server_name)?;
    if let Some(domain) = &config.ssl_domain {
        validate_domain(domain)?;
    }
    for location in &config.locations {
        validate_location(location)?;
    }
    Ok(())
}

fn validate_nginx_project_name(name: &str) -> Result<(), String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("nginx project name is empty".to_string());
    }
    if trimmed.starts_with('.') || trimmed.contains('/') {
        return Err("nginx project name contains invalid characters".to_string());
    }
    if !trimmed
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
    {
        return Err("nginx project name contains invalid characters".to_string());
    }
    Ok(())
}

fn validate_server_name(server_name: &str) -> Result<(), String> {
    let trimmed = server_name.trim();
    if trimmed.is_empty() {
        return Err("nginx server_name is empty".to_string());
    }
    if trimmed.chars().any(|ch| ch.is_whitespace() || ch == ';') {
        return Err("nginx server_name contains invalid characters".to_string());
    }
    Ok(())
}

fn validate_location(location: &agus_core_domain::NginxLocation) -> Result<(), String> {
    let path = location.path.trim();
    if !path.starts_with('/') {
        return Err("nginx location path must start with '/'".to_string());
    }
    if path.chars().any(|ch| ch.is_whitespace() || ch == ';') {
        return Err("nginx location path contains invalid characters".to_string());
    }
    validate_proxy_pass(&location.proxy_pass)
}

fn validate_proxy_pass(proxy_pass: &str) -> Result<(), String> {
    let trimmed = proxy_pass.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return Err("nginx proxy_pass must start with http:// or https://".to_string());
    }
    if trimmed.chars().any(|ch| ch.is_whitespace()) {
        return Err("nginx proxy_pass contains whitespace".to_string());
    }
    let scheme_len = if trimmed.starts_with("http://") { 7 } else { 8 };
    let rest = &trimmed[scheme_len..];
    if rest.is_empty() {
        return Err("nginx proxy_pass missing host".to_string());
    }
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..host_end];
    if host.is_empty() || host.starts_with(':') {
        return Err("nginx proxy_pass missing host".to_string());
    }
    if host
        .chars()
        .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | ':' | '[' | ']')))
    {
        return Err("nginx proxy_pass contains invalid host characters".to_string());
    }
    Ok(())
}

fn validate_domain(domain: &str) -> Result<(), String> {
    let trimmed = domain.trim();
    if trimmed.is_empty() {
        return Err("ssl domain is empty".to_string());
    }
    if trimmed != domain {
        return Err("ssl domain contains whitespace".to_string());
    }
    if trimmed.len() > 253 {
        return Err("ssl domain is too long".to_string());
    }
    if trimmed.starts_with('.') || trimmed.ends_with('.') {
        return Err("ssl domain cannot start or end with '.'".to_string());
    }
    if trimmed.contains('*') {
        return Err("ssl domain cannot contain '*'".to_string());
    }
    if !trimmed.contains('.') {
        return Err("ssl domain must contain at least one '.'".to_string());
    }

    for label in trimmed.split('.') {
        if label.is_empty() {
            return Err("ssl domain contains empty label".to_string());
        }
        if label.len() > 63 {
            return Err("ssl domain label is too long".to_string());
        }
        if label.starts_with('-') || label.ends_with('-') {
            return Err("ssl domain label cannot start or end with '-'".to_string());
        }
        if !label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            return Err("ssl domain label contains invalid characters".to_string());
        }
    }

    Ok(())
}

fn validate_email(email: &str) -> Result<(), String> {
    let trimmed = email.trim();
    if trimmed.is_empty() {
        return Err("ssl email is empty".to_string());
    }
    if trimmed != email {
        return Err("ssl email contains whitespace".to_string());
    }

    let mut parts = trimmed.split('@');
    let local = parts.next().unwrap_or("");
    let domain = parts.next().unwrap_or("");
    if parts.next().is_some() {
        return Err("ssl email must contain a single '@'".to_string());
    }
    if local.is_empty() || domain.is_empty() {
        return Err("ssl email must include local and domain".to_string());
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return Err("ssl email local part is invalid".to_string());
    }
    if !local
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '%' | '+' | '-'))
    {
        return Err("ssl email local part contains invalid characters".to_string());
    }

    validate_domain(domain)
}

fn validate_webroot(webroot: &str) -> Result<(), String> {
    let trimmed = webroot.trim();
    if trimmed.is_empty() {
        return Err("ssl webroot is empty".to_string());
    }
    if trimmed != webroot {
        return Err("ssl webroot contains whitespace".to_string());
    }
    if !trimmed.starts_with('/') {
        return Err("ssl webroot must be an absolute path".to_string());
    }
    if trimmed == "/" {
        return Err("ssl webroot cannot be root".to_string());
    }
    if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("ssl webroot contains invalid characters".to_string());
    }
    if trimmed.split('/').any(|segment| segment == "..") {
        return Err("ssl webroot cannot contain '..'".to_string());
    }
    if trimmed.chars().any(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                ';' | '&' | '|' | '`' | '$' | '>' | '<' | '"' | '\'' | '\\'
            )
    }) {
        return Err("ssl webroot contains invalid characters".to_string());
    }
    Ok(())
}

fn validate_ssl_project_identity(config: &SslProjectConfig) -> Result<(), String> {
    validate_nginx_project_name(&config.project)?;
    validate_domain(&config.domain)?;
    validate_email(&config.email)?;
    Ok(())
}

fn build_webroot_check_command(webroot: &str) -> Result<String, String> {
    validate_webroot(webroot)?;
    Ok(format!("test -d -- {webroot} && test -w -- {webroot}"))
}

fn build_certbot_issue_command(config: &SslProjectConfig) -> Result<String, String> {
    validate_ssl_project_identity(config)?;
    validate_webroot(&config.webroot)?;
    Ok(format!(
        "certbot certonly --webroot -w {} -d {} --email {} --agree-tos --non-interactive --keep-until-expiring",
        config.webroot, config.domain, config.email
    ))
}

fn build_certbot_renew_command(config: &SslProjectConfig) -> Result<String, String> {
    validate_ssl_project_identity(config)?;
    validate_webroot(&config.webroot)?;
    Ok(format!(
        "certbot renew --cert-name {} --non-interactive",
        config.domain
    ))
}

fn build_ssl_rollback_command(project: &str) -> Result<String, String> {
    validate_nginx_project_name(project)?;
    let config_path = nginx_config_path(project);
    Ok(format!(
        "sed -i -e '/listen 443 ssl;/d' -e '/ssl_certificate /d' -e '/ssl_certificate_key /d' -- {config_path}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agus_core_domain::{
        DatabaseType, DbConnectionConfig, DeploymentAction, DeploymentPlan, DeploymentStep,
        Environment, HostContext, NginxLocation, NginxProjectConfig, SslProjectConfig,
    };
    use agus_ssh::{SshClient, SshCommandResult, SshError, SshTarget};
    use std::collections::HashMap;

    fn step(id: &str, depends_on: &[&str]) -> DeploymentStep {
        DeploymentStep {
            id: id.to_string(),
            service_name: "svc".to_string(),
            action: DeploymentAction::DeployService,
            depends_on: depends_on.iter().map(|value| value.to_string()).collect(),
            approval_required: false,
            memo: None,
        }
    }

    fn ssh_step(id: &str, command: &str, depends_on: &[&str]) -> DeploymentStep {
        DeploymentStep {
            id: id.to_string(),
            service_name: "svc".to_string(),
            action: DeploymentAction::RunSshCommand {
                command: command.to_string(),
            },
            depends_on: depends_on.iter().map(|value| value.to_string()).collect(),
            approval_required: false,
            memo: None,
        }
    }

    fn docker_step(id: &str, action: DeploymentAction, depends_on: &[&str]) -> DeploymentStep {
        DeploymentStep {
            id: id.to_string(),
            service_name: "svc".to_string(),
            action,
            depends_on: depends_on.iter().map(|value| value.to_string()).collect(),
            approval_required: false,
            memo: None,
        }
    }

    fn approval_step(id: &str, depends_on: &[&str]) -> DeploymentStep {
        DeploymentStep {
            id: id.to_string(),
            service_name: "svc".to_string(),
            action: DeploymentAction::DeployService,
            depends_on: depends_on.iter().map(|value| value.to_string()).collect(),
            approval_required: true,
            memo: None,
        }
    }

    fn ssl_config(project: &str, domain: &str, email: &str, webroot: &str) -> SslProjectConfig {
        SslProjectConfig {
            project: project.to_string(),
            domain: domain.to_string(),
            email: email.to_string(),
            webroot: webroot.to_string(),
        }
    }

    fn db_migration_spec(plan_id: &str, sql: &str) -> DbMigrationSpec {
        DbMigrationSpec {
            plan_id: plan_id.to_string(),
            connection: DbConnectionConfig {
                db_type: DatabaseType::Postgres,
                host: "db.internal".to_string(),
                port: 5432,
                user: "dbuser".to_string(),
                database: "app".to_string(),
            },
            sql: sql.to_string(),
        }
    }

    #[derive(Debug, Clone)]
    struct MockSshClient {
        responses: HashMap<String, Result<SshCommandResult, SshError>>,
    }

    impl SshClient for MockSshClient {
        fn execute(
            &self,
            _target: &SshTarget,
            command: &str,
        ) -> Result<SshCommandResult, SshError> {
            self.responses.get(command).cloned().unwrap_or_else(|| {
                Err(SshError::Command {
                    exit_code: 1,
                    stderr: "unexpected command".to_string(),
                })
            })
        }
    }

    #[derive(Debug, Clone)]
    struct RecordingSshClient {
        responses: HashMap<String, Result<SshCommandResult, SshError>>,
        commands: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl RecordingSshClient {
        fn new(
            responses: HashMap<String, Result<SshCommandResult, SshError>>,
            commands: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        ) -> Self {
            Self {
                responses,
                commands,
            }
        }
    }

    impl SshClient for RecordingSshClient {
        fn execute(
            &self,
            _target: &SshTarget,
            command: &str,
        ) -> Result<SshCommandResult, SshError> {
            if let Ok(mut guard) = self.commands.lock() {
                guard.push(command.to_string());
            }
            self.responses.get(command).cloned().unwrap_or_else(|| {
                Ok(SshCommandResult {
                    stdout: "".to_string(),
                    stderr: "".to_string(),
                    exit_code: 0,
                })
            })
        }
    }

    #[test]
    fn executes_in_dependency_order() {
        let plan = DeploymentPlan {
            steps: vec![
                step("deploy:db", &[]),
                step("deploy:api", &["deploy:db"]),
                step("verify:db", &["deploy:db"]),
                step("verify:api", &["deploy:api"]),
            ],
        };

        let executor = Executor::new();
        let mut session = executor.start_dry_run(plan).expect("start session");
        let records = session.run().expect("execute plan");

        let index_of = |id: &str| {
            records
                .iter()
                .position(|record| {
                    record.step_id == id && record.status != ExecutionStatus::WaitingApproval
                })
                .expect("record present")
        };

        assert!(index_of("deploy:db") < index_of("deploy:api"));
        assert!(index_of("deploy:db") < index_of("verify:db"));
        assert!(index_of("deploy:api") < index_of("verify:api"));
    }

    #[test]
    fn blocks_dependents_on_failure() {
        let plan = DeploymentPlan {
            steps: vec![
                step("deploy:db", &[]),
                step("deploy:api", &["deploy:db"]),
                step("deploy:cache", &[]),
            ],
        };

        let executor = Executor::with_failure_steps(vec!["deploy:db".to_string()]);
        let mut session = executor.start_dry_run(plan).expect("start session");
        let records = session.run().expect("execute plan");

        let status_for = |id: &str| {
            records
                .iter()
                .find(|record| {
                    record.step_id == id && record.status != ExecutionStatus::WaitingApproval
                })
                .expect("record present")
                .status
                .clone()
        };

        assert!(matches!(status_for("deploy:db"), ExecutionStatus::Failed));
        assert!(matches!(status_for("deploy:api"), ExecutionStatus::Skipped));
        assert!(matches!(
            status_for("deploy:cache"),
            ExecutionStatus::Simulated
        ));
    }

    #[test]
    fn records_status_and_logs() {
        let plan = DeploymentPlan {
            steps: vec![step("deploy:api", &[])],
        };

        let executor = Executor::new();
        let mut session = executor.start_dry_run(plan).expect("start session");
        let records = session.run().expect("execute plan");
        let record = records.first().expect("record");

        assert!(matches!(record.status, ExecutionStatus::Simulated));
        assert!(
            record
                .logs
                .iter()
                .any(|line| line.contains("Executing step deploy:api"))
        );
        assert!(
            record
                .logs
                .iter()
                .any(|line| line.contains("dry-run (no remote): would run"))
        );
        assert!(record.error.is_none());
    }

    #[test]
    fn approval_required_pauses_execution() {
        let plan = DeploymentPlan {
            steps: vec![
                approval_step("deploy:db", &[]),
                step("deploy:api", &["deploy:db"]),
            ],
        };

        let executor = Executor::new();
        let mut session = executor.start_dry_run(plan).expect("start session");
        let records = session.run().expect("execute plan");

        assert!(records.iter().any(|record| {
            record.step_id == "deploy:db"
                && matches!(record.status, ExecutionStatus::WaitingApproval)
        }));
        assert!(!records.iter().any(|record| record.step_id == "deploy:api"));

        let records = session.approve_step("deploy:db").expect("approve step");

        assert!(records.iter().any(|record| {
            record.step_id == "deploy:db" && matches!(record.status, ExecutionStatus::Simulated)
        }));
        assert!(records.iter().any(|record| {
            record.step_id == "deploy:api" && matches!(record.status, ExecutionStatus::Simulated)
        }));
    }

    #[test]
    fn prod_requires_approval_for_state_change() {
        let plan = DeploymentPlan {
            steps: vec![DeploymentStep {
                id: "compose:up".to_string(),
                service_name: "app".to_string(),
                action: DeploymentAction::ComposeUp {
                    project_dir: "/srv/app".to_string(),
                },
                depends_on: Vec::new(),
                approval_required: false,
                memo: None,
            }],
        };

        let mut responses = HashMap::new();
        responses.insert(
            "cd -- /srv/app && docker compose up -d".to_string(),
            Ok(SshCommandResult {
                stdout: "ok".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );

        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Prod,
        };
        let executor = Executor::new();
        let mut session = executor
            .start_compose(plan, host, client)
            .expect("start session");

        let records = session.run().expect("execute plan");
        assert!(records.iter().any(|record| {
            record.step_id == "compose:up"
                && matches!(record.status, ExecutionStatus::WaitingApproval)
        }));

        let records = session.approve_step("compose:up").expect("approve step");
        assert!(records.iter().any(|record| {
            record.step_id == "compose:up" && matches!(record.status, ExecutionStatus::Succeeded)
        }));
    }

    #[test]
    fn rejection_blocks_downstream_steps() {
        let plan = DeploymentPlan {
            steps: vec![
                approval_step("deploy:db", &[]),
                step("deploy:api", &["deploy:db"]),
            ],
        };

        let executor = Executor::new();
        let mut session = executor.start_dry_run(plan).expect("start session");
        session.run().expect("execute plan");

        let records = session.reject_step("deploy:db").expect("reject step");

        assert!(records.iter().any(|record| {
            record.step_id == "deploy:db" && matches!(record.status, ExecutionStatus::Failed)
        }));
        assert!(records.iter().any(|record| {
            record.step_id == "deploy:api" && matches!(record.status, ExecutionStatus::Skipped)
        }));
    }

    #[test]
    fn ssl_issue_requires_approval() {
        let plan = DeploymentPlan {
            steps: vec![DeploymentStep {
                id: "ssl:issue".to_string(),
                service_name: "ssl".to_string(),
                action: DeploymentAction::SslIssue {
                    project: "demo".to_string(),
                },
                depends_on: Vec::new(),
                approval_required: false,
                memo: None,
            }],
        };
        let client = MockSshClient {
            responses: HashMap::new(),
        };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };
        let executor = Executor::new();
        let mut session = executor
            .start_ssl(
                plan,
                host,
                client,
                vec![ssl_config(
                    "demo",
                    "example.com",
                    "ops@example.com",
                    "/var/www/html",
                )],
            )
            .expect("start session");

        let result = session.run();
        assert!(matches!(result, Err(ExecutionError::InvalidPlan { .. })));
    }

    #[test]
    fn db_migrate_requires_approval() {
        let plan = DeploymentPlan {
            steps: vec![DeploymentStep {
                id: "db:migrate".to_string(),
                service_name: "db".to_string(),
                action: DeploymentAction::DbMigrate {
                    plan_id: "plan-1".to_string(),
                },
                depends_on: Vec::new(),
                approval_required: false,
                memo: None,
            }],
        };
        let client = MockSshClient {
            responses: HashMap::new(),
        };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };
        let executor = Executor::new();
        let mut session = executor
            .start_db_migration(
                plan,
                host,
                client,
                vec![db_migration_spec(
                    "plan-1",
                    "ALTER TABLE users ADD COLUMN email text;",
                )],
            )
            .expect("start session");

        let result = session.run();
        assert!(matches!(result, Err(ExecutionError::InvalidPlan { .. })));
    }

    #[test]
    fn ssl_validation_rejects_bad_domain() {
        let config = ssl_config("demo", "bad_domain", "ops@example.com", "/var/www/html");
        let result = build_certbot_issue_command(&config);
        assert!(result.is_err());
    }

    #[test]
    fn ssl_validation_rejects_bad_email() {
        let config = ssl_config("demo", "example.com", "invalid-email", "/var/www/html");
        let result = build_certbot_issue_command(&config);
        assert!(result.is_err());
    }

    #[test]
    fn builds_certbot_issue_command() {
        let config = ssl_config("demo", "example.com", "ops@example.com", "/var/www/html");
        let command = build_certbot_issue_command(&config).expect("command");
        assert_eq!(
            command,
            "certbot certonly --webroot -w /var/www/html -d example.com --email ops@example.com --agree-tos --non-interactive --keep-until-expiring"
        );
    }

    #[test]
    fn executes_ssh_command_and_captures_output() {
        let plan = DeploymentPlan {
            steps: vec![ssh_step("ssh:uname", "uname -a", &[])],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "uname -a".to_string(),
            Ok(SshCommandResult {
                stdout: "Linux test-host 6.1.0".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_ssh_readonly(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute plan");
        let record = records.first().expect("record");

        assert!(matches!(record.status, ExecutionStatus::Succeeded));
        assert!(record
            .logs
            .iter()
            .any(|line| line.contains("stdout: Linux test-host 6.1.0")));
    }

    #[test]
    fn ssh_failure_blocks_downstream_steps() {
        let plan = DeploymentPlan {
            steps: vec![
                ssh_step("ssh:hostname", "hostname", &[]),
                step("deploy:api", &["ssh:hostname"]),
            ],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "hostname".to_string(),
            Err(SshError::Command {
                exit_code: 2,
                stderr: "permission denied".to_string(),
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_ssh_readonly(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute plan");

        let status_for = |id: &str| {
            records
                .iter()
                .find(|record| {
                    record.step_id == id && record.status != ExecutionStatus::WaitingApproval
                })
                .expect("record present")
                .status
                .clone()
        };

        assert!(matches!(
            status_for("ssh:hostname"),
            ExecutionStatus::Failed
        ));
        assert!(matches!(status_for("deploy:api"), ExecutionStatus::Skipped));
    }

    #[test]
    fn executes_docker_ps_command() {
        let plan = DeploymentPlan {
            steps: vec![docker_step("docker:ps", DeploymentAction::DockerPs, &[])],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "docker ps".to_string(),
            Ok(SshCommandResult {
                stdout: "CONTAINER ID   IMAGE".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_docker_readonly(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute docker plan");
        let record = records.first().expect("record");

        assert!(matches!(record.status, ExecutionStatus::Succeeded));
        assert!(record
            .logs
            .iter()
            .any(|line| line.contains("stdout: CONTAINER ID")));
    }

    #[test]
    fn executes_docker_inspect_command() {
        let plan = DeploymentPlan {
            steps: vec![docker_step(
                "docker:inspect",
                DeploymentAction::DockerInspect {
                    target: "nginx".to_string(),
                },
                &[],
            )],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "docker inspect -- nginx".to_string(),
            Ok(SshCommandResult {
                stdout: "[{\"Id\":\"abc\"}]".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_docker_readonly(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute docker plan");
        let record = records.first().expect("record");

        assert!(matches!(record.status, ExecutionStatus::Succeeded));
        assert!(record
            .logs
            .iter()
            .any(|line| line.contains("stdout: [{\"Id\":\"abc\"}]")));
    }

    #[test]
    fn executes_docker_logs_with_tail() {
        let plan = DeploymentPlan {
            steps: vec![docker_step(
                "docker:logs",
                DeploymentAction::DockerLogs {
                    container: "api".to_string(),
                    tail: 25,
                },
                &[],
            )],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "docker logs --tail 25 -- api".to_string(),
            Ok(SshCommandResult {
                stdout: "line1".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_docker_readonly(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute docker plan");
        let record = records.first().expect("record");

        assert!(matches!(record.status, ExecutionStatus::Succeeded));
        assert!(record
            .logs
            .iter()
            .any(|line| line.contains("stdout: line1")));
    }

    #[test]
    fn docker_failure_blocks_downstream_steps() {
        let plan = DeploymentPlan {
            steps: vec![
                docker_step(
                    "docker:inspect",
                    DeploymentAction::DockerInspect {
                        target: "db".to_string(),
                    },
                    &[],
                ),
                step("deploy:api", &["docker:inspect"]),
            ],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "docker inspect -- db".to_string(),
            Err(SshError::Command {
                exit_code: 1,
                stderr: "not found".to_string(),
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_docker_readonly(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute docker plan");

        let status_for = |id: &str| {
            records
                .iter()
                .find(|record| {
                    record.step_id == id && record.status != ExecutionStatus::WaitingApproval
                })
                .expect("record present")
                .status
                .clone()
        };

        assert!(matches!(
            status_for("docker:inspect"),
            ExecutionStatus::Failed
        ));
        assert!(matches!(status_for("deploy:api"), ExecutionStatus::Skipped));
    }

    #[test]
    fn executes_compose_up_command() {
        let plan = DeploymentPlan {
            steps: vec![docker_step(
                "compose:up",
                DeploymentAction::ComposeUp {
                    project_dir: "/srv/app".to_string(),
                },
                &[],
            )],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "cd -- /srv/app && docker compose up -d".to_string(),
            Ok(SshCommandResult {
                stdout: "Started".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_compose(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute compose plan");
        let record = records.first().expect("record");

        assert!(matches!(record.status, ExecutionStatus::Succeeded));
        assert!(record
            .logs
            .iter()
            .any(|line| line.contains("command: cd -- /srv/app && docker compose up -d")));
    }

    #[test]
    fn executes_compose_down_command() {
        let plan = DeploymentPlan {
            steps: vec![docker_step(
                "compose:down",
                DeploymentAction::ComposeDown {
                    project_dir: "/srv/app".to_string(),
                },
                &[],
            )],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "cd -- /srv/app && docker compose down".to_string(),
            Ok(SshCommandResult {
                stdout: "Removed".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_compose(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute compose plan");
        let record = records.first().expect("record");

        assert!(matches!(record.status, ExecutionStatus::Succeeded));
        assert!(record
            .logs
            .iter()
            .any(|line| line.contains("command: cd -- /srv/app && docker compose down")));
    }

    #[test]
    fn compose_failure_blocks_downstream_steps() {
        let plan = DeploymentPlan {
            steps: vec![
                docker_step(
                    "compose:up",
                    DeploymentAction::ComposeUp {
                        project_dir: "/srv/app".to_string(),
                    },
                    &[],
                ),
                step("deploy:api", &["compose:up"]),
            ],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "cd -- /srv/app && docker compose up -d".to_string(),
            Err(SshError::Command {
                exit_code: 1,
                stderr: "compose failed".to_string(),
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_compose(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute compose plan");

        let status_for = |id: &str| {
            records
                .iter()
                .find(|record| {
                    record.step_id == id && record.status != ExecutionStatus::WaitingApproval
                })
                .expect("record present")
                .status
                .clone()
        };

        assert!(matches!(status_for("compose:up"), ExecutionStatus::Failed));
        assert!(matches!(status_for("deploy:api"), ExecutionStatus::Skipped));
    }

    #[test]
    fn compose_rejects_invalid_directory() {
        let plan = DeploymentPlan {
            steps: vec![
                docker_step(
                    "compose:up",
                    DeploymentAction::ComposeUp {
                        project_dir: "/".to_string(),
                    },
                    &[],
                ),
                step("deploy:api", &["compose:up"]),
            ],
        };
        let client = MockSshClient {
            responses: HashMap::new(),
        };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_compose(plan, host, client)
            .expect("start session");
        let records = session.run().expect("execute compose plan");

        let compose_record = records
            .iter()
            .find(|record| record.step_id == "compose:up")
            .expect("compose record");
        let deploy_record = records
            .iter()
            .find(|record| record.step_id == "deploy:api")
            .expect("deploy record");

        assert!(matches!(compose_record.status, ExecutionStatus::Failed));
        assert!(compose_record
            .logs
            .iter()
            .any(|line| line.contains("compose project_dir cannot be root")));
        assert!(matches!(deploy_record.status, ExecutionStatus::Skipped));
    }

    #[test]
    fn rollback_compose_up_executes_compose_down() {
        let plan = DeploymentPlan {
            steps: vec![docker_step(
                "compose:up",
                DeploymentAction::ComposeUp {
                    project_dir: "/srv/app".to_string(),
                },
                &[],
            )],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "cd -- /srv/app && docker compose up -d".to_string(),
            Ok(SshCommandResult {
                stdout: "Started".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        responses.insert(
            "cd -- /srv/app && docker compose down".to_string(),
            Ok(SshCommandResult {
                stdout: "Removed".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        let client = MockSshClient { responses };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };

        let executor = Executor::new();
        let mut session = executor
            .start_compose(plan, host, client)
            .expect("start session");
        session.run().expect("execute compose plan");

        let records = session.rollback_step("compose:up").expect("rollback step");
        let record = records
            .iter()
            .rev()
            .find(|record| record.step_id == "compose:up")
            .expect("rollback record");

        assert!(matches!(record.status, ExecutionStatus::RolledBack));
        assert!(record
            .logs
            .iter()
            .any(|line| line.contains("command: cd -- /srv/app && docker compose down")));
    }

    fn nginx_config(project: &str, proxy_pass: &str) -> NginxProjectConfig {
        NginxProjectConfig {
            project: project.to_string(),
            server_name: "example.com".to_string(),
            listen: 80,
            locations: vec![NginxLocation {
                path: "/".to_string(),
                proxy_pass: proxy_pass.to_string(),
            }],
            ssl_domain: None,
        }
    }

    #[test]
    fn nginx_apply_requires_approval() {
        let plan = DeploymentPlan {
            steps: vec![DeploymentStep {
                id: "nginx:apply".to_string(),
                service_name: "nginx".to_string(),
                action: DeploymentAction::NginxApply {
                    project: "demo".to_string(),
                },
                depends_on: Vec::new(),
                approval_required: false,
                memo: None,
            }],
        };
        let client = MockSshClient {
            responses: HashMap::new(),
        };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };
        let executor = Executor::new();
        let mut session = executor
            .start_nginx(
                plan,
                host,
                client,
                vec![nginx_config("demo", "http://localhost")],
            )
            .expect("start session");

        let result = session.run();
        assert!(matches!(result, Err(ExecutionError::InvalidPlan { .. })));
    }

    #[test]
    fn nginx_validation_rejects_bad_proxy_pass() {
        let plan = DeploymentPlan {
            steps: vec![DeploymentStep {
                id: "nginx:apply".to_string(),
                service_name: "nginx".to_string(),
                action: DeploymentAction::NginxApply {
                    project: "demo".to_string(),
                },
                depends_on: Vec::new(),
                approval_required: true,
                memo: None,
            }],
        };
        let client = MockSshClient {
            responses: HashMap::new(),
        };
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };
        let executor = Executor::new();
        let mut session = executor
            .start_nginx(plan, host, client, vec![nginx_config("demo", "ftp://bad")])
            .expect("start session");
        session.run().expect("run to approval");

        let records = session.approve_step("nginx:apply").expect("approve step");
        let record = records
            .iter()
            .rev()
            .find(|record| record.step_id == "nginx:apply")
            .expect("record");

        assert!(matches!(record.status, ExecutionStatus::Failed));
        assert!(record
            .logs
            .iter()
            .any(|line| line.contains("proxy_pass must start with http:// or https://")));
    }

    #[test]
    fn nginx_runs_test_before_reload() {
        let plan = DeploymentPlan {
            steps: vec![DeploymentStep {
                id: "nginx:apply".to_string(),
                service_name: "nginx".to_string(),
                action: DeploymentAction::NginxApply {
                    project: "demo".to_string(),
                },
                depends_on: Vec::new(),
                approval_required: true,
                memo: None,
            }],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "nginx -t".to_string(),
            Ok(SshCommandResult {
                stdout: "ok".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        responses.insert(
            "nginx reload".to_string(),
            Ok(SshCommandResult {
                stdout: "reloaded".to_string(),
                stderr: "".to_string(),
                exit_code: 0,
            }),
        );
        let commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = RecordingSshClient::new(responses, commands.clone());
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };
        let executor = Executor::new();
        let mut session = executor
            .start_nginx(
                plan,
                host,
                client,
                vec![nginx_config("demo", "http://localhost")],
            )
            .expect("start session");
        session.run().expect("run to approval");

        let records = session.approve_step("nginx:apply").expect("approve step");
        assert!(records.iter().any(|record| {
            record.step_id == "nginx:apply" && matches!(record.status, ExecutionStatus::Succeeded)
        }));

        let captured = commands.lock().expect("commands lock").clone();
        let test_index = captured
            .iter()
            .position(|cmd| cmd == "nginx -t")
            .expect("nginx -t executed");
        let reload_index = captured
            .iter()
            .position(|cmd| cmd == "nginx reload")
            .expect("nginx reload executed");
        assert!(test_index < reload_index);
    }

    #[test]
    fn nginx_failure_halts_execution() {
        let plan = DeploymentPlan {
            steps: vec![
                DeploymentStep {
                    id: "nginx:apply".to_string(),
                    service_name: "nginx".to_string(),
                    action: DeploymentAction::NginxApply {
                        project: "demo".to_string(),
                    },
                    depends_on: Vec::new(),
                    approval_required: true,
                    memo: None,
                },
                step("deploy:api", &["nginx:apply"]),
            ],
        };
        let mut responses = HashMap::new();
        responses.insert(
            "nginx -t".to_string(),
            Err(SshError::Command {
                exit_code: 1,
                stderr: "syntax error".to_string(),
            }),
        );
        let commands = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let client = RecordingSshClient::new(responses, commands);
        let host = HostContext {
            host: "127.0.0.1".to_string(),
            user: "tester".to_string(),
            port: 22,
            environment: Environment::Dev,
        };
        let executor = Executor::new();
        let mut session = executor
            .start_nginx(
                plan,
                host,
                client,
                vec![nginx_config("demo", "http://localhost")],
            )
            .expect("start session");
        session.run().expect("run to approval");

        let records = session.approve_step("nginx:apply").expect("approve step");

        assert!(records.iter().any(|record| {
            record.step_id == "nginx:apply" && matches!(record.status, ExecutionStatus::Failed)
        }));
        assert!(!records.iter().any(|record| record.step_id == "deploy:api"));
    }
}

// P0-4, P0-5: LLM 驱动的执行循环框架

/// 执行状态，用于 LLM 分析
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionState {
    pub execution_id: String,
    pub project_name: String,
    pub current_step: CurrentStepInfo,
    pub execution_logs: Vec<String>,
    pub system_metrics: SystemMetrics,
    pub container_status: Vec<ContainerStatus>,
    pub completed_steps: Vec<CompletedStepInfo>,
    pub pending_steps: Vec<PendingStepInfo>,
    pub start_time: String,
    pub elapsed_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentStepInfo {
    pub id: String,
    pub service_name: String,
    pub command: String,
    pub status: String, // "running", "succeeded", "failed", "waiting_approval"
    pub step_start_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerStatus {
    pub name: String,
    pub status: String,
    pub health: String,
    pub uptime: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedStepInfo {
    pub id: String,
    pub service_name: String,
    pub status: String,
    pub duration: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingStepInfo {
    pub id: String,
    pub service_name: String,
    pub description: String,
}

/// LLM 执行分析响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMExecutionAnalysis {
    pub status_analysis: StatusAnalysis,
    pub problem_diagnosis: ProblemDiagnosis,
    pub action_plan: ActionPlan,
    pub validation_checklist: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusAnalysis {
    pub current_status: String, // "succeeded", "failed", "warning", "needs_attention"
    pub summary: String,
    pub issues: Vec<IssueInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueInfo {
    pub severity: String, // "warning", "error", "critical"
    pub description: String,
    pub impact: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProblemDiagnosis {
    pub root_cause: String,
    pub related_issues: Vec<String>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionPlan {
    pub recommended_actions: Vec<RecommendedAction>,
    pub next_step_id: Option<String>,
    pub should_continue: bool,
    pub should_rollback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedAction {
    pub id: String,
    pub r#type: String, // "continue", "rollback", "retry", "manual_intervention", "adjust_plan"
    pub description: String,
    pub command: Option<String>,
    pub estimated_duration: Option<String>,
    pub risk_level: String, // "Low", "Medium", "High", "Critical"
    pub requires_approval: bool,
}

/// 构建执行分析的 Prompt
pub fn build_execution_analysis_prompt(state: &ExecutionState) -> Result<String, ExecutionError> {
    let prompt = format!(
        r#"
# 角色定义
你是一位资深的 DevOps/SRE 工程师，负责监控和优化部署执行过程。

# 任务目标
基于部署执行的日志、状态和反馈，分析当前情况，识别问题，并生成下一步的行动建议。

# 输入信息

## 1. 部署上下文
- 项目名称: {}
- 执行ID: {}
- 当前步骤: {} ({})
- 执行开始时间: {}
- 已执行时间: {}

## 2. 当前步骤状态
- 步骤ID: {}
- 步骤名称: {}
- 服务名称: {}
- 执行命令: {}
- 状态: {}

## 3. 执行日志
```
{}
```

## 4. 系统指标
- CPU 使用率: {}%
- 内存使用率: {}%
- 磁盘使用率: {}%

## 5. 容器状态
```json
{}
```

## 6. 已完成的步骤
```json
{}
```

## 7. 待执行的步骤
```json
{}
```

# 输出要求

请以 JSON 格式输出分析结果和行动建议：

{{
  "status_analysis": {{
    "current_status": "succeeded|failed|warning|needs_attention",
    "summary": "当前步骤执行成功，服务已启动",
    "issues": []
  }},
  "problem_diagnosis": {{
    "root_cause": "",
    "related_issues": [],
    "confidence": 0.85
  }},
  "action_plan": {{
    "recommended_actions": [
      {{
        "id": "action_1",
        "type": "continue|rollback|retry|manual_intervention",
        "description": "继续执行下一步",
        "command": "docker-compose up -d api",
        "estimated_duration": "3分钟",
        "risk_level": "Low",
        "requires_approval": false
      }}
    ],
    "next_step_id": "step_3",
    "should_continue": true,
    "should_rollback": false
  }},
  "validation_checklist": [],
  "recommendations": []
}}

# 输出格式

请严格按照 JSON 格式输出。
"#,
        state.project_name,
        state.execution_id,
        state.current_step.id,
        state.current_step.service_name,
        state.start_time,
        state.elapsed_time,
        state.current_step.id,
        state.current_step.service_name,
        state.current_step.service_name,
        state.current_step.command,
        state.current_step.status,
        state.execution_logs.join("\n"),
        format!("{:.1}", state.system_metrics.cpu_usage),
        format!("{:.1}", state.system_metrics.memory_usage),
        format!("{:.1}", state.system_metrics.disk_usage),
        serde_json::to_string(&state.container_status).unwrap_or_else(|_| "[]".to_string()),
        serde_json::to_string(&state.completed_steps).unwrap_or_else(|_| "[]".to_string()),
        serde_json::to_string(&state.pending_steps).unwrap_or_else(|_| "[]".to_string()),
    );

    Ok(prompt)
}

/// 从 LLM 响应中提取 JSON（复用 agus-planner 的逻辑）
fn extract_json_from_execution_response(response: &str) -> String {
    // 尝试提取 ```json ... ``` 代码块
    if let Some(start) = response.find("```json") {
        if let Some(end) = response[start..].find("```") {
            return response[start + 7..start + end].trim().to_string();
        }
    }

    // 尝试提取 ``` ... ``` 代码块
    if let Some(start) = response.find("```") {
        if let Some(end) = response[start + 3..].find("```") {
            let content = &response[start + 3..start + 3 + end];
            if content.trim().starts_with('{') {
                return content.trim().to_string();
            }
        }
    }

    // 尝试提取 {...} JSON 对象
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            return response[start..=end].to_string();
        }
    }

    response.to_string()
}

/// 解析 LLM 执行分析响应
pub fn parse_llm_execution_response(
    response: &str,
) -> Result<LLMExecutionAnalysis, ExecutionError> {
    let json_str = extract_json_from_execution_response(response);

    let analysis: LLMExecutionAnalysis =
        serde_json::from_str(&json_str).map_err(|e| ExecutionError::SessionError {
            message: format!(
                "Failed to parse LLM execution response: {}. Response: {}",
                e, json_str
            ),
        })?;

    Ok(analysis)
}

/// LLM 驱动的执行会话
/// 在执行过程中调用 LLM 进行分析和决策
pub struct LLMDrivenExecutionSession {
    session: ExecutionSession,
    execution_id: String,
    project_name: String,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    execution_logs: Vec<String>,
    start_time: std::time::Instant,
}

impl LLMDrivenExecutionSession {
    /// 创建新的 LLM 驱动执行会话
    pub fn new(
        session: ExecutionSession,
        execution_id: String,
        project_name: String,
        llm_provider: Option<Arc<dyn LlmProvider>>,
    ) -> Self {
        Self {
            session,
            execution_id,
            project_name,
            llm_provider,
            execution_logs: Vec::new(),
            start_time: std::time::Instant::now(),
        }
    }

    pub fn into_session(self) -> ExecutionSession {
        self.session
    }

    pub fn session_mut(&mut self) -> &mut ExecutionSession {
        &mut self.session
    }

    /// 逐步执行真实步骤；每步后可选调用 LLM 分析。
    /// - WaitingApproval：暂停返回（由 UI `approve_step` 继续）
    /// - should_rollback：对最近成功步骤尝试 rollback，并返回错误
    /// - 需人工审批的推荐动作：写入日志后暂停返回
    pub fn run_with_llm_analysis(&mut self) -> Result<Vec<ExecutionRecord>, ExecutionError> {
        let Some(provider) = self.llm_provider.clone() else {
            return self.session.run();
        };

        loop {
            self.session.run_one_step()?;

            if let Some(step_id) = self.session.pending_approval_step() {
                if let Some(stream) = &self.session.log_stream {
                    stream.warning(
                        &step_id,
                        &format!("Paused for approval on step {step_id}"),
                    );
                }
                return Ok(self.session.records.clone());
            }

            if self.session.is_complete() {
                break;
            }

            let Some(last) = self.session.last_record().cloned() else {
                break;
            };
            self.execution_logs
                .extend(last.logs.iter().cloned().take(40));

            let state = self.collect_execution_state_from_session(&last)?;
            let prompt = build_execution_analysis_prompt(&state)?;
            let raw = provider.complete_prompt(&prompt).map_err(|e| {
                ExecutionError::SessionError {
                    message: format!("LLM analysis failed: {e}"),
                }
            })?;
            let analysis = parse_llm_execution_response(&raw)?;

            if let Some(stream) = &self.session.log_stream {
                stream.info(
                    &last.step_id,
                    &format!(
                        "LLM analysis: {} — {}",
                        analysis.status_analysis.current_status, analysis.status_analysis.summary
                    ),
                );
            }

            if analysis.action_plan.should_rollback {
                let target = self
                    .session
                    .records
                    .iter()
                    .rev()
                    .find(|r| r.status == ExecutionStatus::Succeeded)
                    .map(|r| r.step_id.clone());
                if let Some(step_id) = target {
                    let _ = self.session.rollback_step(&step_id);
                }
                return Err(ExecutionError::SessionError {
                    message: format!(
                        "LLM recommended rollback after step {}: {}",
                        last.step_id, analysis.problem_diagnosis.root_cause
                    ),
                });
            }

            let needs_human = analysis
                .action_plan
                .recommended_actions
                .iter()
                .any(|a| a.requires_approval);
            if needs_human {
                if let Some(stream) = &self.session.log_stream {
                    stream.warning(
                        &last.step_id,
                        "LLM recommended actions require human approval; pausing execution",
                    );
                }
                return Ok(self.session.records.clone());
            }

            if !analysis.action_plan.should_continue {
                if let Some(stream) = &self.session.log_stream {
                    stream.warning(
                        &last.step_id,
                        "LLM advised stopping further steps; pausing execution",
                    );
                }
                return Ok(self.session.records.clone());
            }
        }

        Ok(self.session.records.clone())
    }

    fn collect_execution_state_from_session(
        &self,
        last: &ExecutionRecord,
    ) -> Result<ExecutionState, ExecutionError> {
        let status = match last.status {
            ExecutionStatus::Succeeded => "succeeded",
            ExecutionStatus::Simulated => "simulated",
            ExecutionStatus::Failed => "failed",
            ExecutionStatus::WaitingApproval => "waiting_approval",
            ExecutionStatus::Skipped => "skipped",
            ExecutionStatus::RolledBack => "rolled_back",
            ExecutionStatus::Running => "running",
            ExecutionStatus::Pending => "pending",
        };
        let completed: Vec<CompletedStepInfo> = self
            .session
            .records
            .iter()
            .filter(|r| {
                matches!(
                    r.status,
                    ExecutionStatus::Succeeded
                        | ExecutionStatus::Simulated
                        | ExecutionStatus::RolledBack
                        | ExecutionStatus::Skipped
                )
            })
            .map(|r| CompletedStepInfo {
                id: r.step_id.clone(),
                service_name: r.step_id.clone(),
                status: format!("{:?}", r.status),
                duration: "-".to_string(),
            })
            .collect();
        let pending: Vec<PendingStepInfo> = self
            .session
            .order
            .iter()
            .skip(self.session.index)
            .filter_map(|id| {
                self.session.steps_by_id.get(id).map(|step| PendingStepInfo {
                    id: id.clone(),
                    service_name: step.service_name.clone(),
                    description: step.memo.clone().unwrap_or_default(),
                })
            })
            .collect();

        Ok(ExecutionState {
            execution_id: self.execution_id.clone(),
            project_name: self.project_name.clone(),
            current_step: CurrentStepInfo {
                id: last.step_id.clone(),
                service_name: last.step_id.clone(),
                command: last.logs.first().cloned().unwrap_or_default(),
                status: status.to_string(),
                step_start_time: "-".to_string(),
            },
            execution_logs: self.execution_logs.clone(),
            system_metrics: SystemMetrics {
                cpu_usage: 0.0,
                memory_usage: 0.0,
                disk_usage: 0.0,
            },
            container_status: Vec::new(),
            completed_steps: completed,
            pending_steps: pending,
            start_time: "-".to_string(),
            elapsed_time: format!("{:.1}s", self.start_time.elapsed().as_secs_f64()),
        })
    }
}
