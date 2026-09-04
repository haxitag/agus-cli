//! 确定性漏洞匹配：调用公开 OSV API（https://api.osv.dev），非 LLM 猜测。
//! 仅对可映射到 OSV ecosystem 的包查询；匹配失败返回 Err，不得伪造空「安全」结论。

use crate::vulnerability::{SoftwareVersion, SystemVulnerabilityContext};
use serde::{Deserialize, Serialize};

const OSV_QUERYBATCH_URL: &str = "https://api.osv.dev/v1/querybatch";
const MAX_QUERIES: usize = 80;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OsvMatch {
    pub package_name: String,
    pub package_version: String,
    pub ecosystem: String,
    pub source: String,
    pub vulnerability_id: String,
    pub summary: String,
    pub severity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsvMatchReport {
    pub queried: usize,
    pub matched: usize,
    pub matches: Vec<OsvMatch>,
    pub skipped_unmapped: usize,
    pub api_url: String,
    pub notes: Vec<String>,
}

#[derive(Debug)]
pub enum OsvError {
    Network { message: String },
    Api { status: u16, message: String },
    Parse { message: String },
}

impl std::fmt::Display for OsvError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OsvError::Network { message } => write!(f, "OSV network error: {message}"),
            OsvError::Api { status, message } => write!(f, "OSV API {status}: {message}"),
            OsvError::Parse { message } => write!(f, "OSV parse error: {message}"),
        }
    }
}

impl std::error::Error for OsvError {}

#[derive(Serialize)]
struct OsvQueryBatch {
    queries: Vec<OsvQuery>,
}

#[derive(Serialize)]
struct OsvQuery {
    package: OsvPackage,
    version: String,
}

#[derive(Serialize)]
struct OsvPackage {
    name: String,
    ecosystem: String,
}

#[derive(Deserialize)]
struct OsvQueryBatchResponse {
    results: Vec<OsvQueryResult>,
}

#[derive(Deserialize)]
struct OsvQueryResult {
    #[serde(default)]
    vulns: Vec<OsvVuln>,
}

#[derive(Deserialize)]
struct OsvVuln {
    id: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    severity: Vec<OsvSeverity>,
}

#[derive(Deserialize)]
struct OsvSeverity {
    #[serde(default)]
    score: String,
    #[serde(rename = "type", default)]
    severity_type: String,
}

/// 从 os-release 文本推断 OSV ecosystem
pub fn infer_osv_ecosystem(os_info: &str) -> Option<&'static str> {
    let lower = os_info.to_ascii_lowercase();
    if lower.contains("ubuntu") {
        Some("Ubuntu")
    } else if lower.contains("debian") {
        Some("Debian")
    } else if lower.contains("alpine") {
        Some("Alpine")
    } else if lower.contains("red hat") || lower.contains("rhel") || lower.contains("centos") || lower.contains("rocky") || lower.contains("alma") {
        Some("Red Hat")
    } else {
        None
    }
}

fn prioritize_packages(packages: &[SoftwareVersion]) -> Vec<&SoftwareVersion> {
    const PRIORITY: &[&str] = &[
        "openssl", "openssh", "openssh-server", "openssh-client", "curl", "wget",
        "nginx", "apache2", "httpd", "python3", "python", "nodejs", "npm",
        "docker", "containerd", "git", "sudo", "bash", "glibc", "libc6",
        "mysql", "mariadb", "postgresql", "redis", "redis-server",
    ];
    let mut selected: Vec<&SoftwareVersion> = packages
        .iter()
        .filter(|p| {
            let n = p.name.to_ascii_lowercase();
            PRIORITY.iter().any(|k| n == *k || n.starts_with(&format!("{k}-")))
        })
        .collect();
    if selected.len() < MAX_QUERIES / 2 {
        for p in packages {
            if selected.len() >= MAX_QUERIES {
                break;
            }
            if !selected.iter().any(|s| s.name == p.name && s.version == p.version) {
                selected.push(p);
            }
        }
    }
    selected.truncate(MAX_QUERIES);
    selected
}

fn process_ecosystem(name: &str) -> Option<&'static str> {
    match name.to_ascii_lowercase().as_str() {
        "nginx" | "redis" | "mysql" | "docker" | "ssh" => None, // 用发行版包名更准
        _ => None,
    }
}

/// 对上下文中的软件包做 OSV querybatch 确定性匹配。
pub fn match_osv_vulnerabilities(context: &SystemVulnerabilityContext) -> Result<OsvMatchReport, OsvError> {
    let mut notes = Vec::new();
    let ecosystem = infer_osv_ecosystem(&context.os_info);
    let skipped_unmapped: usize;

    let Some(ecosystem) = ecosystem else {
        notes.push(format!(
            "无法从 os_info=\"{}\" 映射到 OSV ecosystem（仅支持 Ubuntu/Debian/Alpine/RHEL 系）。未发起查询。",
            context.os_info
        ));
        return Ok(OsvMatchReport {
            queried: 0,
            matched: 0,
            matches: Vec::new(),
            skipped_unmapped: context.installed_packages.len(),
            api_url: OSV_QUERYBATCH_URL.to_string(),
            notes,
        });
    };

    let packages = prioritize_packages(&context.installed_packages);
    skipped_unmapped = context
        .installed_packages
        .len()
        .saturating_sub(packages.len());

    // 中间件进程：仅在包列表里没有同名时补充查询（需能映射到发行版包名）
    let queries: Vec<(String, String, String, String)> = packages
        .iter()
        .filter(|p| !p.version.trim().is_empty() && p.version != "unknown")
        .map(|p| {
            (
                p.name.clone(),
                p.version.clone(),
                ecosystem.to_string(),
                p.source.clone(),
            )
        })
        .collect();

    for proc in &context.running_processes {
        if process_ecosystem(&proc.name).is_some() {
            // reserved for future ecosystem-specific process mapping
        }
        let _ = proc;
    }

    if queries.is_empty() {
        notes.push("没有可查询的已安装包版本（列表为空或版本缺失）".to_string());
        return Ok(OsvMatchReport {
            queried: 0,
            matched: 0,
            matches: Vec::new(),
            skipped_unmapped,
            api_url: OSV_QUERYBATCH_URL.to_string(),
            notes,
        });
    }

    notes.push(format!(
        "使用 OSV ecosystem={}，查询 {} 个包（优先安全相关组件，上限 {}）",
        ecosystem,
        queries.len(),
        MAX_QUERIES
    ));

    let body = OsvQueryBatch {
        queries: queries
            .iter()
            .map(|(name, version, eco, _)| OsvQuery {
                package: OsvPackage {
                    name: name.clone(),
                    ecosystem: eco.clone(),
                },
                version: version.clone(),
            })
            .collect(),
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .user_agent("Agus-OSV-Scanner/0.2")
        .build()
        .map_err(|e| OsvError::Network {
            message: e.to_string(),
        })?;

    let response = client
        .post(OSV_QUERYBATCH_URL)
        .json(&body)
        .send()
        .map_err(|e| OsvError::Network {
            message: e.to_string(),
        })?;

    let status = response.status();
    if !status.is_success() {
        let message = response.text().unwrap_or_default();
        return Err(OsvError::Api {
            status: status.as_u16(),
            message: message.chars().take(500).collect(),
        });
    }

    let parsed: OsvQueryBatchResponse = response.json().map_err(|e| OsvError::Parse {
        message: e.to_string(),
    })?;

    let mut matches = Vec::new();
    for (idx, result) in parsed.results.iter().enumerate() {
        let Some((name, version, eco, source)) = queries.get(idx) else {
            continue;
        };
        for vuln in &result.vulns {
            let severity = vuln
                .severity
                .iter()
                .find(|s| !s.score.is_empty())
                .map(|s| {
                    if s.severity_type.is_empty() {
                        s.score.clone()
                    } else {
                        format!("{}:{}", s.severity_type, s.score)
                    }
                });
            matches.push(OsvMatch {
                package_name: name.clone(),
                package_version: version.clone(),
                ecosystem: eco.clone(),
                source: source.clone(),
                vulnerability_id: vuln.id.clone(),
                summary: if vuln.summary.is_empty() {
                    vuln.id.clone()
                } else {
                    vuln.summary.clone()
                },
                severity,
            });
        }
    }

    Ok(OsvMatchReport {
        queried: queries.len(),
        matched: matches.len(),
        matches,
        skipped_unmapped,
        api_url: OSV_QUERYBATCH_URL.to_string(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_ubuntu_ecosystem() {
        assert_eq!(
            infer_osv_ecosystem("Ubuntu 22.04.3 LTS"),
            Some("Ubuntu")
        );
        assert_eq!(infer_osv_ecosystem("Something Else"), None);
    }

    #[test]
    fn prioritizes_openssl() {
        let pkgs = vec![
            SoftwareVersion {
                name: "hello".into(),
                version: "1".into(),
                source: "dpkg".into(),
            },
            SoftwareVersion {
                name: "openssl".into(),
                version: "3.0.2".into(),
                source: "dpkg".into(),
            },
        ];
        let selected = prioritize_packages(&pkgs);
        assert_eq!(selected[0].name, "openssl");
    }
}
