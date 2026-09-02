use crate::deploy::{git_state, load_receipt_for, validate};
use crate::model::{
    ActionReport, BUILDER_REPOSITORY, BuilderCompanionReport, BuilderInstallation, CheckReport,
    CheckResult, CheckSpec, DeploymentMode, DoctorReport, EnvironmentReport, EnvironmentResult,
    OMARCHY_CONTRACT_REVISION, OMARCHY_MANIFEST_SCHEMA, PROJECT_SCHEMA, Project, ProjectStatus,
    ReleaseReadinessReport, ToolResult, WorkflowReport,
};
use crate::paths::AppPaths;
use crate::process::{capture_tool, command_exists, run_check};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

pub fn project_statuses(paths: &AppPaths, projects: &[Project]) -> Result<Vec<ProjectStatus>> {
    let enabled = enabled_plugins();
    projects
        .iter()
        .map(|project| project_status(paths, project, &enabled))
        .collect()
}

pub fn project_status(
    paths: &AppPaths,
    project: &Project,
    enabled: &HashMap<String, bool>,
) -> Result<ProjectStatus> {
    let security = crate::security::status(paths, project)?;
    let git = git_state(&project.project_root);
    let receipt = load_receipt_for(paths, &project.id)?;
    let installed_target = paths.plugins_dir.join(&project.id);
    let (deployment, deployed_revision) = if installed_target.is_symlink() {
        let actual = fs::read_link(&installed_target)?;
        if let Some(receipt) = &receipt {
            let active = receipt.history.get(receipt.active_index);
            if active.is_some_and(|entry| entry.target == actual) {
                let entry = active.expect("checked above");
                (
                    match entry.mode {
                        DeploymentMode::LiveLink => "live-link",
                        DeploymentMode::Snapshot => "snapshot",
                    }
                    .to_owned(),
                    entry.revision.clone(),
                )
            } else {
                ("drifted".to_owned(), None)
            }
        } else {
            ("unmanaged-link".to_owned(), None)
        }
    } else if installed_target.exists() {
        ("unmanaged-install".to_owned(), None)
    } else {
        ("not-deployed".to_owned(), None)
    };
    Ok(ProjectStatus {
        id: project.id.clone(),
        name: project.name.clone(),
        project_root: project.project_root.clone(),
        plugin_root: project.plugin_root.clone(),
        revision: git.revision,
        dirty: git.dirty,
        deployment,
        deployed_revision,
        enabled: enabled.get(&project.id).copied(),
        checks: project.checks.len(),
        workflows: project.workflows.len(),
        environment_requirements: project.environment.len(),
        active_sessions: crate::coordination::active_session_count(paths, &project.id)?,
        active_test_sessions: crate::test_session::active_count(paths, &project.id)?,
        project_checks_trusted: project.project_checks_trusted,
        definition_changed_since_trust: project.project_checks_trusted
            && !crate::registry::definition_is_trusted(project)?,
        security_review_status: security.status,
        security_review_revision: security.reviewed_revision,
        security_review_findings: security.findings,
    })
}

fn enabled_plugins() -> HashMap<String, bool> {
    let result = capture_tool("omarchy", &["plugin", "list", "--json"], None);
    if !result.available || !result.ok {
        return HashMap::new();
    }
    let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&result.output) else {
        return HashMap::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| {
            let id = entry.get("id")?.as_str()?.to_owned();
            let enabled = entry.get("enabled")?.as_bool()?;
            Some((id, enabled))
        })
        .collect()
}

pub fn run_project_checks(
    paths: &AppPaths,
    project: &Project,
    selected: Option<&str>,
) -> Result<CheckReport> {
    validate(project)?;
    let checks = selected_checks(project, selected)?;
    if !checks.is_empty() && !crate::registry::definition_is_trusted(project)? {
        bail!(
            "project checks are not trusted; review .omarchy-workbench.json and run: omarchy-plugin-workbench trust {}",
            project.id
        );
    }
    let mut results = vec![CheckResult {
        name: "omarchy-plugin-validate".to_owned(),
        argv: vec!["omarchy plugin validate".to_owned()],
        ok: true,
        exit_code: Some(0),
        timed_out: false,
        duration_ms: 0,
        stdout: "manifest and plugin tree passed validation".to_owned(),
        stderr: String::new(),
        output_truncated: false,
    }];
    let output_dir = paths.state_dir.join("check-output");
    for check in checks {
        let result = run_check(check, &project.project_root, &output_dir)?;
        let stop = !result.ok;
        results.push(result);
        if stop {
            break;
        }
    }
    let ok = results.iter().all(|result| result.ok);
    let report = CheckReport {
        ok,
        project_id: project.id.clone(),
        results,
    };
    let evidence = crate::coordination::evidence_record(
        project,
        "check",
        selected.unwrap_or("all"),
        report.ok,
        serde_json::to_value(&report)?,
    );
    crate::coordination::append_evidence(paths, &evidence)?;
    Ok(report)
}

pub fn run_workflow(paths: &AppPaths, project: &Project, name: &str) -> Result<WorkflowReport> {
    if !crate::registry::definition_is_trusted(project)? {
        bail!(
            "project definition is not trusted or changed since trust; review it and run: omarchy-plugin-workbench trust {}",
            project.id
        );
    }
    let workflow = project
        .workflows
        .iter()
        .find(|item| item.name == name)
        .with_context(|| format!("project '{}' has no workflow named '{name}'", project.id))?;
    let mut capabilities = workflow.requires.clone();
    capabilities.push(workflow.capability.clone());
    let missing = capabilities
        .iter()
        .filter(|item| !project.approved_capabilities.contains(item))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!(
            "workflow '{}' requires local capability approval: {}; approve each with: omarchy-plugin-workbench approve {} CAPABILITY",
            workflow.name,
            missing.join(", "),
            project.id
        );
    }
    let check = CheckSpec {
        name: workflow.name.clone(),
        argv: workflow.argv.clone(),
        timeout_seconds: workflow.timeout_seconds,
    };
    let result = run_check(
        &check,
        &project.project_root,
        &paths.state_dir.join("workflow-output"),
    )?;
    let report = WorkflowReport {
        ok: result.ok,
        project_id: project.id.clone(),
        capability: workflow.capability.clone(),
        result,
    };
    let evidence = crate::coordination::evidence_record(
        project,
        "workflow",
        name,
        report.ok,
        serde_json::to_value(&report)?,
    );
    crate::coordination::append_evidence(paths, &evidence)?;
    Ok(report)
}

pub fn inspect_environment(paths: &AppPaths, project: &Project) -> Result<EnvironmentReport> {
    if !project.environment.is_empty() && !crate::registry::definition_is_trusted(project)? {
        bail!(
            "project definition is not trusted or changed since trust; environment probes will not run"
        );
    }
    let results = project
        .environment
        .iter()
        .map(|requirement| {
            let executable = requirement
                .argv
                .first()
                .expect("validated environment argv");
            let args = requirement.argv[1..]
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            EnvironmentResult {
                name: requirement.name.clone(),
                required: requirement.required,
                argv: requirement.argv.clone(),
                result: capture_tool(executable, &args, Some(&project.project_root)),
            }
        })
        .collect::<Vec<_>>();
    let ok = results.iter().all(|item| !item.required || item.result.ok);
    let report = EnvironmentReport {
        ok,
        project_id: project.id.clone(),
        results,
    };
    let evidence = crate::coordination::evidence_record(
        project,
        "environment",
        "requirements",
        report.ok,
        serde_json::to_value(&report)?,
    );
    crate::coordination::append_evidence(paths, &evidence)?;
    Ok(report)
}

pub fn release_readiness(paths: &AppPaths, project: &Project) -> Result<ReleaseReadinessReport> {
    let validation = validate(project)?;
    let git = git_state(&project.project_root);
    let changelog = ["CHANGELOG.md", "Changelog.md", "changelog.md"]
        .iter()
        .map(|name| project.project_root.join(name))
        .find(|path| path.is_file());
    let changelog_mentions_version = changelog
        .and_then(|path| fs::read_to_string(path).ok())
        .is_some_and(|text| text.contains(&validation.plugin_version));
    let tag = format!("v{}", validation.plugin_version);
    let tag_result = capture_tool(
        "git",
        &[
            "-C",
            &project.project_root.to_string_lossy(),
            "tag",
            "--list",
            &tag,
        ],
        None,
    );
    let tag_exists = tag_result.ok && tag_result.output.lines().any(|line| line == tag);
    let passing = crate::coordination::has_passing_checks(paths, project, git.revision.as_deref())?;
    let security = crate::security::status(paths, project)?;
    let active_sessions = crate::coordination::active_session_count(paths, &project.id)?;
    let active_test_sessions = crate::test_session::active_count(paths, &project.id)?;
    let mut blockers = Vec::new();
    if git.revision.is_none() {
        blockers.push("project is not at a Git commit".to_owned());
    }
    if git.dirty {
        blockers.push("working tree is dirty".to_owned());
    }
    if !changelog_mentions_version {
        blockers.push("changelog does not mention the manifest version".to_owned());
    }
    if !passing {
        blockers.push("no clean passing check evidence exists for the current revision".to_owned());
    }
    if !security.ready {
        blockers.push(match security.status.as_str() {
            "stale" => "manual security review is stale for the current source".to_owned(),
            "needs-fixes" => "manual security review still has blocking findings".to_owned(),
            "incomplete" => "manual security review is incomplete".to_owned(),
            _ => "no current Ready manual security review exists".to_owned(),
        });
    }
    if active_sessions > 0 {
        blockers.push("one or more work sessions are still active".to_owned());
    }
    if active_test_sessions > 0 {
        blockers.push("one or more nested test sessions are still active".to_owned());
    }
    let report = ReleaseReadinessReport {
        ok: blockers.is_empty(),
        project_id: project.id.clone(),
        version: validation.plugin_version,
        revision: git.revision,
        clean: !git.dirty,
        changelog_mentions_version,
        current_revision_has_passing_checks: passing,
        current_revision_has_ready_security_review: security.ready,
        security_review_status: security.status,
        tag_exists,
        active_sessions,
        active_test_sessions,
        blockers,
    };
    let evidence = crate::coordination::evidence_record(
        project,
        "release-readiness",
        &report.version,
        report.ok,
        serde_json::to_value(&report)?,
    );
    crate::coordination::append_evidence(paths, &evidence)?;
    Ok(report)
}

fn selected_checks<'a>(project: &'a Project, selected: Option<&str>) -> Result<Vec<&'a CheckSpec>> {
    if let Some(name) = selected {
        let check = project
            .checks
            .iter()
            .find(|check| check.name == name)
            .with_context(|| format!("project '{}' has no check named '{name}'", project.id))?;
        Ok(vec![check])
    } else {
        Ok(project.checks.iter().collect())
    }
}

pub fn enable_project(project: &Project) -> Result<ActionReport> {
    require_command("omarchy")?;
    let result = capture_tool("omarchy", &["plugin", "enable", &project.id], None);
    if !result.ok {
        bail!("could not enable '{}': {}", project.id, result.output);
    }
    Ok(ActionReport {
        ok: true,
        action: "enable".to_owned(),
        project_id: project.id.clone(),
        message: result.output,
        warnings: Vec::new(),
    })
}

pub fn disable_project(project: &Project) -> Result<ActionReport> {
    require_command("omarchy")?;
    let result = capture_tool("omarchy", &["plugin", "disable", &project.id], None);
    if !result.ok {
        bail!("could not disable '{}': {}", project.id, result.output);
    }
    Ok(ActionReport {
        ok: true,
        action: "disable".to_owned(),
        project_id: project.id.clone(),
        message: result.output,
        warnings: Vec::new(),
    })
}

pub fn logs(project: &Project, lines: usize) -> Result<ToolResult> {
    if lines == 0 || lines > 1000 {
        bail!("log line count must be between 1 and 1000");
    }
    let omarchy_path =
        std::env::var("OMARCHY_PATH").unwrap_or_else(|_| "/usr/share/omarchy".to_owned());
    let lines_arg = lines.to_string();
    let mut result = capture_tool(
        "qs",
        &[
            "log",
            "-p",
            &format!("{omarchy_path}/shell"),
            "--tail",
            &lines_arg,
        ],
        None,
    );
    if result.ok {
        let needle = project.id.to_ascii_lowercase();
        let filtered = result
            .output
            .lines()
            .filter(|line| line.to_ascii_lowercase().contains(&needle))
            .collect::<Vec<_>>()
            .join("\n");
        result.output = if filtered.is_empty() {
            format!("no recent shell log lines mention {}", project.id)
        } else {
            filtered
        };
    }
    Ok(result)
}

pub fn doctor(paths: &AppPaths) -> DoctorReport {
    let mut tools = BTreeMap::new();
    for (name, args) in [
        ("git", vec!["--version"]),
        ("omarchy", vec!["--version"]),
        ("omarchy-shell", vec!["shell", "ping"]),
        ("qmllint", vec!["--version"]),
        ("qs", vec!["--version"]),
        ("Hyprland", vec!["--version"]),
        ("hyprctl", vec!["version"]),
        ("quickshell", vec!["--version"]),
    ] {
        tools.insert(name.to_owned(), capture_tool(name, &args, None));
    }
    let required_ok = ["git", "omarchy", "omarchy-shell"].iter().all(|name| {
        tools
            .get(*name)
            .is_some_and(|result| result.available && result.ok)
    });
    DoctorReport {
        ok: required_ok,
        expected_omarchy_revision: OMARCHY_CONTRACT_REVISION.to_owned(),
        manifest_schema: OMARCHY_MANIFEST_SCHEMA,
        architecture: std::env::consts::ARCH.to_owned(),
        config_file: paths.config_file.clone(),
        state_directory: paths.state_dir.clone(),
        plugins_directory: paths.plugins_dir.clone(),
        tools,
        builder_companion: detect_builder_companion(paths),
    }
}

const BUILDER_RECEIPT: &str = ".build-omarchy-plugins-receipt.json";
const MAX_BUILDER_RECEIPT_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BuilderReceipt {
    schema_version: u32,
    manager: String,
    source: BuilderSource,
    skills: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct BuilderSource {
    repository: String,
    version: String,
}

fn detect_builder_companion(paths: &AppPaths) -> BuilderCompanionReport {
    let candidates = [
        ("agents/codex", paths.home_dir.join(".agents/skills")),
        ("cursor", paths.home_dir.join(".cursor/skills")),
        ("gemini", paths.home_dir.join(".gemini/skills")),
        ("claude", paths.home_dir.join(".claude/skills")),
        ("opencode", paths.home_dir.join(".config/opencode/skills")),
    ];
    let mut installations = Vec::new();
    let mut issues = Vec::new();
    for (target, directory) in candidates {
        let receipt_path = directory.join(BUILDER_RECEIPT);
        if !receipt_path.exists() && !receipt_path.is_symlink() {
            continue;
        }
        match read_builder_receipt(&receipt_path) {
            Ok(receipt) => installations.push(BuilderInstallation {
                target: target.to_owned(),
                version: receipt.source.version,
                receipt: receipt_path,
            }),
            Err(error) => issues.push(format!("{}: {error}", receipt_path.display())),
        }
    }
    BuilderCompanionReport {
        detected: !installations.is_empty(),
        repository: BUILDER_REPOSITORY.to_owned(),
        supported_project_schema: PROJECT_SCHEMA,
        installations,
        issues,
    }
}

fn read_builder_receipt(path: &Path) -> Result<BuilderReceipt> {
    let inspected = fs::symlink_metadata(path)
        .with_context(|| format!("inspect builder receipt {}", path.display()))?;
    if inspected.file_type().is_symlink() || !inspected.is_file() {
        bail!("receipt is not a regular file");
    }
    if inspected.len() > MAX_BUILDER_RECEIPT_BYTES {
        bail!("receipt exceeds {MAX_BUILDER_RECEIPT_BYTES} bytes");
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("open builder receipt {}", path.display()))?;
    let opened = file
        .metadata()
        .with_context(|| format!("inspect opened builder receipt {}", path.display()))?;
    if !opened.is_file() || (inspected.dev(), inspected.ino()) != (opened.dev(), opened.ino()) {
        bail!("receipt changed while being opened");
    }
    let mut bytes = Vec::with_capacity(inspected.len() as usize);
    file.by_ref()
        .take(MAX_BUILDER_RECEIPT_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("read builder receipt")?;
    if bytes.len() as u64 > MAX_BUILDER_RECEIPT_BYTES {
        bail!("receipt exceeds {MAX_BUILDER_RECEIPT_BYTES} bytes");
    }
    let receipt: BuilderReceipt =
        serde_json::from_slice(&bytes).context("parse builder receipt")?;
    if receipt.schema_version != 1
        || receipt.manager != "build-omarchy-plugins"
        || receipt.source.repository != BUILDER_REPOSITORY
        || receipt.source.version.trim().is_empty()
        || !receipt.skills.contains_key("omarchy-plugin-scaffold")
    {
        bail!("receipt does not describe a supported Build Omarchy Plugins installation");
    }
    Ok(receipt)
}

fn require_command(name: &str) -> Result<()> {
    if !command_exists(name) {
        bail!("required command is unavailable: {name}");
    }
    Ok(())
}

pub fn managed_deployment_exists(paths: &AppPaths, project: &Project) -> Result<bool> {
    let receipt = load_receipt_for(paths, &project.id)?;
    let target = paths.plugins_dir.join(&project.id);
    Ok(receipt.is_some() && (target.exists() || target.is_symlink()))
}
