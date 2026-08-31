use crate::deploy::{git_state, load_receipt_for, validate};
use crate::model::{
    ActionReport, CheckReport, CheckResult, CheckSpec, DeploymentMode, DoctorReport,
    OMARCHY_CONTRACT_REVISION, OMARCHY_MANIFEST_SCHEMA, Project, ProjectStatus, ToolResult,
};
use crate::paths::AppPaths;
use crate::process::{capture_tool, command_exists, run_check};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::fs;

pub fn project_statuses(paths: &AppPaths, projects: &[Project]) -> Result<Vec<ProjectStatus>> {
    let enabled = enabled_plugins();
    projects
        .iter()
        .map(|project| project_status(paths, project, &enabled))
        .collect()
}

fn project_status(
    paths: &AppPaths,
    project: &Project,
    enabled: &HashMap<String, bool>,
) -> Result<ProjectStatus> {
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
        project_checks_trusted: project.project_checks_trusted,
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
    if !checks.is_empty() && !project.project_checks_trusted {
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
    Ok(CheckReport {
        ok,
        project_id: project.id.clone(),
        results,
    })
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
    }
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
