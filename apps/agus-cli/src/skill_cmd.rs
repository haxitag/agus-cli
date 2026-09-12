use agus_cli_core::CliError;
use agus_observer::{collect_system_metrics, SystemMetrics};
use agus_skill_runtime::{
    resolve_agus_home, SkillEvidence, SkillReport, SkillService, SkillSummary, StoredSkillRun,
};
use agus_ssh::ProcessSshClient;
use clap::{Args, Subcommand};
use serde_json;

use agus_cli_core::{exec, hosts};

#[derive(Subcommand)]
pub enum SkillCommand {
    List,
    Show(SkillShowArgs),
    Run(SkillRunArgs),
    Approve(SkillApproveArgs),
    Reject(SkillRejectArgs),
    Reports(SkillReportsArgs),
}

#[derive(Args)]
pub struct SkillShowArgs {
    pub id: String,
}

#[derive(Args)]
pub struct SkillRunArgs {
    pub id: String,
    #[arg(long)]
    pub host: Option<String>,
    #[arg(long)]
    pub message: Option<String>,
    #[arg(long)]
    pub dry_run: bool,
    #[arg(long)]
    pub yes: bool,
}

#[derive(Args)]
pub struct SkillApproveArgs {
    pub run_id: String,
    pub proposal_id: String,
}

#[derive(Args)]
pub struct SkillRejectArgs {
    pub run_id: String,
    pub proposal_id: String,
}

#[derive(Args)]
pub struct SkillReportsArgs {
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

pub fn handle_skill(cmd: SkillCommand) -> Result<(), CliError> {
    let svc = SkillService::with_home(resolve_agus_home())
        .map_err(|e| CliError::Config(format!("skill service: {e}")))?;

    match cmd {
        SkillCommand::List => {
            let items = svc.list();
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
        SkillCommand::Show(args) => {
            let pkg = svc
                .get(&args.id)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            let summary = SkillSummary {
                id: pkg.manifest.id.clone(),
                version: pkg.manifest.version.clone(),
                description: pkg.manifest.description.trim().to_string(),
                risk_class: pkg.manifest.risk_class,
                permissions: pkg.manifest.permissions.clone(),
                may_auto_run: pkg.manifest.may_auto_run(),
            };
            println!("{}", serde_json::to_string_pretty(&summary)?);
        }
        SkillCommand::Run(args) => {
            let report = run_skill(&svc, &args)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        SkillCommand::Approve(args) => {
            let report = svc
                .approve_proposal(&args.run_id, &args.proposal_id, true)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        SkillCommand::Reject(args) => {
            let report = svc
                .approve_proposal(&args.run_id, &args.proposal_id, false)
                .map_err(|e| CliError::InvalidInput(e.to_string()))?;
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        SkillCommand::Reports(args) => {
            let reports: Vec<StoredSkillRun> = svc
                .list_reports(args.limit)
                .map_err(|e| CliError::Config(e.to_string()))?;
            println!("{}", serde_json::to_string_pretty(&reports)?);
        }
    }
    Ok(())
}

fn run_skill(svc: &SkillService, args: &SkillRunArgs) -> Result<SkillReport, CliError> {
    let trigger = args
        .message
        .clone()
        .unwrap_or_else(|| format!("cli run {}", args.id));
    let findings = args
        .message
        .clone()
        .map(|m| vec![m])
        .unwrap_or_default();
    let evidence = collect_evidence(args.host.as_deref())?;

    let pkg = svc
        .get(&args.id)
        .map_err(|e| CliError::InvalidInput(e.to_string()))?;
    let propose_capable = pkg
        .manifest
        .permissions
        .iter()
        .any(|p| matches!(p, agus_skill_runtime::Permission::ProposeExecute));

    let report = if propose_capable {
        // dry_run still drafts allowlisted proposals; --yes alone can approve (never execute).
        svc.run_diagnose_with_proposals(&args.id, &trigger, findings, evidence, None)
    } else {
        svc.run_readonly(&args.id, &trigger, findings, evidence)
    }
    .map_err(|e| CliError::Config(e.to_string()))?;

    if args.yes && !args.dry_run {
        if let Some(proposal) = report
            .proposals
            .iter()
            .find(|p| p.status == agus_skill_runtime::ProposalStatus::WaitingApproval)
        {
            return svc
                .approve_proposal(&report.run_id, &proposal.id, true)
                .map_err(|e| CliError::Config(e.to_string()));
        }
    }

    Ok(report)
}

fn collect_evidence(host_id: Option<&str>) -> Result<Vec<SkillEvidence>, CliError> {
    let Some(host_id) = host_id else {
        return Ok(Vec::new());
    };
    let host = hosts::find_host(host_id)?;
    let target = exec::ssh_target_from_host(&host);
    let client = ProcessSshClient::new();
    match collect_system_metrics(&client, &target, &host.id) {
        Ok(metrics) => Ok(vec![metrics_to_evidence(&metrics)]),
        Err(e) => {
            let summary = format!("metrics collection failed: {e}");
            Ok(vec![agus_skill_runtime::SkillRuntime::now_evidence(
                "synthetic",
                &summary,
                None,
                None,
            )])
        }
    }
}

fn metrics_to_evidence(metrics: &SystemMetrics) -> SkillEvidence {
    let disk_pct = metrics
        .disk
        .first()
        .map(|d| d.usage_percent)
        .unwrap_or(0.0);
    let summary = format!(
        "cpu={:.1}% mem={:.1}% disk={:.1}%",
        metrics.cpu.usage_percent, metrics.memory.usage_percent, disk_pct
    );
    agus_skill_runtime::SkillRuntime::now_evidence(
        "metrics",
        &summary,
        Some("observer.collect_system_metrics"),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dry_run_skill_list_smoke() {
        let svc = SkillService::new().expect("service");
        let list = svc.list();
        assert!(!list.is_empty());
    }
}
