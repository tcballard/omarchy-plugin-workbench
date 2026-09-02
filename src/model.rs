use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const CONFIG_SCHEMA: u32 = 1;
pub const PROJECT_SCHEMA: u32 = 1;
pub const RECEIPT_SCHEMA: u32 = 1;
pub const OMARCHY_MANIFEST_SCHEMA: u32 = 1;
pub const OMARCHY_CONTRACT_REVISION: &str = "b686ed892d9c3020c3336203f6d34cc75b544e2b";
pub const BUILDER_REPOSITORY: &str = "https://github.com/tcballard/build-omarchy-plugins";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct CheckSpec {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct WorkflowSpec {
    pub name: String,
    pub capability: String,
    pub argv: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct EnvironmentSpec {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default = "default_required")]
    pub required: bool,
}

fn default_required() -> bool {
    true
}

fn default_timeout() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ProjectDefinition {
    pub schema_version: u32,
    #[serde(default)]
    pub plugin_path: Option<PathBuf>,
    #[serde(default)]
    pub checks: Vec<CheckSpec>,
    #[serde(default)]
    pub workflows: Vec<WorkflowSpec>,
    #[serde(default)]
    pub environment: Vec<EnvironmentSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub project_root: PathBuf,
    pub plugin_root: PathBuf,
    pub checks: Vec<CheckSpec>,
    #[serde(default)]
    pub workflows: Vec<WorkflowSpec>,
    #[serde(default)]
    pub environment: Vec<EnvironmentSpec>,
    pub project_checks_trusted: bool,
    #[serde(default)]
    pub trusted_definition_digest: Option<String>,
    #[serde(default)]
    pub definition_digest: Option<String>,
    #[serde(default)]
    pub approved_capabilities: Vec<String>,
    pub added_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryConfig {
    pub schema_version: u32,
    pub omarchy_contract_revision: String,
    pub manifest_schema: u32,
    pub projects: Vec<Project>,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA,
            omarchy_contract_revision: OMARCHY_CONTRACT_REVISION.to_owned(),
            manifest_schema: OMARCHY_MANIFEST_SCHEMA,
            projects: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentEntry {
    pub mode: DeploymentMode,
    pub target: PathBuf,
    pub revision: Option<String>,
    pub dirty: bool,
    pub deployed_at_unix: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentMode {
    LiveLink,
    Snapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeploymentReceipt {
    pub schema_version: u32,
    pub plugin_id: String,
    pub managed_target: PathBuf,
    pub active_index: usize,
    pub history: Vec<DeploymentEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationReport {
    pub ok: bool,
    pub plugin_id: String,
    pub plugin_name: String,
    pub plugin_version: String,
    pub kinds: Vec<String>,
    pub plugin_root: PathBuf,
    pub internal_validation: String,
    pub omarchy_validation: ToolResult,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
    pub available: bool,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub output: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitState {
    pub revision: Option<String>,
    pub dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatus {
    pub id: String,
    pub name: String,
    pub project_root: PathBuf,
    pub plugin_root: PathBuf,
    pub revision: Option<String>,
    pub dirty: bool,
    pub deployment: String,
    pub deployed_revision: Option<String>,
    pub enabled: Option<bool>,
    pub checks: usize,
    pub workflows: usize,
    pub environment_requirements: usize,
    pub active_sessions: usize,
    pub active_test_sessions: usize,
    pub project_checks_trusted: bool,
    pub definition_changed_since_trust: bool,
    pub security_review_status: String,
    pub security_review_revision: Option<String>,
    pub security_review_findings: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub ok: bool,
    pub project_id: String,
    pub results: Vec<CheckResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowReport {
    pub ok: bool,
    pub project_id: String,
    pub capability: String,
    pub result: CheckResult,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentReport {
    pub ok: bool,
    pub project_id: String,
    pub results: Vec<EnvironmentResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentResult {
    pub name: String,
    pub required: bool,
    pub argv: Vec<String>,
    pub result: ToolResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub task: String,
    pub agent: Option<String>,
    pub objective: String,
    pub branch: String,
    pub worktree: PathBuf,
    pub started_at_unix: u64,
    pub closed_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSessionRecord {
    pub schema_version: u32,
    pub id: String,
    pub project_id: String,
    pub root: PathBuf,
    pub compositor_pid: u32,
    pub compositor_start_ticks: u64,
    pub shell_pid: u32,
    pub shell_start_ticks: u64,
    pub hyprland_instance: String,
    pub wayland_display: String,
    pub started_at_unix: u64,
    pub live_source: bool,
    pub isolation: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestSessionStatus {
    #[serde(flatten)]
    pub session: TestSessionRecord,
    pub running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffRecord {
    pub schema_version: u32,
    pub session_id: String,
    pub project_id: String,
    pub objective: String,
    pub decisions: Vec<String>,
    pub blockers: Vec<String>,
    pub next_action: String,
    pub branch: String,
    pub worktree: PathBuf,
    pub revision: Option<String>,
    pub dirty: bool,
    pub recorded_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceRecord {
    pub schema_version: u32,
    pub project_id: String,
    pub kind: String,
    pub name: String,
    pub ok: bool,
    pub revision: Option<String>,
    pub dirty: bool,
    pub platform: String,
    pub recorded_at_unix: u64,
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseReadinessReport {
    pub ok: bool,
    pub project_id: String,
    pub version: String,
    pub revision: Option<String>,
    pub clean: bool,
    pub changelog_mentions_version: bool,
    pub current_revision_has_passing_checks: bool,
    pub current_revision_has_ready_security_review: bool,
    pub security_review_status: String,
    pub tag_exists: bool,
    pub active_sessions: usize,
    pub active_test_sessions: usize,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    pub name: String,
    pub argv: Vec<String>,
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_ms: u128,
    pub stdout: String,
    pub stderr: String,
    pub output_truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionReport {
    pub ok: bool,
    pub action: String,
    pub project_id: String,
    pub message: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub ok: bool,
    pub expected_omarchy_revision: String,
    pub manifest_schema: u32,
    pub architecture: String,
    pub config_file: PathBuf,
    pub state_directory: PathBuf,
    pub plugins_directory: PathBuf,
    pub tools: std::collections::BTreeMap<String, ToolResult>,
    pub builder_companion: BuilderCompanionReport,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderCompanionReport {
    pub detected: bool,
    pub repository: String,
    pub supported_project_schema: u32,
    pub installations: Vec<BuilderInstallation>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuilderInstallation {
    pub target: String,
    pub version: String,
    pub receipt: PathBuf,
}
