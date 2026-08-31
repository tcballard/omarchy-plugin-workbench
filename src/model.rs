use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const CONFIG_SCHEMA: u32 = 1;
pub const PROJECT_SCHEMA: u32 = 1;
pub const RECEIPT_SCHEMA: u32 = 1;
pub const OMARCHY_MANIFEST_SCHEMA: u32 = 1;
pub const OMARCHY_CONTRACT_REVISION: &str = "b686ed892d9c3020c3336203f6d34cc75b544e2b";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CheckSpec {
    pub name: String,
    pub argv: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

fn default_timeout() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDefinition {
    pub schema_version: u32,
    #[serde(default)]
    pub plugin_path: Option<PathBuf>,
    #[serde(default)]
    pub checks: Vec<CheckSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub project_root: PathBuf,
    pub plugin_root: PathBuf,
    pub checks: Vec<CheckSpec>,
    pub project_checks_trusted: bool,
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
    pub project_checks_trusted: bool,
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
}
