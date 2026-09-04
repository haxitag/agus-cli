use agus_core_domain::ServiceDependencyGraph;
use futures_util::Stream;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

/// 流式响应的 chunk 类型，区分普通内容、思考内容与用量
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// 普通回答内容
    Content(String),
    /// 思考过程内容（reasoning / thinking）
    Reasoning(String),
    /// API 返回的真实 token 用量（若 provider 支持）
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
    },
}

/// 聊天消息（用于多轮对话上下文）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LlmProviderType {
    OpenAI,
    Claude,
    Ollama,
    Gemini,
    OpenRouter,
    AlibabaQwen,
    DeepSeek,
    MinMAX,
    Zhipu,
    /// 阿里云百炼 DashScope（compatible-mode，支持多模态）
    DashScope,
    /// NVIDIA NIM API（integrate.api.nvidia.com）
    NvidiaNim,
}

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: LlmProviderType,
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceAnalysis {
    pub performance_notes: Vec<String>,
    pub security_concerns: Vec<String>,
    pub deployment_order_suggestions: Vec<String>,
    pub resource_requirements: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskAssessment {
    pub risk_level: String,
    pub concerns: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorDiagnosis {
    pub error_type: String,
    pub root_cause: String,
    pub severity: String, // "Low", "Medium", "High", "Critical"
    pub possible_causes: Vec<String>,
    pub suggested_fixes: Vec<String>,
    pub prevention_tips: Vec<String>,
    #[serde(default)]
    pub fix_commands: Vec<String>,
    #[serde(default)]
    pub verification_steps: Vec<String>,
    #[serde(default)]
    pub rollback_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceEvaluation {
    pub service_name: String,
    pub overall_score: f64, // 0.0 - 100.0
    pub cpu_usage_analysis: String,
    pub memory_usage_analysis: String,
    pub network_analysis: String,
    pub bottlenecks: Vec<String>,
    pub optimization_suggestions: Vec<String>,
    pub scalability_assessment: String,
    pub resource_recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyAnalysis {
    pub service_name: String,
    pub dependencies: Vec<DependencyInfo>,
    pub dependents: Vec<String>,
    pub critical_path: Vec<String>,
    pub circular_risk: bool,
    pub deployment_order: Vec<String>,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyInfo {
    pub service_name: String,
    pub dependency_type: String, // "required", "optional", "weak"
    pub impact_level: String,    // "critical", "high", "medium", "low"
    pub description: String,
}

// P0-2: 部署计划制定相关数据结构

/// 部署计划上下文，包含生成部署计划所需的所有信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentPlanContext {
    pub project_name: String,
    pub project_id: String,
    pub host_id: String,
    pub host_address: String,
    pub environment: String, // "dev", "test", "staging", "prod"
    pub local_repo_path: String,
    pub remote_repo_path: String,
    pub sync_status: String,
    pub code_consistency_status: String,
    pub last_sync_time: String,
    pub dependency_graph: ServiceDependencyGraph,
    pub remote_state: RemoteEnvironmentState,
}

/// 远程环境状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEnvironmentState {
    pub docker_version: String,
    pub compose_version: String,
    pub running_containers_count: usize,
    pub available_images_count: usize,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
}

/// LLM 返回的部署计划响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMDeploymentPlanResponse {
    pub deployment_plan: DeploymentPlanDraft,
    pub risk_assessment: RiskAssessment,
    pub dry_run_analysis: DryRunAnalysis,
    pub validation_checklist: Vec<String>,
}

/// 部署计划草案
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentPlanDraft {
    pub steps: Vec<DeploymentStepDraft>,
    pub total_estimated_duration: String,
}

/// 部署步骤草案
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentStepDraft {
    pub id: String,
    pub service_name: String,
    pub action: String, // "DeployService" | "VerifyService"
    pub description: String,
    pub command: String,
    pub depends_on: Vec<String>,
    pub estimated_duration: String,
    pub rollback_command: Option<String>,
}

/// 推演分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunAnalysis {
    pub simulated_steps: Vec<String>,
    pub potential_issues: Vec<String>,
    pub recommendations: Vec<String>,
}

// ========== 环境扫描报告分析 ==========

/// 扫描报告分析结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReportAnalysis {
    pub executive_summary: String,           // 执行摘要
    pub environment_assessment: String,      // 环境评估
    pub security_analysis: String,           // 安全分析
    pub resource_utilization: String,        // 资源利用分析
    pub alignment_analysis: String,          // 对齐状态分析
    pub risk_assessment: Vec<String>,        // 风险评估
    pub action_recommendations: Vec<String>, // 行动建议
    pub priority_actions: Vec<String>,       // 优先级行动
}

/// 扫描报告上下文
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReportContext {
    pub project_name: String,
    pub host_address: String,
    pub host_user: String,
    pub scanned_at: String,
    pub repo_graph: Option<ServiceDependencyGraph>,
    pub docker_images: Vec<DockerImageSummary>,
    pub docker_containers: Vec<DockerContainerSummary>,
    pub warnings: Vec<String>,
    pub alignment_suggestions: Vec<String>,
}

/// Docker镜像摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerImageSummary {
    pub repository: String,
    pub tag: String,
    pub id: String,
    pub created_at: String,
    pub size: String,
}

/// Docker容器摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerContainerSummary {
    pub name: String,
    pub image: String,
    pub status: String,
    pub ports: String,
}

#[derive(Debug, Clone)]
pub enum LlmError {
    ApiError { message: String },
    NetworkError { message: String },
    ParseError { message: String },
    ConfigError { message: String },
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LlmError::ApiError { message } => write!(f, "LLM API error: {}", message),
            LlmError::NetworkError { message } => write!(f, "Network error: {}", message),
            LlmError::ParseError { message } => write!(f, "Parse error: {}", message),
            LlmError::ConfigError { message } => write!(f, "Config error: {}", message),
        }
    }
}

impl Error for LlmError {}

static LLM_RUNTIME: OnceLock<Result<tokio::runtime::Runtime, LlmError>> = OnceLock::new();

fn llm_runtime() -> Result<&'static tokio::runtime::Runtime, LlmError> {
    let result = LLM_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(|e| LlmError::ConfigError {
                message: format!("Failed to create tokio runtime: {}", e),
            })
    });
    match result {
        Ok(runtime) => Ok(runtime),
        Err(err) => Err(err.clone()),
    }
}

fn run_async<F, T>(future: F) -> Result<T, LlmError>
where
    F: Future<Output = Result<T, LlmError>>,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| handle.block_on(future))
    } else {
        llm_runtime()?.block_on(future)
    }
}

pub trait LlmProvider: Send + Sync {
    fn analyze_services(&self, graph: &ServiceDependencyGraph)
        -> Result<ServiceAnalysis, LlmError>;

    fn generate_memo(&self, service_name: &str, action: &str) -> Result<String, LlmError>;

    fn assess_risk(&self, service_name: &str, action: &str) -> Result<RiskAssessment, LlmError>;

    fn diagnose_error(
        &self,
        error_message: &str,
        error_logs: &[String],
        context: Option<&str>,
    ) -> Result<ErrorDiagnosis, LlmError>;

    fn evaluate_performance(
        &self,
        service_name: &str,
        metrics: &PerformanceMetrics,
    ) -> Result<PerformanceEvaluation, LlmError>;

    fn analyze_dependencies(
        &self,
        service_name: &str,
        graph: &ServiceDependencyGraph,
    ) -> Result<DependencyAnalysis, LlmError>;

    fn complete_prompt(&self, prompt: &str) -> Result<String, LlmError> {
        let _ = prompt;
        Err(LlmError::ConfigError {
            message: "LLM provider does not support prompt completion".to_string(),
        })
    }

    /// Stream a response from the LLM for a given prompt.
    /// Returns a stream of chunks (content or reasoning). Default implementation returns an empty stream.
    /// Providers should implement this for streaming support.
    /// When `messages` is provided, providers should use the messages array for multi-turn context.
    /// When `max_tokens` is None, providers should use a sensible default (e.g. 4096).
    fn stream_response(
        &self,
        _prompt: &str,
        _system_prompt: Option<&str>,
        _messages: Option<&[ChatMessage]>,
        _max_tokens: Option<u32>,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send + '_>> {
        Box::pin(futures_util::stream::empty())
    }

    /// Generate deployment plan based on scan results and context
    /// P0-2: 部署计划制定 - LLM 调用
    fn generate_deployment_plan(
        &self,
        context: &DeploymentPlanContext,
    ) -> Result<LLMDeploymentPlanResponse, LlmError> {
        // Default implementation uses complete_prompt
        // Note: build_deployment_plan_prompt and parse_llm_plan_response are defined later in this file
        let prompt = crate::llm::build_deployment_plan_prompt(context)?;
        let response = self.complete_prompt(&prompt)?;
        crate::llm::parse_llm_plan_response(&response)
    }

    /// Generate scan report analysis based on scan results
    /// 环境扫描报告分析 - LLM 调用
    fn generate_scan_report_analysis(
        &self,
        context: &ScanReportContext,
    ) -> Result<ScanReportAnalysis, LlmError> {
        // Default implementation uses complete_prompt
        let prompt = crate::llm::build_scan_report_prompt(context)?;
        let response = self.complete_prompt(&prompt)?;
        crate::llm::parse_scan_report_response(&response)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub cpu_usage_percent: Option<f64>,
    pub memory_usage_mb: Option<f64>,
    pub memory_limit_mb: Option<f64>,
    pub network_rx_bytes: Option<u64>,
    pub network_tx_bytes: Option<u64>,
    pub request_count: Option<u64>,
    pub error_rate: Option<f64>,
    pub response_time_ms: Option<f64>,
}

pub struct OpenAILlmProvider {
    config: LlmConfig,
    client: reqwest::Client,
}

impl OpenAILlmProvider {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    #[allow(dead_code)]
    async fn call_api(&self, prompt: &str) -> Result<String, LlmError> {
        self.call_api_with_retry(prompt, 3).await
    }

    async fn call_api_with_retry(
        &self,
        prompt: &str,
        max_retries: u32,
    ) -> Result<String, LlmError> {
        let url = "https://api.openai.com/v1/chat/completions";
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are an expert DevOps engineer helping with deployment planning."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.7,
            "max_tokens": 4096
        });

        let mut last_error = None;

        for attempt in 0..max_retries {
            let request = self
                .client
                .post(url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(30)); // 30秒超时

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if let Some(content) =
                                    json["choices"][0]["message"]["content"].as_str()
                                {
                                    return Ok(content.to_string());
                                } else {
                                    last_error = Some(LlmError::ParseError {
                                        message: "Invalid response format".to_string(),
                                    });
                                }
                            }
                            Err(e) => {
                                last_error = Some(LlmError::ParseError {
                                    message: e.to_string(),
                                });
                            }
                        }
                    } else {
                        let status = response.status();
                        // 对于 4xx 错误（客户端错误），不重试
                        if status.is_client_error() {
                            return Err(LlmError::ApiError {
                                message: format!(
                                    "API returned status: {} (client error, not retrying)",
                                    status
                                ),
                            });
                        }
                        // 对于 5xx 错误（服务器错误），重试
                        last_error = Some(LlmError::ApiError {
                            message: format!(
                                "API returned status: {} (attempt {}/{})",
                                status,
                                attempt + 1,
                                max_retries
                            ),
                        });
                    }
                }
                Err(e) => {
                    // 网络错误，重试
                    last_error = Some(LlmError::NetworkError {
                        message: format!(
                            "Network error (attempt {}/{}): {}",
                            attempt + 1,
                            max_retries,
                            e
                        ),
                    });
                }
            }

            // 如果不是最后一次尝试，等待后重试
            if attempt < max_retries - 1 {
                let delay = std::time::Duration::from_millis(500 * (attempt + 1) as u64); // 指数退避：500ms, 1000ms, 1500ms
                tokio::time::sleep(delay).await;
            }
        }

        Err(last_error.unwrap())
    }
}

impl LlmProvider for OpenAILlmProvider {
    fn analyze_services(
        &self,
        graph: &ServiceDependencyGraph,
    ) -> Result<ServiceAnalysis, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let service_names: Vec<String> = graph.nodes.iter().map(|s| s.name.clone()).collect();
        let prompt = format!(
            "Analyze the following microservices for deployment planning:\n\nServices: {}\n\nProvide analysis in JSON format with fields: performance_notes (array of strings), security_concerns (array of strings), deployment_order_suggestions (array of strings), resource_requirements (array of strings).",
            service_names.join(", ")
        );

        let response = run_async(self.call_api(&prompt))?;

        // Try to parse JSON response, fallback to simple text parsing
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response) {
            Ok(ServiceAnalysis {
                performance_notes: json["performance_notes"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                security_concerns: json["security_concerns"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                deployment_order_suggestions: json["deployment_order_suggestions"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                resource_requirements: json["resource_requirements"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        } else {
            // Fallback: parse simple text response
            Ok(ServiceAnalysis {
                performance_notes: vec![response.clone()],
                security_concerns: vec![],
                deployment_order_suggestions: vec![],
                resource_requirements: vec![],
            })
        }
    }

    fn generate_memo(&self, service_name: &str, action: &str) -> Result<String, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let prompt = format!(
            "Generate a deployment memo for service '{}' with action '{}'. The memo should be concise (2-3 sentences) and explain what this step does and why it's important.",
            service_name, action
        );

        run_async(self.call_api(&prompt))
    }

    fn assess_risk(&self, service_name: &str, action: &str) -> Result<RiskAssessment, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let prompt = format!(
            "Assess the risk level for deploying service '{}' with action '{}'. Respond in JSON format with fields: risk_level (one of: Low, Medium, High, Critical), concerns (array of strings), recommendations (array of strings).",
            service_name, action
        );

        let response = run_async(self.call_api(&prompt))?;

        // Try to parse JSON response
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response) {
            Ok(RiskAssessment {
                risk_level: json["risk_level"].as_str().unwrap_or("Medium").to_string(),
                concerns: json["concerns"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                recommendations: json["recommendations"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        } else {
            // Fallback: default risk assessment
            Ok(RiskAssessment {
                risk_level: "Medium".to_string(),
                concerns: vec![response],
                recommendations: vec![],
            })
        }
    }

    fn diagnose_error(
        &self,
        error_message: &str,
        error_logs: &[String],
        context: Option<&str>,
    ) -> Result<ErrorDiagnosis, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let logs_summary = if error_logs.len() > 20 {
            format!(
                "{} logs (showing last 20):\n{}",
                error_logs.len(),
                error_logs
                    .iter()
                    .rev()
                    .take(20)
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            error_logs.join("\n")
        };

        let context_str = context.unwrap_or("No additional context provided");

        let prompt = format!(
            r#"You are an expert DevOps engineer diagnosing a deployment error. Analyze the following error and provide a detailed diagnosis.

Error Message: {}
Context: {}
Error Logs:
{}

Please provide a comprehensive diagnosis in JSON format with the following fields:
- error_type: A brief classification of the error (e.g., "Connection Error", "Permission Denied", "Resource Exhaustion")
- root_cause: A detailed explanation of what likely caused this error
- severity: One of "Low", "Medium", "High", or "Critical"
- possible_causes: An array of possible root causes (at least 3 items)
- suggested_fixes: An array of specific, actionable fixes (at least 3 items)
- prevention_tips: An array of tips to prevent this error in the future (at least 2 items)
- fix_commands: An array of concrete shell commands to apply the fix safely (empty if unsafe/unknown)
- verification_steps: An array of steps or commands to verify the fix
- rollback_steps: An array of rollback steps or commands if the fix fails

Respond ONLY with valid JSON, no markdown formatting."#,
            error_message, context_str, logs_summary
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        match serde_json::from_str::<ErrorDiagnosis>(&response) {
            Ok(diagnosis) => Ok(diagnosis),
            Err(_) => {
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse error diagnosis: {}", e),
                    })
                } else {
                    Err(LlmError::ParseError {
                        message: "Failed to parse error diagnosis: no JSON object in LLM response".to_string(),
                    })
                }
            }
        }
    }

    fn evaluate_performance(
        &self,
        service_name: &str,
        metrics: &PerformanceMetrics,
    ) -> Result<PerformanceEvaluation, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let metrics_json = serde_json::to_string(metrics).unwrap_or_default();
        let prompt = format!(
            r#"You are an expert performance engineer analyzing a microservice. Evaluate the performance metrics and provide a comprehensive assessment.

Service Name: {}
Performance Metrics:
{}

Please provide a detailed performance evaluation in JSON format with the following fields:
- overall_score: A number between 0.0 and 100.0 representing overall performance
- cpu_usage_analysis: Analysis of CPU usage patterns
- memory_usage_analysis: Analysis of memory usage patterns
- network_analysis: Analysis of network traffic patterns
- bottlenecks: Array of identified performance bottlenecks
- optimization_suggestions: Array of specific optimization recommendations
- scalability_assessment: Assessment of service scalability
- resource_recommendations: Array of resource allocation recommendations

Respond ONLY with valid JSON, no markdown formatting."#,
            service_name, metrics_json
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        match serde_json::from_str::<PerformanceEvaluation>(&response) {
            Ok(evaluation) => Ok(evaluation),
            Err(_) => {
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse performance evaluation: {}", e),
                    })
                } else {
                    Err(LlmError::ParseError {
                        message: "Failed to parse performance evaluation: no JSON object in LLM response".to_string(),
                    })
                }
            }
        }
    }

    fn analyze_dependencies(
        &self,
        service_name: &str,
        graph: &ServiceDependencyGraph,
    ) -> Result<DependencyAnalysis, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let graph_json = serde_json::to_string(graph).unwrap_or_default();
        let prompt = format!(
            r#"You are an expert DevOps engineer analyzing service dependencies. Analyze the dependency graph and provide a comprehensive dependency analysis.

Service Name: {}
Dependency Graph:
{}

Please provide a detailed dependency analysis in JSON format with the following fields:
- dependencies: Array of objects with fields: service_name, dependency_type ("required"/"optional"/"weak"), impact_level ("critical"/"high"/"medium"/"low"), description
- dependents: Array of service names that depend on this service
- critical_path: Array of service names in the critical deployment path
- circular_risk: Boolean indicating if there's a risk of circular dependencies
- deployment_order: Recommended deployment order for this service and its dependencies
- recommendations: Array of dependency management recommendations

Respond ONLY with valid JSON, no markdown formatting."#,
            service_name, graph_json
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        match serde_json::from_str::<DependencyAnalysis>(&response) {
            Ok(analysis) => Ok(analysis),
            Err(_) => {
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse dependency analysis: {}", e),
                    })
                } else {
                    // Fallback: extract from graph
                    let dependencies: Vec<DependencyInfo> = graph
                        .edges
                        .iter()
                        .filter(|e| e.from == service_name)
                        .map(|e| DependencyInfo {
                            service_name: e.to.clone(),
                            dependency_type: "required".to_string(),
                            impact_level: "medium".to_string(),
                            description: String::new(),
                        })
                        .collect();

                    let dependents: Vec<String> = graph
                        .edges
                        .iter()
                        .filter(|e| e.to == service_name)
                        .map(|e| e.from.clone())
                        .collect();

                    Ok(DependencyAnalysis {
                        service_name: service_name.to_string(),
                        dependencies,
                        dependents,
                        critical_path: vec![],
                        circular_risk: false,
                        deployment_order: vec![],
                        recommendations: vec![],
                    })
                }
            }
        }
    }

    fn complete_prompt(&self, prompt: &str) -> Result<String, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }
        run_async(self.call_api_with_retry(prompt, 3))
    }

    fn stream_response(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        messages: Option<&[ChatMessage]>,
        max_tokens: Option<u32>,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send + '_>> {
        let client = self.client.clone();
        let api_key = self.config.api_key.clone();
        let model = self.config.model.clone();
        let base_url = self.config.base_url.clone();
        let prompt = prompt.to_string();
        let system_prompt = system_prompt.unwrap_or("").to_string();
        let messages = messages.map(|ms| ms.to_vec());
        let max_tokens = max_tokens.unwrap_or(4096);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            if api_key.is_empty() {
                let _ = tx.send(Err(LlmError::ConfigError {
                    message: "OpenAI API key is not configured".to_string(),
                }));
                return;
            }

            let url = base_url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .map(|u| {
                    let u = u.trim_end_matches('/');
                    if u.ends_with("/chat/completions") {
                        u.to_string()
                    } else {
                        format!("{u}/chat/completions")
                    }
                })
                .unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());

            let messages_arr = if let Some(ref msgs) = messages {
                let mut arr: Vec<serde_json::Value> = Vec::new();
                if !system_prompt.is_empty() {
                    arr.push(serde_json::json!({"role": "system", "content": system_prompt}));
                }
                for m in msgs {
                    arr.push(serde_json::json!({"role": m.role, "content": m.content}));
                }
                arr.push(serde_json::json!({"role": "user", "content": prompt}));
                arr
            } else {
                let mut arr: Vec<serde_json::Value> = Vec::new();
                if !system_prompt.is_empty() {
                    arr.push(serde_json::json!({"role": "system", "content": system_prompt}));
                }
                arr.push(serde_json::json!({"role": "user", "content": prompt}));
                arr
            };

            let body = serde_json::json!({
                "model": model,
                "messages": messages_arr,
                "temperature": 0.7,
                "max_tokens": max_tokens,
                "stream": true,
                "stream_options": { "include_usage": true }
            });

            let request = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", "application/json")
                .json(&body);

            match request.send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        let _ = tx.send(Err(LlmError::ApiError {
                            message: format!("API returned status: {}", response.status()),
                        }));
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();

                    use futures_util::StreamExt as _;
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(bytes) => {
                                let text = match String::from_utf8(bytes.to_vec()) {
                                    Ok(t) => t,
                                    Err(e) => {
                                        let _ = tx.send(Err(LlmError::ParseError {
                                            message: format!("Invalid UTF-8: {}", e),
                                        }));
                                        continue;
                                    }
                                };

                                buffer.push_str(&text);

                                // Process complete lines (SSE format: "data: {...}\n\n")
                                while let Some(newline_pos) = buffer.find("\n\n") {
                                    let line = buffer[..newline_pos].trim().to_string();
                                    buffer = buffer[newline_pos + 2..].to_string();

                                    if line.starts_with("data: ") {
                                        let json_str = &line[6..];

                                        // Check for [DONE] marker
                                        if json_str.trim() == "[DONE]" {
                                            return;
                                        }

                                        match serde_json::from_str::<serde_json::Value>(json_str) {
                                            Ok(json) => {
                                                if let Some(usage) = json.get("usage") {
                                                    let prompt_tokens = usage
                                                        .get("prompt_tokens")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(0) as u32;
                                                    let completion_tokens = usage
                                                        .get("completion_tokens")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(0) as u32;
                                                    if prompt_tokens > 0 || completion_tokens > 0 {
                                                        let _ = tx.send(Ok(StreamChunk::Usage {
                                                            prompt_tokens,
                                                            completion_tokens,
                                                        }));
                                                    }
                                                }
                                                if let Some(choices) =
                                                    json.get("choices").and_then(|c| c.as_array())
                                                {
                                                    if let Some(choice) = choices.first() {
                                                        if let Some(delta) = choice.get("delta") {
                                                            // 处理 reasoning_content / thinking（思考模式）
                                                            let reasoning_text = delta
                                                                .get("reasoning_content")
                                                                .and_then(|r| r.as_str())
                                                                .or_else(|| {
                                                                    delta.get("thinking").and_then(|t| t.as_str())
                                                                });
                                                            if let Some(reasoning) = reasoning_text {
                                                                if !reasoning.is_empty() {
                                                                    let _ = tx.send(Ok(
                                                                        StreamChunk::Reasoning(reasoning.to_string())
                                                                    ));
                                                                }
                                                            }
                                                            // 处理普通 content
                                                            if let Some(content) = delta
                                                                .get("content")
                                                                .and_then(|c| c.as_str())
                                                            {
                                                                if !content.is_empty() {
                                                                    let _ = tx.send(Ok(
                                                                        StreamChunk::Content(content.to_string())
                                                                    ));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            Err(_) => {
                                                // Skip invalid JSON lines (e.g., keep-alive messages)
                                                continue;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(LlmError::NetworkError {
                                    message: format!("Stream error: {}", e),
                                }));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(LlmError::NetworkError {
                        message: format!("Network error: {}", e),
                    }));
                }
            }
        });

        // Convert receiver to stream
        Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
    }
}

pub struct OllamaLlmProvider {
    config: LlmConfig,
    client: reqwest::Client,
}

impl OllamaLlmProvider {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    async fn call_api(&self, prompt: &str) -> Result<String, LlmError> {
        self.call_api_with_retry(prompt, 3).await
    }

    async fn call_api_with_retry(
        &self,
        prompt: &str,
        max_retries: u32,
    ) -> Result<String, LlmError> {
        let default_url = "http://localhost:11434".to_string();
        let base_url = self.config.base_url.as_ref().unwrap_or(&default_url);
        let url = format!("{}/api/generate", base_url);

        let body = serde_json::json!({
            "model": self.config.model,
            "prompt": prompt,
            "stream": false
        });

        let mut last_error = None;

        for attempt in 0..max_retries {
            let request = self
                .client
                .post(&url)
                .json(&body)
                .timeout(std::time::Duration::from_secs(60)); // 60秒超时（本地模型可能较慢）

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if let Some(content) = json["response"].as_str() {
                                    return Ok(content.to_string());
                                } else {
                                    last_error = Some(LlmError::ParseError {
                                        message: "Invalid response format".to_string(),
                                    });
                                }
                            }
                            Err(e) => {
                                last_error = Some(LlmError::ParseError {
                                    message: e.to_string(),
                                });
                            }
                        }
                    } else {
                        let status = response.status();
                        // 对于 4xx 错误，不重试
                        if status.is_client_error() {
                            return Err(LlmError::ApiError {
                                message: format!(
                                    "API returned status: {} (client error, not retrying)",
                                    status
                                ),
                            });
                        }
                        last_error = Some(LlmError::ApiError {
                            message: format!(
                                "API returned status: {} (attempt {}/{})",
                                status,
                                attempt + 1,
                                max_retries
                            ),
                        });
                    }
                }
                Err(e) => {
                    last_error = Some(LlmError::NetworkError {
                        message: format!(
                            "Network error (attempt {}/{}): {}",
                            attempt + 1,
                            max_retries,
                            e
                        ),
                    });
                }
            }

            // 如果不是最后一次尝试，等待后重试
            if attempt < max_retries - 1 {
                let delay = std::time::Duration::from_millis(500 * (attempt + 1) as u64);
                tokio::time::sleep(delay).await;
            }
        }

        Err(last_error.unwrap())
    }
}

impl LlmProvider for OllamaLlmProvider {
    fn analyze_services(
        &self,
        graph: &ServiceDependencyGraph,
    ) -> Result<ServiceAnalysis, LlmError> {
        let service_names: Vec<String> = graph.nodes.iter().map(|s| s.name.clone()).collect();
        let prompt = format!(
            "Analyze the following microservices for deployment planning:\n\nServices: {}\n\nProvide analysis in JSON format with fields: performance_notes (array of strings), security_concerns (array of strings), deployment_order_suggestions (array of strings), resource_requirements (array of strings).",
            service_names.join(", ")
        );

        match run_async(self.call_api(&prompt)) {
            Ok(response) => {
                // Try to parse JSON response
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response) {
                    Ok(ServiceAnalysis {
                        performance_notes: json["performance_notes"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        security_concerns: json["security_concerns"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        deployment_order_suggestions: json["deployment_order_suggestions"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        resource_requirements: json["resource_requirements"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                } else {
                    Ok(ServiceAnalysis {
                        performance_notes: vec![response],
                        security_concerns: vec![],
                        deployment_order_suggestions: vec![],
                        resource_requirements: vec![],
                    })
                }
            }
            Err(e) => {
                // If Ollama is not available, return a basic analysis
                Ok(ServiceAnalysis {
                    performance_notes: vec![format!(
                        "Ollama analysis failed: {}. Please ensure Ollama is running locally.",
                        e
                    )],
                    security_concerns: vec![],
                    deployment_order_suggestions: vec![],
                    resource_requirements: vec![],
                })
            }
        }
    }

    fn generate_memo(&self, service_name: &str, action: &str) -> Result<String, LlmError> {
        let prompt = format!(
            "Generate a deployment memo for service '{}' with action '{}'. The memo should be concise (2-3 sentences) and explain what this step does and why it's important.",
            service_name, action
        );

        match run_async(self.call_api(&prompt)) {
            Ok(response) => Ok(response),
            Err(_) => Ok(format!(
                "Deploy service {} with action {}",
                service_name, action
            )),
        }
    }

    fn assess_risk(&self, service_name: &str, action: &str) -> Result<RiskAssessment, LlmError> {
        let prompt = format!(
            "Assess the risk level for deploying service '{}' with action '{}'. Respond in JSON format with fields: risk_level (one of: Low, Medium, High, Critical), concerns (array of strings), recommendations (array of strings).",
            service_name, action
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        // Try to parse JSON response
        match serde_json::from_str::<RiskAssessment>(&response) {
            Ok(assessment) => Ok(assessment),
            Err(_) => {
                // Fallback: try to extract JSON from markdown code blocks
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse risk assessment: {}", e),
                    })
                } else {
                    Ok(RiskAssessment {
                        risk_level: "Medium".to_string(),
                        concerns: vec![],
                        recommendations: vec![],
                    })
                }
            }
        }
    }

    fn diagnose_error(
        &self,
        error_message: &str,
        error_logs: &[String],
        context: Option<&str>,
    ) -> Result<ErrorDiagnosis, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let logs_summary = if error_logs.len() > 20 {
            format!(
                "{} logs (showing last 20):\n{}",
                error_logs.len(),
                error_logs
                    .iter()
                    .rev()
                    .take(20)
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            error_logs.join("\n")
        };

        let context_str = context.unwrap_or("No additional context provided");

        let prompt = format!(
            r#"You are an expert DevOps engineer diagnosing a deployment error. Analyze the following error and provide a detailed diagnosis.

Error Message: {}
Context: {}
Error Logs:
{}

Please provide a comprehensive diagnosis in JSON format with the following fields:
- error_type: A brief classification of the error (e.g., "Connection Error", "Permission Denied", "Resource Exhaustion")
- root_cause: A detailed explanation of what likely caused this error
- severity: One of "Low", "Medium", "High", or "Critical"
- possible_causes: An array of possible root causes (at least 3 items)
- suggested_fixes: An array of specific, actionable fixes (at least 3 items)
- prevention_tips: An array of tips to prevent this error in the future (at least 2 items)
- fix_commands: An array of concrete shell commands to apply the fix safely (empty if unsafe/unknown)
- verification_steps: An array of steps or commands to verify the fix
- rollback_steps: An array of rollback steps or commands if the fix fails

Respond ONLY with valid JSON, no markdown formatting."#,
            error_message, context_str, logs_summary
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        // Try to parse JSON response
        match serde_json::from_str::<ErrorDiagnosis>(&response) {
            Ok(diagnosis) => Ok(diagnosis),
            Err(_) => {
                // Fallback: try to extract JSON from markdown code blocks
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse error diagnosis: {}", e),
                    })
                } else {
                    Err(LlmError::ParseError {
                        message: "Failed to parse error diagnosis: no JSON object in LLM response".to_string(),
                    })
                }
            }
        }
    }

    fn evaluate_performance(
        &self,
        service_name: &str,
        metrics: &PerformanceMetrics,
    ) -> Result<PerformanceEvaluation, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let metrics_json = serde_json::to_string(metrics).unwrap_or_default();
        let prompt = format!(
            r#"You are an expert performance engineer analyzing a microservice. Evaluate the performance metrics and provide a comprehensive assessment.

Service Name: {}
Performance Metrics:
{}

Please provide a detailed performance evaluation in JSON format with the following fields:
- overall_score: A number between 0.0 and 100.0 representing overall performance
- cpu_usage_analysis: Analysis of CPU usage patterns
- memory_usage_analysis: Analysis of memory usage patterns
- network_analysis: Analysis of network traffic patterns
- bottlenecks: Array of identified performance bottlenecks
- optimization_suggestions: Array of specific optimization recommendations
- scalability_assessment: Assessment of service scalability
- resource_recommendations: Array of resource allocation recommendations

Respond ONLY with valid JSON, no markdown formatting."#,
            service_name, metrics_json
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        match serde_json::from_str::<PerformanceEvaluation>(&response) {
            Ok(evaluation) => Ok(evaluation),
            Err(_) => {
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse performance evaluation: {}", e),
                    })
                } else {
                    Err(LlmError::ParseError {
                        message: "Failed to parse performance evaluation: no JSON object in LLM response".to_string(),
                    })
                }
            }
        }
    }

    fn analyze_dependencies(
        &self,
        service_name: &str,
        graph: &ServiceDependencyGraph,
    ) -> Result<DependencyAnalysis, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let graph_json = serde_json::to_string(graph).unwrap_or_default();
        let prompt = format!(
            r#"You are an expert DevOps engineer analyzing service dependencies. Analyze the dependency graph and provide a comprehensive dependency analysis.

Service Name: {}
Dependency Graph:
{}

Please provide a detailed dependency analysis in JSON format with the following fields:
- dependencies: Array of objects with fields: service_name, dependency_type ("required"/"optional"/"weak"), impact_level ("critical"/"high"/"medium"/"low"), description
- dependents: Array of service names that depend on this service
- critical_path: Array of service names in the critical deployment path
- circular_risk: Boolean indicating if there's a risk of circular dependencies
- deployment_order: Recommended deployment order for this service and its dependencies
- recommendations: Array of dependency management recommendations

Respond ONLY with valid JSON, no markdown formatting."#,
            service_name, graph_json
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        match serde_json::from_str::<DependencyAnalysis>(&response) {
            Ok(analysis) => Ok(analysis),
            Err(_) => {
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse dependency analysis: {}", e),
                    })
                } else {
                    // Fallback: extract from graph
                    let dependencies: Vec<DependencyInfo> = graph
                        .edges
                        .iter()
                        .filter(|e| e.from == service_name)
                        .map(|e| DependencyInfo {
                            service_name: e.to.clone(),
                            dependency_type: "required".to_string(),
                            impact_level: "medium".to_string(),
                            description: String::new(),
                        })
                        .collect();

                    let dependents: Vec<String> = graph
                        .edges
                        .iter()
                        .filter(|e| e.to == service_name)
                        .map(|e| e.from.clone())
                        .collect();

                    Ok(DependencyAnalysis {
                        service_name: service_name.to_string(),
                        dependencies,
                        dependents,
                        critical_path: vec![],
                        circular_risk: false,
                        deployment_order: vec![],
                        recommendations: vec![],
                    })
                }
            }
        }
    }

    fn complete_prompt(&self, prompt: &str) -> Result<String, LlmError> {
        run_async(self.call_api_with_retry(prompt, 3))
    }

    fn stream_response(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        messages: Option<&[ChatMessage]>,
        max_tokens: Option<u32>,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send + '_>> {
        let client = self.client.clone();
        let base_url = self
            .config
            .base_url
            .clone()
            .unwrap_or_else(|| "http://localhost:11434".to_string());
        let model = self.config.model.clone();
        let prompt = prompt.to_string();
        let system_prompt = system_prompt.unwrap_or("").to_string();
        let messages = messages.map(|ms| ms.to_vec());
        let num_predict = max_tokens.unwrap_or(4096);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            let url = format!("{}/api/chat", base_url);

            let messages_arr = if let Some(ref msgs) = messages {
                let mut arr: Vec<serde_json::Value> = Vec::new();
                if !system_prompt.is_empty() {
                    arr.push(serde_json::json!({"role": "system", "content": system_prompt}));
                }
                for m in msgs {
                    arr.push(serde_json::json!({"role": m.role, "content": m.content}));
                }
                arr.push(serde_json::json!({"role": "user", "content": prompt}));
                arr
            } else {
                let mut arr: Vec<serde_json::Value> = Vec::new();
                if !system_prompt.is_empty() {
                    arr.push(serde_json::json!({"role": "system", "content": system_prompt}));
                }
                arr.push(serde_json::json!({"role": "user", "content": prompt}));
                arr
            };

            let body = serde_json::json!({
                "model": model,
                "messages": messages_arr,
                "stream": true
            });

            let request = client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body);

            match request.send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        let _ = tx.send(Err(LlmError::ApiError {
                            message: format!("API returned status: {}", response.status()),
                        }));
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();

                    use futures_util::StreamExt as _;
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(bytes) => {
                                let text = match String::from_utf8(bytes.to_vec()) {
                                    Ok(t) => t,
                                    Err(e) => {
                                        let _ = tx.send(Err(LlmError::ParseError {
                                            message: format!("Invalid UTF-8: {}", e),
                                        }));
                                        continue;
                                    }
                                };

                                buffer.push_str(&text);

                                while let Some(newline_pos) = buffer.find('\n') {
                                    let line = buffer[..newline_pos].trim().to_string();
                                    buffer = buffer[newline_pos + 1..].to_string();

                                    if line.is_empty() {
                                        continue;
                                    }

                                    match serde_json::from_str::<serde_json::Value>(&line) {
                                        Ok(json) => {
                                            // Ollama /api/chat format: {"message": {"role": "assistant", "content": "...", "thinking": "..."}, "done": false}
                                            if let Some(message) = json.get("message") {
                                                // 处理 thinking（推理模式）
                                                if let Some(thinking) =
                                                    message.get("thinking").and_then(|t| t.as_str())
                                                {
                                                    if !thinking.is_empty() {
                                                        let _ = tx.send(Ok(StreamChunk::Reasoning(thinking.to_string())));
                                                    }
                                                }
                                                // 处理 content
                                                if let Some(content) =
                                                    message.get("content").and_then(|c| c.as_str())
                                                {
                                                    if !content.is_empty() {
                                                        let _ = tx.send(Ok(StreamChunk::Content(content.to_string())));
                                                    }
                                                }
                                            }

                                            // 兼容旧 /api/generate 格式（response 字段）
                                            if let Some(response) =
                                                json.get("response").and_then(|r| r.as_str())
                                            {
                                                if !response.is_empty() {
                                                    let _ = tx.send(Ok(StreamChunk::Content(response.to_string())));
                                                }
                                            }

                                            if json
                                                .get("done")
                                                .and_then(|d| d.as_bool())
                                                .unwrap_or(false)
                                            {
                                                return;
                                            }
                                        }
                                        Err(_) => {
                                            continue;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(LlmError::NetworkError {
                                    message: format!("Stream error: {}", e),
                                }));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(LlmError::NetworkError {
                        message: format!("Network error: {}", e),
                    }));
                }
            }
        });

        Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
    }
}

// Claude (Anthropic) Provider
pub struct ClaudeLlmProvider {
    config: LlmConfig,
    client: reqwest::Client,
}

impl ClaudeLlmProvider {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    async fn call_api(&self, prompt: &str) -> Result<String, LlmError> {
        self.call_api_with_retry(prompt, 3).await
    }

    async fn call_api_with_retry(
        &self,
        prompt: &str,
        max_retries: u32,
    ) -> Result<String, LlmError> {
        let url = "https://api.anthropic.com/v1/messages";
        let body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": 4096,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "system": "You are an expert DevOps engineer helping with deployment planning."
        });

        let mut last_error = None;

        for attempt in 0..max_retries {
            let request = self
                .client
                .post(url)
                .header("x-api-key", &self.config.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(30));

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if let Some(content) = json["content"]
                                    .as_array()
                                    .and_then(|arr| arr.first())
                                    .and_then(|item| item["text"].as_str())
                                {
                                    return Ok(content.to_string());
                                } else {
                                    last_error = Some(LlmError::ParseError {
                                        message: "Invalid response format".to_string(),
                                    });
                                }
                            }
                            Err(e) => {
                                last_error = Some(LlmError::ParseError {
                                    message: e.to_string(),
                                });
                            }
                        }
                    } else {
                        let status = response.status();
                        if status.is_client_error() {
                            return Err(LlmError::ApiError {
                                message: format!(
                                    "API returned status: {} (client error, not retrying)",
                                    status
                                ),
                            });
                        }
                        last_error = Some(LlmError::ApiError {
                            message: format!(
                                "API returned status: {} (attempt {}/{})",
                                status,
                                attempt + 1,
                                max_retries
                            ),
                        });
                    }
                }
                Err(e) => {
                    last_error = Some(LlmError::NetworkError {
                        message: format!(
                            "Network error (attempt {}/{}): {}",
                            attempt + 1,
                            max_retries,
                            e
                        ),
                    });
                }
            }

            if attempt < max_retries - 1 {
                let delay = std::time::Duration::from_millis(500 * (attempt + 1) as u64);
                tokio::time::sleep(delay).await;
            }
        }

        Err(last_error.unwrap())
    }
}

impl LlmProvider for ClaudeLlmProvider {
    fn analyze_services(
        &self,
        graph: &ServiceDependencyGraph,
    ) -> Result<ServiceAnalysis, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let service_names: Vec<String> = graph.nodes.iter().map(|s| s.name.clone()).collect();
        let prompt = format!(
            "Analyze the following microservices for deployment planning:\n\nServices: {}\n\nProvide analysis in JSON format with fields: performance_notes (array of strings), security_concerns (array of strings), deployment_order_suggestions (array of strings), resource_requirements (array of strings).",
            service_names.join(", ")
        );

        let response = run_async(self.call_api(&prompt))?;

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response) {
            Ok(ServiceAnalysis {
                performance_notes: json["performance_notes"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                security_concerns: json["security_concerns"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                deployment_order_suggestions: json["deployment_order_suggestions"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                resource_requirements: json["resource_requirements"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        } else {
            Ok(ServiceAnalysis {
                performance_notes: vec![response.clone()],
                security_concerns: vec![],
                deployment_order_suggestions: vec![],
                resource_requirements: vec![],
            })
        }
    }

    fn generate_memo(&self, service_name: &str, action: &str) -> Result<String, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let prompt = format!(
            "Generate a deployment memo for service '{}' with action '{}'. The memo should be concise (2-3 sentences) and explain what this step does and why it's important.",
            service_name, action
        );

        run_async(self.call_api(&prompt))
    }

    fn assess_risk(&self, service_name: &str, action: &str) -> Result<RiskAssessment, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let prompt = format!(
            "Assess the risk level for deploying service '{}' with action '{}'. Respond in JSON format with fields: risk_level (one of: Low, Medium, High, Critical), concerns (array of strings), recommendations (array of strings).",
            service_name, action
        );

        let response = run_async(self.call_api(&prompt))?;

        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response) {
            Ok(RiskAssessment {
                risk_level: json["risk_level"].as_str().unwrap_or("Medium").to_string(),
                concerns: json["concerns"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
                recommendations: json["recommendations"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        } else {
            Ok(RiskAssessment {
                risk_level: "Medium".to_string(),
                concerns: vec![response],
                recommendations: vec![],
            })
        }
    }

    fn diagnose_error(
        &self,
        error_message: &str,
        error_logs: &[String],
        context: Option<&str>,
    ) -> Result<ErrorDiagnosis, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let logs_summary = if error_logs.len() > 20 {
            format!(
                "{} logs (showing last 20):\n{}",
                error_logs.len(),
                error_logs
                    .iter()
                    .rev()
                    .take(20)
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            error_logs.join("\n")
        };

        let context_str = context.unwrap_or("No additional context provided");

        let prompt = format!(
            r#"You are an expert DevOps engineer diagnosing a deployment error. Analyze the following error and provide a detailed diagnosis.

Error Message: {}
Context: {}
Error Logs:
{}

Please provide a comprehensive diagnosis in JSON format with the following fields:
- error_type: A brief classification of the error (e.g., "Connection Error", "Permission Denied", "Resource Exhaustion")
- root_cause: A detailed explanation of what likely caused this error
- severity: One of "Low", "Medium", "High", or "Critical"
- possible_causes: An array of possible root causes (at least 3 items)
- suggested_fixes: An array of specific, actionable fixes (at least 3 items)
- prevention_tips: An array of tips to prevent this error in the future (at least 2 items)
- fix_commands: An array of concrete shell commands to apply the fix safely (empty if unsafe/unknown)
- verification_steps: An array of steps or commands to verify the fix
- rollback_steps: An array of rollback steps or commands if the fix fails

Respond ONLY with valid JSON, no markdown formatting."#,
            error_message, context_str, logs_summary
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        match serde_json::from_str::<ErrorDiagnosis>(&response) {
            Ok(diagnosis) => Ok(diagnosis),
            Err(_) => {
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse error diagnosis: {}", e),
                    })
                } else {
                    Err(LlmError::ParseError {
                        message: "Failed to parse error diagnosis: no JSON object in LLM response".to_string(),
                    })
                }
            }
        }
    }

    fn evaluate_performance(
        &self,
        service_name: &str,
        metrics: &PerformanceMetrics,
    ) -> Result<PerformanceEvaluation, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let metrics_json = serde_json::to_string(metrics).unwrap_or_default();
        let prompt = format!(
            r#"You are an expert performance engineer analyzing a microservice. Evaluate the performance metrics and provide a comprehensive assessment.

Service Name: {}
Performance Metrics:
{}

Please provide a detailed performance evaluation in JSON format with the following fields:
- overall_score: A number between 0.0 and 100.0 representing overall performance
- cpu_usage_analysis: Analysis of CPU usage patterns
- memory_usage_analysis: Analysis of memory usage patterns
- network_analysis: Analysis of network traffic patterns
- bottlenecks: Array of identified performance bottlenecks
- optimization_suggestions: Array of specific optimization recommendations
- scalability_assessment: Assessment of service scalability
- resource_recommendations: Array of resource allocation recommendations

Respond ONLY with valid JSON, no markdown formatting."#,
            service_name, metrics_json
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        match serde_json::from_str::<PerformanceEvaluation>(&response) {
            Ok(evaluation) => Ok(evaluation),
            Err(_) => {
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse performance evaluation: {}", e),
                    })
                } else {
                    Err(LlmError::ParseError {
                        message: "Failed to parse performance evaluation: no JSON object in LLM response".to_string(),
                    })
                }
            }
        }
    }

    fn analyze_dependencies(
        &self,
        service_name: &str,
        graph: &ServiceDependencyGraph,
    ) -> Result<DependencyAnalysis, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }

        let graph_json = serde_json::to_string(graph).unwrap_or_default();
        let prompt = format!(
            r#"You are an expert DevOps engineer analyzing service dependencies. Analyze the dependency graph and provide a comprehensive dependency analysis.

Service Name: {}
Dependency Graph:
{}

Please provide a detailed dependency analysis in JSON format with the following fields:
- dependencies: Array of objects with fields: service_name, dependency_type ("required"/"optional"/"weak"), impact_level ("critical"/"high"/"medium"/"low"), description
- dependents: Array of service names that depend on this service
- critical_path: Array of service names in the critical deployment path
- circular_risk: Boolean indicating if there's a risk of circular dependencies
- deployment_order: Recommended deployment order for this service and its dependencies
- recommendations: Array of dependency management recommendations

Respond ONLY with valid JSON, no markdown formatting."#,
            service_name, graph_json
        );

        let response = run_async(self.call_api_with_retry(&prompt, 3))?;

        match serde_json::from_str::<DependencyAnalysis>(&response) {
            Ok(analysis) => Ok(analysis),
            Err(_) => {
                let json_start = response.find('{');
                let json_end = response.rfind('}');
                if let (Some(start), Some(end)) = (json_start, json_end) {
                    let json_str = &response[start..=end];
                    serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                        message: format!("Failed to parse dependency analysis: {}", e),
                    })
                } else {
                    let dependencies: Vec<DependencyInfo> = graph
                        .edges
                        .iter()
                        .filter(|e| e.from == service_name)
                        .map(|e| DependencyInfo {
                            service_name: e.to.clone(),
                            dependency_type: "required".to_string(),
                            impact_level: "medium".to_string(),
                            description: String::new(),
                        })
                        .collect();

                    let dependents: Vec<String> = graph
                        .edges
                        .iter()
                        .filter(|e| e.to == service_name)
                        .map(|e| e.from.clone())
                        .collect();

                    Ok(DependencyAnalysis {
                        service_name: service_name.to_string(),
                        dependencies,
                        dependents,
                        critical_path: vec![],
                        circular_risk: false,
                        deployment_order: vec![],
                        recommendations: vec![],
                    })
                }
            }
        }
    }

    fn complete_prompt(&self, prompt: &str) -> Result<String, LlmError> {
        if self.config.api_key.is_empty() {
            return Err(LlmError::ConfigError {
                message: "LLM API key not configured".to_string(),
            });
        }
        run_async(self.call_api_with_retry(prompt, 3))
    }

    fn stream_response(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        messages: Option<&[ChatMessage]>,
        max_tokens: Option<u32>,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send + '_>> {
        let client = self.client.clone();
        let api_key = self.config.api_key.clone();
        let model = self.config.model.clone();
        let base_url = self.config.base_url.clone();
        let prompt = prompt.to_string();
        let system_prompt = system_prompt.unwrap_or("").to_string();
        let messages = messages.map(|ms| ms.to_vec());
        let max_tokens = max_tokens.unwrap_or(4096);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            if api_key.is_empty() {
                let _ = tx.send(Err(LlmError::ConfigError {
                    message: "Claude API key is not configured".to_string(),
                }));
                return;
            }

            let url = base_url
                .as_deref()
                .map(str::trim)
                .filter(|u| !u.is_empty())
                .unwrap_or("https://api.anthropic.com/v1/messages");

            let messages_arr = if let Some(ref msgs) = messages {
                let mut arr: Vec<serde_json::Value> = msgs
                    .iter()
                    .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
                    .collect();
                // 追加当前用户消息
                arr.push(serde_json::json!({"role": "user", "content": prompt}));
                arr
            } else {
                vec![serde_json::json!({"role": "user", "content": prompt})]
            };

            let mut body = serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "messages": messages_arr,
                "stream": true
            });
            if !system_prompt.is_empty() {
                body["system"] = serde_json::Value::String(system_prompt.clone());
            }

            let request = client
                .post(url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Content-Type", "application/json")
                .json(&body);

            match request.send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        let status = response.status();
                        let body = response.text().await.unwrap_or_default();
                        let _ = tx.send(Err(LlmError::ApiError {
                            message: format!("Claude API error: {} - {}", status, body),
                        }));
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();

                    use futures_util::StreamExt as _;
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(bytes) => {
                                let text = match String::from_utf8(bytes.to_vec()) {
                                    Ok(t) => t,
                                    Err(e) => {
                                        let _ = tx.send(Err(LlmError::ParseError {
                                            message: format!("Invalid UTF-8: {}", e),
                                        }));
                                        continue;
                                    }
                                };

                                buffer.push_str(&text);

                                while let Some(newline_pos) = buffer.find('\n') {
                                    let line = buffer[..newline_pos].trim().to_string();
                                    buffer = buffer[newline_pos + 1..].to_string();

                                    if line.is_empty() {
                                        continue;
                                    }

                                    if line.starts_with("data: ") {
                                        let data = &line[6..];
                                        if let Ok(json) =
                                            serde_json::from_str::<serde_json::Value>(data)
                                        {
                                            let event_type = json
                                                .get("type")
                                                .and_then(|t| t.as_str())
                                                .unwrap_or("");

                                            match event_type {
                                                "content_block_delta" => {
                                                    if let Some(delta) =
                                                        json.get("delta")
                                                    {
                                                        let delta_type = delta
                                                            .get("type")
                                                            .and_then(|t| t.as_str())
                                                            .unwrap_or("");

                                                        match delta_type {
                                                            "thinking_delta" => {
                                                                if let Some(thinking) =
                                                                    delta.get("thinking").and_then(|t| t.as_str())
                                                                {
                                                                    if !thinking.is_empty() {
                                                                        let _ = tx.send(Ok(
                                                                            StreamChunk::Reasoning(
                                                                                thinking.to_string(),
                                                                            ),
                                                                        ));
                                                                    }
                                                                }
                                                            }
                                                            "text_delta" => {
                                                                if let Some(text) =
                                                                    delta.get("text").and_then(|t| t.as_str())
                                                                {
                                                                    if !text.is_empty() {
                                                                        let _ = tx.send(Ok(
                                                                            StreamChunk::Content(
                                                                                text.to_string(),
                                                                            ),
                                                                        ));
                                                                    }
                                                                }
                                                            }
                                                            _ => {}
                                                        }
                                                    }
                                                }
                                                "message_stop" => {
                                                    return;
                                                }
                                                "error" => {
                                                    let err_msg = json
                                                        .get("error")
                                                        .and_then(|e| e.get("message"))
                                                        .and_then(|m| m.as_str())
                                                        .unwrap_or("Unknown error");
                                                    let _ = tx.send(Err(LlmError::ApiError {
                                                        message: err_msg.to_string(),
                                                    }));
                                                    return;
                                                }
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(LlmError::NetworkError {
                                    message: format!("Stream error: {}", e),
                                }));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(LlmError::NetworkError {
                        message: format!("Network error: {}", e),
                    }));
                }
            }
        });

        Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
    }
}

// Generic OpenAI-compatible provider for OpenRouter, DeepSeek, etc.
pub struct OpenAICompatibleProvider {
    config: LlmConfig,
    client: reqwest::Client,
    api_url: String,
}

impl OpenAICompatibleProvider {
    pub fn new(config: LlmConfig, api_url: String) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            api_url,
        }
    }

    async fn call_api(&self, prompt: &str) -> Result<String, LlmError> {
        self.call_api_with_retry(prompt, 3).await
    }

    async fn call_api_with_retry(
        &self,
        prompt: &str,
        max_retries: u32,
    ) -> Result<String, LlmError> {
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are an expert DevOps engineer helping with deployment planning."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.7,
            "max_tokens": 4096
        });

        let mut last_error = None;

        for attempt in 0..max_retries {
            let mut request = self
                .client
                .post(&self.api_url)
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(30));

            // Add authorization header
            if !self.config.api_key.is_empty() {
                request =
                    request.header("Authorization", format!("Bearer {}", self.config.api_key));
            }

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if let Some(content) =
                                    json["choices"][0]["message"]["content"].as_str()
                                {
                                    return Ok(content.to_string());
                                } else {
                                    last_error = Some(LlmError::ParseError {
                                        message: "Invalid response format".to_string(),
                                    });
                                }
                            }
                            Err(e) => {
                                last_error = Some(LlmError::ParseError {
                                    message: e.to_string(),
                                });
                            }
                        }
                    } else {
                        let status = response.status();
                        if status.is_client_error() {
                            return Err(LlmError::ApiError {
                                message: format!(
                                    "API returned status: {} (client error, not retrying)",
                                    status
                                ),
                            });
                        }
                        last_error = Some(LlmError::ApiError {
                            message: format!(
                                "API returned status: {} (attempt {}/{})",
                                status,
                                attempt + 1,
                                max_retries
                            ),
                        });
                    }
                }
                Err(e) => {
                    last_error = Some(LlmError::NetworkError {
                        message: format!(
                            "Network error (attempt {}/{}): {}",
                            attempt + 1,
                            max_retries,
                            e
                        ),
                    });
                }
            }

            if attempt < max_retries - 1 {
                let delay = std::time::Duration::from_millis(500 * (attempt + 1) as u64);
                tokio::time::sleep(delay).await;
            }
        }

        Err(last_error.unwrap())
    }
}

// Implement LlmProvider for OpenAICompatibleProvider using delegation
macro_rules! impl_llm_provider_for_openai_compatible {
    () => {
        fn analyze_services(
            &self,
            graph: &ServiceDependencyGraph,
        ) -> Result<ServiceAnalysis, LlmError> {
            if self.config.api_key.is_empty() {
                return Err(LlmError::ConfigError {
                    message: "LLM API key not configured".to_string(),
                });
            }

            let service_names: Vec<String> = graph.nodes.iter().map(|s| s.name.clone()).collect();
            let prompt = format!(
                "Analyze the following microservices for deployment planning:\n\nServices: {}\n\nProvide analysis in JSON format with fields: performance_notes (array of strings), security_concerns (array of strings), deployment_order_suggestions (array of strings), resource_requirements (array of strings).",
                service_names.join(", ")
            );

            let response = run_async(self.call_api(&prompt))?;

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response) {
                Ok(ServiceAnalysis {
                    performance_notes: json["performance_notes"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                    security_concerns: json["security_concerns"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                    deployment_order_suggestions: json["deployment_order_suggestions"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                    resource_requirements: json["resource_requirements"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                })
            } else {
                Ok(ServiceAnalysis {
                    performance_notes: vec![response.clone()],
                    security_concerns: vec![],
                    deployment_order_suggestions: vec![],
                    resource_requirements: vec![],
                })
            }
        }

        fn generate_memo(&self, service_name: &str, action: &str) -> Result<String, LlmError> {
            if self.config.api_key.is_empty() {
                return Err(LlmError::ConfigError {
                    message: "LLM API key not configured".to_string(),
                });
            }

            let prompt = format!(
                "Generate a deployment memo for service '{}' with action '{}'. The memo should be concise (2-3 sentences) and explain what this step does and why it's important.",
                service_name, action
            );

            run_async(self.call_api(&prompt))
        }

        fn assess_risk(&self, service_name: &str, action: &str) -> Result<RiskAssessment, LlmError> {
            if self.config.api_key.is_empty() {
                return Err(LlmError::ConfigError {
                    message: "LLM API key not configured".to_string(),
                });
            }

            let prompt = format!(
                "Assess the risk level for deploying service '{}' with action '{}'. Respond in JSON format with fields: risk_level (one of: Low, Medium, High, Critical), concerns (array of strings), recommendations (array of strings).",
                service_name, action
            );

            let response = run_async(self.call_api(&prompt))?;

            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&response) {
                Ok(RiskAssessment {
                    risk_level: json["risk_level"]
                        .as_str()
                        .unwrap_or("Medium")
                        .to_string(),
                    concerns: json["concerns"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                    recommendations: json["recommendations"]
                        .as_array()
                        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                        .unwrap_or_default(),
                })
            } else {
                Ok(RiskAssessment {
                    risk_level: "Medium".to_string(),
                    concerns: vec![response],
                    recommendations: vec![],
                })
            }
        }

        fn diagnose_error(
            &self,
            error_message: &str,
            error_logs: &[String],
            context: Option<&str>,
        ) -> Result<ErrorDiagnosis, LlmError> {
            if self.config.api_key.is_empty() {
                return Err(LlmError::ConfigError {
                    message: "LLM API key not configured".to_string(),
                });
            }

            let logs_summary = if error_logs.len() > 20 {
                format!("{} logs (showing last 20):\n{}", error_logs.len(), error_logs.iter().rev().take(20).map(|s| s.as_str()).collect::<Vec<_>>().join("\n"))
            } else {
                error_logs.join("\n")
            };

            let context_str = context.unwrap_or("No additional context provided");

            let prompt = format!(
                r#"You are an expert DevOps engineer diagnosing a deployment error. Analyze the following error and provide a detailed diagnosis.

Error Message: {}
Context: {}
Error Logs:
{}

Please provide a comprehensive diagnosis in JSON format with the following fields:
- error_type: A brief classification of the error (e.g., "Connection Error", "Permission Denied", "Resource Exhaustion")
- root_cause: A detailed explanation of what likely caused this error
- severity: One of "Low", "Medium", "High", or "Critical"
- possible_causes: An array of possible root causes (at least 3 items)
- suggested_fixes: An array of specific, actionable fixes (at least 3 items)
- prevention_tips: An array of tips to prevent this error in the future (at least 2 items)
- fix_commands: An array of concrete shell commands to apply the fix safely (empty if unsafe/unknown)
- verification_steps: An array of steps or commands to verify the fix
- rollback_steps: An array of rollback steps or commands if the fix fails

Respond ONLY with valid JSON, no markdown formatting."#,
                error_message, context_str, logs_summary
            );

            let response = run_async(self.call_api_with_retry(&prompt, 3))?;

            match serde_json::from_str::<ErrorDiagnosis>(&response) {
                Ok(diagnosis) => Ok(diagnosis),
                Err(_) => {
                    let json_start = response.find('{');
                    let json_end = response.rfind('}');
                    if let (Some(start), Some(end)) = (json_start, json_end) {
                        let json_str = &response[start..=end];
                        serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                            message: format!("Failed to parse error diagnosis: {}", e),
                        })
                    } else {
                        Err(LlmError::ParseError {
                            message: "Failed to parse error diagnosis: no JSON object in LLM response".to_string(),
                        })
                    }
                }
            }
        }

        fn evaluate_performance(
            &self,
            service_name: &str,
            metrics: &PerformanceMetrics,
        ) -> Result<PerformanceEvaluation, LlmError> {
            if self.config.api_key.is_empty() {
                return Err(LlmError::ConfigError {
                    message: "LLM API key not configured".to_string(),
                });
            }

            let metrics_json = serde_json::to_string(metrics).unwrap_or_default();
            let prompt = format!(
                r#"You are an expert performance engineer analyzing a microservice. Evaluate the performance metrics and provide a comprehensive assessment.

Service Name: {}
Performance Metrics:
{}

Please provide a detailed performance evaluation in JSON format with the following fields:
- overall_score: A number between 0.0 and 100.0 representing overall performance
- cpu_usage_analysis: Analysis of CPU usage patterns
- memory_usage_analysis: Analysis of memory usage patterns
- network_analysis: Analysis of network traffic patterns
- bottlenecks: Array of identified performance bottlenecks
- optimization_suggestions: Array of specific optimization recommendations
- scalability_assessment: Assessment of service scalability
- resource_recommendations: Array of resource allocation recommendations

Respond ONLY with valid JSON, no markdown formatting."#,
                service_name, metrics_json
            );

            let response = run_async(self.call_api_with_retry(&prompt, 3))?;

            match serde_json::from_str::<PerformanceEvaluation>(&response) {
                Ok(evaluation) => Ok(evaluation),
                Err(_) => {
                    let json_start = response.find('{');
                    let json_end = response.rfind('}');
                    if let (Some(start), Some(end)) = (json_start, json_end) {
                        let json_str = &response[start..=end];
                        serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                            message: format!("Failed to parse performance evaluation: {}", e),
                        })
                    } else {
                        Err(LlmError::ParseError {
                            message: "Failed to parse performance evaluation: no JSON object in LLM response".to_string(),
                        })
                    }
                }
            }
        }

        fn analyze_dependencies(
            &self,
            service_name: &str,
            graph: &ServiceDependencyGraph,
        ) -> Result<DependencyAnalysis, LlmError> {
            if self.config.api_key.is_empty() {
                return Err(LlmError::ConfigError {
                    message: "LLM API key not configured".to_string(),
                });
            }

            let graph_json = serde_json::to_string(graph).unwrap_or_default();
            let prompt = format!(
                r#"You are an expert DevOps engineer analyzing service dependencies. Analyze the dependency graph and provide a comprehensive dependency analysis.

Service Name: {}
Dependency Graph:
{}

Please provide a detailed dependency analysis in JSON format with the following fields:
- dependencies: Array of objects with fields: service_name, dependency_type ("required"/"optional"/"weak"), impact_level ("critical"/"high"/"medium"/"low"), description
- dependents: Array of service names that depend on this service
- critical_path: Array of service names in the critical deployment path
- circular_risk: Boolean indicating if there's a risk of circular dependencies
- deployment_order: Recommended deployment order for this service and its dependencies
- recommendations: Array of dependency management recommendations

Respond ONLY with valid JSON, no markdown formatting."#,
                service_name, graph_json
            );

            let response = run_async(self.call_api_with_retry(&prompt, 3))?;

            match serde_json::from_str::<DependencyAnalysis>(&response) {
                Ok(analysis) => Ok(analysis),
                Err(_) => {
                    let json_start = response.find('{');
                    let json_end = response.rfind('}');
                    if let (Some(start), Some(end)) = (json_start, json_end) {
                        let json_str = &response[start..=end];
                        serde_json::from_str(json_str).map_err(|e| LlmError::ParseError {
                            message: format!("Failed to parse dependency analysis: {}", e),
                        })
                    } else {
                        let dependencies: Vec<DependencyInfo> = graph
                            .edges
                            .iter()
                            .filter(|e| e.from == service_name)
                            .map(|e| DependencyInfo {
                                service_name: e.to.clone(),
                                dependency_type: "required".to_string(),
                                impact_level: "medium".to_string(),
                                description: String::new(),
                            })
                            .collect();

                        let dependents: Vec<String> = graph
                            .edges
                            .iter()
                            .filter(|e| e.to == service_name)
                            .map(|e| e.from.clone())
                            .collect();

                        Ok(DependencyAnalysis {
                            service_name: service_name.to_string(),
                            dependencies,
                            dependents,
                            critical_path: vec![],
                            circular_risk: false,
                            deployment_order: vec![],
                            recommendations: vec![],
                        })
                    }
                }
            }
        }

        fn complete_prompt(&self, prompt: &str) -> Result<String, LlmError> {
            if self.config.api_key.is_empty() {
                return Err(LlmError::ConfigError {
                    message: "LLM API key not configured".to_string(),
                });
            }
            run_async(self.call_api_with_retry(prompt, 3))
        }
    };
}

impl LlmProvider for OpenAICompatibleProvider {
    impl_llm_provider_for_openai_compatible!();

    fn stream_response(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        messages: Option<&[ChatMessage]>,
        max_tokens: Option<u32>,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamChunk, LlmError>> + Send + '_>> {
        let client = self.client.clone();
        let api_url = self.api_url.clone();
        let api_key = self.config.api_key.clone();
        let model = self.config.model.clone();
        let prompt = prompt.to_string();
        let system_prompt = system_prompt.unwrap_or("").to_string();
        let messages = messages.map(|ms| ms.to_vec());
        let max_tokens = max_tokens.unwrap_or(4096);

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            let is_local = api_url.contains("127.0.0.1") || api_url.contains("localhost");
            if api_key.trim().is_empty() && !is_local {
                let _ = tx.send(Err(LlmError::ConfigError {
                    message: "LLM API key is empty; refusing to stream without credentials".to_string(),
                }));
                return;
            }

            let messages_arr = if let Some(ref msgs) = messages {
                let mut arr: Vec<serde_json::Value> = Vec::new();
                if !system_prompt.is_empty() {
                    arr.push(serde_json::json!({"role": "system", "content": system_prompt}));
                }
                for m in msgs {
                    arr.push(serde_json::json!({"role": m.role, "content": m.content}));
                }
                arr.push(serde_json::json!({"role": "user", "content": prompt}));
                arr
            } else {
                let mut arr: Vec<serde_json::Value> = Vec::new();
                if !system_prompt.is_empty() {
                    arr.push(serde_json::json!({"role": "system", "content": system_prompt}));
                }
                arr.push(serde_json::json!({"role": "user", "content": prompt}));
                arr
            };

            let body = serde_json::json!({
                "model": model,
                "messages": messages_arr,
                "temperature": 0.7,
                "max_tokens": max_tokens,
                "stream": true,
                "stream_options": { "include_usage": true }
            });

            let mut request = client
                .post(&api_url)
                .header("Content-Type", "application/json")
                .json(&body);

            if !api_key.is_empty() {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }

            match request.send().await {
                Ok(response) => {
                    if !response.status().is_success() {
                        let _ = tx.send(Err(LlmError::ApiError {
                            message: format!("API returned status: {}", response.status()),
                        }));
                        return;
                    }

                    let mut stream = response.bytes_stream();
                    let mut buffer = String::new();

                    use futures_util::StreamExt as _;
                    while let Some(item) = stream.next().await {
                        match item {
                            Ok(bytes) => {
                                let text = match String::from_utf8(bytes.to_vec()) {
                                    Ok(t) => t,
                                    Err(e) => {
                                        let _ = tx.send(Err(LlmError::ParseError {
                                            message: format!("Invalid UTF-8: {}", e),
                                        }));
                                        continue;
                                    }
                                };

                                buffer.push_str(&text);

                                // Process complete lines (SSE format: "data: {...}\n\n")
                                while let Some(newline_pos) = buffer.find("\n\n") {
                                    let line = buffer[..newline_pos].trim().to_string();
                                    buffer = buffer[newline_pos + 2..].to_string();

                                    if line.starts_with("data: ") {
                                        let json_str = &line[6..];

                                        // Check for [DONE] marker
                                        if json_str.trim() == "[DONE]" {
                                            return;
                                        }

                                        match serde_json::from_str::<serde_json::Value>(json_str) {
                                            Ok(json) => {
                                                if let Some(usage) = json.get("usage") {
                                                    let prompt_tokens = usage
                                                        .get("prompt_tokens")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(0) as u32;
                                                    let completion_tokens = usage
                                                        .get("completion_tokens")
                                                        .and_then(|v| v.as_u64())
                                                        .unwrap_or(0) as u32;
                                                    if prompt_tokens > 0 || completion_tokens > 0 {
                                                        let _ = tx.send(Ok(StreamChunk::Usage {
                                                            prompt_tokens,
                                                            completion_tokens,
                                                        }));
                                                    }
                                                }
                                                if let Some(choices) =
                                                    json.get("choices").and_then(|c| c.as_array())
                                                {
                                                    if let Some(choice) = choices.first() {
                                                        if let Some(delta) = choice.get("delta") {
                                                            // 处理 reasoning_content / thinking（思考模式）
                                                            let reasoning_text = delta
                                                                .get("reasoning_content")
                                                                .and_then(|r| r.as_str())
                                                                .or_else(|| {
                                                                    delta.get("thinking").and_then(|t| t.as_str())
                                                                });
                                                            if let Some(reasoning) = reasoning_text {
                                                                if !reasoning.is_empty() {
                                                                    let _ = tx.send(Ok(
                                                                        StreamChunk::Reasoning(reasoning.to_string())
                                                                    ));
                                                                }
                                                            }
                                                            // 处理普通 content
                                                            if let Some(content) = delta
                                                                .get("content")
                                                                .and_then(|c| c.as_str())
                                                            {
                                                                if !content.is_empty() {
                                                                    let _ = tx.send(Ok(
                                                                        StreamChunk::Content(content.to_string())
                                                                    ));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                            Err(_) => {
                                                // Skip invalid JSON lines (e.g., keep-alive messages)
                                                continue;
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = tx.send(Err(LlmError::NetworkError {
                                    message: format!("Stream error: {}", e),
                                }));
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(LlmError::NetworkError {
                        message: format!("Network error: {}", e),
                    }));
                }
            }
        });

        // Convert receiver to stream
        Box::pin(tokio_stream::wrappers::UnboundedReceiverStream::new(rx))
    }
}

// Gemini Provider
pub struct GeminiLlmProvider {
    config: LlmConfig,
    client: reqwest::Client,
}

impl GeminiLlmProvider {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    async fn call_api(&self, prompt: &str) -> Result<String, LlmError> {
        self.call_api_with_retry(prompt, 3).await
    }

    async fn call_api_with_retry(
        &self,
        prompt: &str,
        max_retries: u32,
    ) -> Result<String, LlmError> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            self.config.model, self.config.api_key
        );

        let body = serde_json::json!({
            "contents": [{
                "parts": [{
                    "text": format!("You are an expert DevOps engineer helping with deployment planning.\n\n{}", prompt)
                }]
            }]
        });

        let mut last_error = None;

        for attempt in 0..max_retries {
            let request = self
                .client
                .post(&url)
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(30));

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if let Some(content) =
                                    json["candidates"][0]["content"]["parts"][0]["text"].as_str()
                                {
                                    return Ok(content.to_string());
                                } else {
                                    last_error = Some(LlmError::ParseError {
                                        message: "Invalid response format".to_string(),
                                    });
                                }
                            }
                            Err(e) => {
                                last_error = Some(LlmError::ParseError {
                                    message: e.to_string(),
                                });
                            }
                        }
                    } else {
                        let status = response.status();
                        if status.is_client_error() {
                            return Err(LlmError::ApiError {
                                message: format!(
                                    "API returned status: {} (client error, not retrying)",
                                    status
                                ),
                            });
                        }
                        last_error = Some(LlmError::ApiError {
                            message: format!(
                                "API returned status: {} (attempt {}/{})",
                                status,
                                attempt + 1,
                                max_retries
                            ),
                        });
                    }
                }
                Err(e) => {
                    last_error = Some(LlmError::NetworkError {
                        message: format!(
                            "Network error (attempt {}/{}): {}",
                            attempt + 1,
                            max_retries,
                            e
                        ),
                    });
                }
            }

            if attempt < max_retries - 1 {
                let delay = std::time::Duration::from_millis(500 * (attempt + 1) as u64);
                tokio::time::sleep(delay).await;
            }
        }

        Err(last_error.unwrap())
    }
}

impl LlmProvider for GeminiLlmProvider {
    impl_llm_provider_for_openai_compatible!();
}

// Alibaba Qwen Provider
pub struct AlibabaQwenLlmProvider {
    config: LlmConfig,
    client: reqwest::Client,
}

impl AlibabaQwenLlmProvider {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    async fn call_api(&self, prompt: &str) -> Result<String, LlmError> {
        self.call_api_with_retry(prompt, 3).await
    }

    async fn call_api_with_retry(
        &self,
        prompt: &str,
        max_retries: u32,
    ) -> Result<String, LlmError> {
        let default_url =
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation"
                .to_string();
        let base_url = self.config.base_url.as_ref().unwrap_or(&default_url);

        let body = serde_json::json!({
            "model": self.config.model,
            "input": {
                "messages": [
                    {
                        "role": "system",
                        "content": "You are an expert DevOps engineer helping with deployment planning."
                    },
                    {
                        "role": "user",
                        "content": prompt
                    }
                ]
            },
            "parameters": {
                "temperature": 0.7,
                "max_tokens": 4096
            }
        });

        let mut last_error = None;

        for attempt in 0..max_retries {
            let request = self
                .client
                .post(base_url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(30));

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if let Some(content) =
                                    json["output"]["choices"][0]["message"]["content"].as_str()
                                {
                                    return Ok(content.to_string());
                                } else {
                                    last_error = Some(LlmError::ParseError {
                                        message: "Invalid response format".to_string(),
                                    });
                                }
                            }
                            Err(e) => {
                                last_error = Some(LlmError::ParseError {
                                    message: e.to_string(),
                                });
                            }
                        }
                    } else {
                        let status = response.status();
                        if status.is_client_error() {
                            return Err(LlmError::ApiError {
                                message: format!(
                                    "API returned status: {} (client error, not retrying)",
                                    status
                                ),
                            });
                        }
                        last_error = Some(LlmError::ApiError {
                            message: format!(
                                "API returned status: {} (attempt {}/{})",
                                status,
                                attempt + 1,
                                max_retries
                            ),
                        });
                    }
                }
                Err(e) => {
                    last_error = Some(LlmError::NetworkError {
                        message: format!(
                            "Network error (attempt {}/{}): {}",
                            attempt + 1,
                            max_retries,
                            e
                        ),
                    });
                }
            }

            if attempt < max_retries - 1 {
                let delay = std::time::Duration::from_millis(500 * (attempt + 1) as u64);
                tokio::time::sleep(delay).await;
            }
        }

        Err(last_error.unwrap())
    }
}

impl LlmProvider for AlibabaQwenLlmProvider {
    impl_llm_provider_for_openai_compatible!();
}

// Zhipu GLM Provider
pub struct ZhipuLlmProvider {
    config: LlmConfig,
    client: reqwest::Client,
}

impl ZhipuLlmProvider {
    pub fn new(config: LlmConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    async fn call_api(&self, prompt: &str) -> Result<String, LlmError> {
        self.call_api_with_retry(prompt, 3).await
    }

    async fn call_api_with_retry(
        &self,
        prompt: &str,
        max_retries: u32,
    ) -> Result<String, LlmError> {
        // Zhipu uses JWT token authentication
        // For simplicity, we'll use API key in Authorization header
        let url = format!("https://open.bigmodel.cn/api/paas/v4/chat/completions");

        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are an expert DevOps engineer helping with deployment planning."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.7,
            "max_tokens": 4096
        });

        let mut last_error = None;

        for attempt in 0..max_retries {
            let request = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.config.api_key))
                .header("Content-Type", "application/json")
                .json(&body)
                .timeout(std::time::Duration::from_secs(30));

            match request.send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        match response.json::<serde_json::Value>().await {
                            Ok(json) => {
                                if let Some(content) =
                                    json["choices"][0]["message"]["content"].as_str()
                                {
                                    return Ok(content.to_string());
                                } else {
                                    last_error = Some(LlmError::ParseError {
                                        message: "Invalid response format".to_string(),
                                    });
                                }
                            }
                            Err(e) => {
                                last_error = Some(LlmError::ParseError {
                                    message: e.to_string(),
                                });
                            }
                        }
                    } else {
                        let status = response.status();
                        if status.is_client_error() {
                            return Err(LlmError::ApiError {
                                message: format!(
                                    "API returned status: {} (client error, not retrying)",
                                    status
                                ),
                            });
                        }
                        last_error = Some(LlmError::ApiError {
                            message: format!(
                                "API returned status: {} (attempt {}/{})",
                                status,
                                attempt + 1,
                                max_retries
                            ),
                        });
                    }
                }
                Err(e) => {
                    last_error = Some(LlmError::NetworkError {
                        message: format!(
                            "Network error (attempt {}/{}): {}",
                            attempt + 1,
                            max_retries,
                            e
                        ),
                    });
                }
            }

            if attempt < max_retries - 1 {
                let delay = std::time::Duration::from_millis(500 * (attempt + 1) as u64);
                tokio::time::sleep(delay).await;
            }
        }

        Err(last_error.unwrap())
    }
}

impl LlmProvider for ZhipuLlmProvider {
    impl_llm_provider_for_openai_compatible!();
}

pub fn create_llm_provider(config: LlmConfig) -> Result<Box<dyn LlmProvider>, LlmError> {
    match config.provider {
        LlmProviderType::OpenAI => Ok(Box::new(OpenAILlmProvider::new(config))),
        LlmProviderType::Ollama => Ok(Box::new(OllamaLlmProvider::new(config))),
        LlmProviderType::Claude => Ok(Box::new(ClaudeLlmProvider::new(config))),
        LlmProviderType::Gemini => {
            let api_url = config
                .base_url
                .clone()
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| {
                    "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions".to_string()
                });
            Ok(Box::new(OpenAICompatibleProvider::new(config, api_url)))
        }
        LlmProviderType::OpenRouter => {
            let api_url = config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://openrouter.ai/api/v1/chat/completions".to_string());
            Ok(Box::new(OpenAICompatibleProvider::new(config, api_url)))
        }
        LlmProviderType::AlibabaQwen => {
            let api_url = config
                .base_url
                .clone()
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| {
                    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions".to_string()
                });
            Ok(Box::new(OpenAICompatibleProvider::new(config, api_url)))
        }
        LlmProviderType::DeepSeek => {
            let api_url = config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.deepseek.com/v1/chat/completions".to_string());
            Ok(Box::new(OpenAICompatibleProvider::new(config, api_url)))
        }
        LlmProviderType::MinMAX => {
            let api_url = config.base_url.clone().unwrap_or_else(|| {
                "https://api.minimax.chat/v1/text/chatcompletion_v2".to_string()
            });
            Ok(Box::new(OpenAICompatibleProvider::new(config, api_url)))
        }
        LlmProviderType::Zhipu => {
            let api_url = config
                .base_url
                .clone()
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| {
                    "https://open.bigmodel.cn/api/paas/v4/chat/completions".to_string()
                });
            Ok(Box::new(OpenAICompatibleProvider::new(config, api_url)))
        }
        LlmProviderType::DashScope => {
            let api_url = config
                .base_url
                .clone()
                .unwrap_or_else(|| {
                    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions".to_string()
                });
            Ok(Box::new(OpenAICompatibleProvider::new(config, api_url)))
        }
        LlmProviderType::NvidiaNim => {
            let api_url = config.base_url.clone().unwrap_or_else(|| {
                "https://integrate.api.nvidia.com/v1/chat/completions".to_string()
            });
            Ok(Box::new(OpenAICompatibleProvider::new(config, api_url)))
        }
    }
}

// P0-2: 部署计划制定 - Prompt 构建和响应解析

/// 构建部署计划制定的 Prompt
pub fn build_deployment_plan_prompt(context: &DeploymentPlanContext) -> Result<String, LlmError> {
    let graph_json = serde_json::to_string_pretty(&context.dependency_graph).map_err(|e| {
        LlmError::ConfigError {
            message: format!("Failed to serialize dependency graph: {}", e),
        }
    })?;

    let prompt = format!(
        r#"
# 角色定义
你是一位资深的 DevOps/SRE 工程师，负责制定可靠的部署计划。

# 任务目标
基于提供的环境扫描结果和依赖关系图，制定一个安全、可靠的部署计划。

# 输入信息

## 1. 项目信息
- 项目名称: {}
- 项目ID: {}
- 目标主机: {} ({})
- 环境类型: {}

## 2. 代码同步状态
- 本地代码路径: {}
- 远程代码路径: {}
- 同步状态: {}
- 代码一致性: {}
- 最后同步时间: {}

## 3. 服务依赖图
```json
{}
```

## 4. 远程环境状态
- Docker 版本: {}
- Docker Compose 版本: {}
- 运行中的容器数: {}
- 可用的镜像数: {}
- 系统资源: CPU {}%, Memory {}%, Disk {}%

# 输出要求

请以 JSON 格式输出部署计划，包含以下字段：

{{
  "deployment_plan": {{
    "steps": [
      {{
        "id": "step_1",
        "service_name": "database",
        "action": "DeployService",
        "description": "部署数据库服务",
        "command": "docker-compose up -d database",
        "depends_on": [],
        "estimated_duration": "2分钟",
        "rollback_command": "docker-compose down database"
      }}
    ],
    "total_estimated_duration": "10分钟"
  }},
  "risk_assessment": {{
    "risk_level": "Medium",
    "concerns": [
      "数据库迁移可能导致数据丢失"
    ],
    "recommendations": [
      "建议先备份数据库"
    ]
  }},
  "dry_run_analysis": {{
    "simulated_steps": [],
    "potential_issues": [],
    "recommendations": []
  }},
  "validation_checklist": [
    "检查数据库备份是否完成",
    "验证网络连接是否正常"
  ]
}}

# 分析要求

1. **依赖关系分析**：识别服务之间的依赖关系，确定正确的部署顺序
2. **风险评估**：评估每个步骤的风险等级（Low/Medium/High/Critical）
3. **推演和测试**：模拟每个步骤的执行，预测可能的失败点
4. **优化建议**：建议并行执行的步骤、检查点、回滚策略

# 约束条件

1. 必须遵循服务依赖关系
2. 高风险操作必须标记为需要人工审批
3. 必须提供回滚方案
4. 必须考虑资源限制
5. 必须考虑最小化服务中断时间
6. `action` 字段必须是 `DeployService` 或 `VerifyService`（首字母大写，符合枚举值）

# 输出格式

请严格按照 JSON 格式输出，确保所有字段都包含在内。
"#,
        context.project_name,
        context.project_id,
        context.host_id,
        context.host_address,
        context.environment,
        context.local_repo_path,
        context.remote_repo_path,
        context.sync_status,
        context.code_consistency_status,
        context.last_sync_time,
        graph_json,
        context.remote_state.docker_version,
        context.remote_state.compose_version,
        context.remote_state.running_containers_count,
        context.remote_state.available_images_count,
        format_usage_or_unknown(context.remote_state.cpu_usage),
        format_usage_or_unknown(context.remote_state.memory_usage),
        format_usage_or_unknown(context.remote_state.disk_usage),
    );

    Ok(prompt)
}

fn format_usage_or_unknown(value: f64) -> String {
    if value < 0.0 {
        "unknown (metrics collection failed)".to_string()
    } else {
        format!("{:.1}", value)
    }
}

/// 构建环境扫描报告分析的 Prompt
pub fn build_scan_report_prompt(context: &ScanReportContext) -> Result<String, LlmError> {
    let graph_json = match &context.repo_graph {
        Some(graph) => serde_json::to_string_pretty(graph).map_err(|e| LlmError::ConfigError {
            message: format!("Failed to serialize dependency graph: {}", e),
        })?,
        None => "null".to_string(),
    };

    let images_json =
        serde_json::to_string_pretty(&context.docker_images).map_err(|e| LlmError::ConfigError {
            message: format!("Failed to serialize docker images: {}", e),
        })?;
    let containers_json =
        serde_json::to_string_pretty(&context.docker_containers).map_err(|e| {
            LlmError::ConfigError {
                message: format!("Failed to serialize docker containers: {}", e),
            }
        })?;
    let warnings_json =
        serde_json::to_string_pretty(&context.warnings).map_err(|e| LlmError::ConfigError {
            message: format!("Failed to serialize warnings: {}", e),
        })?;
    let alignment_json = serde_json::to_string_pretty(&context.alignment_suggestions).map_err(
        |e| LlmError::ConfigError {
            message: format!("Failed to serialize alignment suggestions: {}", e),
        },
    )?;

    let prompt = format!(
        r#"
# 角色定义
你是一位资深的 SRE/安全与平台工程师，负责输出“环境扫描报告”的高质量分析。

# 任务目标
基于扫描结果，输出一份结构化分析，覆盖环境评估、安全风险、资源利用、对齐状态与行动建议。

# 输入信息

## 1. 项目与主机信息
- 项目名称: {}
- 主机地址: {}
- 主机用户: {}
- 扫描时间: {}

## 2. 服务依赖图 (可能为空)
```json
{}
```

## 3. Docker 镜像摘要
```json
{}
```

## 4. Docker 容器摘要
```json
{}
```

## 5. 扫描告警 / 风险提示
```json
{}
```

## 6. 对齐与优化建议 (自动生成)
```json
{}
```

# 输出要求

请严格输出 JSON 格式，包含以下字段：

{{
  "executive_summary": "执行摘要（2-5句话，突出关键风险与总体健康度）",
  "environment_assessment": "环境评估（操作系统/运行态/基础组件）",
  "security_analysis": "安全分析（漏洞暴露、配置风险、权限与合规）",
  "resource_utilization": "资源利用分析（CPU/内存/磁盘/网络）",
  "alignment_analysis": "对齐状态分析（与最佳实践/标准的偏差）",
  "risk_assessment": ["风险点1", "风险点2"],
  "action_recommendations": ["建议1", "建议2"],
  "priority_actions": ["优先级最高的行动1", "行动2"]
}}

注意：只输出 JSON，不要 Markdown。
"#,
        context.project_name,
        context.host_address,
        context.host_user,
        context.scanned_at,
        graph_json,
        images_json,
        containers_json,
        warnings_json,
        alignment_json,
    );

    Ok(prompt)
}

/// 从 LLM 响应中提取 JSON
fn extract_json_from_response(response: &str) -> String {
    // 尝试提取 ```json ... ``` 代码块
    if let Some(start) = response.find("```json") {
        if let Some(end) = response[start..].find("```") {
            return response[start + 7..start + end].trim().to_string();
        }
    }

    // 尝试提取 ``` ... ``` 代码块（可能是 markdown 代码块）
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

/// 解析 LLM 返回的部署计划响应
pub fn parse_llm_plan_response(response: &str) -> Result<LLMDeploymentPlanResponse, LlmError> {
    let json_str = extract_json_from_response(response);

    let plan: LLMDeploymentPlanResponse =
        serde_json::from_str(&json_str).map_err(|e| LlmError::ConfigError {
            message: format!(
                "Failed to parse LLM response: {}. Response: {}",
                e, json_str
            ),
        })?;

    // 验证计划
    validate_llm_plan(&plan)?;

    Ok(plan)
}

/// 解析 LLM 返回的扫描报告分析
pub fn parse_scan_report_response(response: &str) -> Result<ScanReportAnalysis, LlmError> {
    let json_str = extract_json_from_response(response);

    match serde_json::from_str::<ScanReportAnalysis>(&json_str) {
        Ok(analysis) => Ok(analysis),
        Err(parse_err) => {
            // 尝试宽松解析，避免字段缺失导致整体失败
            let value: serde_json::Value = serde_json::from_str(&json_str).map_err(|_| {
                LlmError::ParseError {
                    message: format!(
                        "Failed to parse scan report analysis: {}. Response: {}",
                        parse_err, json_str
                    ),
                }
            })?;

            let get_string = |key: &str| {
                value
                    .get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string()
            };
            let get_vec = |key: &str| {
                value
                    .get(key)
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|item| item.as_str().map(|s| s.to_string()))
                            .collect::<Vec<String>>()
                    })
                    .unwrap_or_default()
            };

            let analysis = ScanReportAnalysis {
                executive_summary: get_string("executive_summary"),
                environment_assessment: get_string("environment_assessment"),
                security_analysis: get_string("security_analysis"),
                resource_utilization: get_string("resource_utilization"),
                alignment_analysis: get_string("alignment_analysis"),
                risk_assessment: get_vec("risk_assessment"),
                action_recommendations: get_vec("action_recommendations"),
                priority_actions: get_vec("priority_actions"),
            };

            let all_empty = analysis.executive_summary.is_empty()
                && analysis.environment_assessment.is_empty()
                && analysis.security_analysis.is_empty()
                && analysis.resource_utilization.is_empty()
                && analysis.alignment_analysis.is_empty()
                && analysis.risk_assessment.is_empty()
                && analysis.action_recommendations.is_empty()
                && analysis.priority_actions.is_empty();

            if all_empty {
                return Err(LlmError::ParseError {
                    message: format!(
                        "Failed to parse scan report analysis: {}. Response: {}",
                        parse_err, json_str
                    ),
                });
            }

            Ok(analysis)
        }
    }
}

/// 验证 LLM 生成的部署计划
fn validate_llm_plan(plan: &LLMDeploymentPlanResponse) -> Result<(), LlmError> {
    // 检查步骤是否为空
    if plan.deployment_plan.steps.is_empty() {
        return Err(LlmError::ConfigError {
            message: "Deployment plan has no steps".to_string(),
        });
    }

    // 检查步骤 ID 是否唯一
    let mut step_ids = std::collections::HashSet::new();
    for step in &plan.deployment_plan.steps {
        if step_ids.contains(&step.id) {
            return Err(LlmError::ConfigError {
                message: format!("Duplicate step ID: {}", step.id),
            });
        }
        step_ids.insert(step.id.clone());
    }

    // 检查依赖关系是否有效
    for step in &plan.deployment_plan.steps {
        for dep in &step.depends_on {
            if !step_ids.contains(dep) {
                return Err(LlmError::ConfigError {
                    message: format!("Step {} depends on unknown step: {}", step.id, dep),
                });
            }
        }
    }

    Ok(())
}
