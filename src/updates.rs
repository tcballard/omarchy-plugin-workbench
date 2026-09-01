use crate::model::{CheckResult, CheckSpec};
use crate::paths::AppPaths;
use crate::process::{command_exists, run_check};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_INSTALLED_PLUGINS: usize = 128;
const MAX_COMMITS: usize = 20;
const GIT_TIMEOUT_SECONDS: u64 = 120;
const UPDATE_TIMEOUT_SECONDS: u64 = 300;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateReport {
    pub ok: bool,
    pub available: usize,
    pub blocked: usize,
    pub plugins: Vec<PluginUpdate>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdate {
    pub id: String,
    pub state: String,
    pub updateable: bool,
    pub dirty: bool,
    pub ahead: u64,
    pub behind: u64,
    pub current_revision: Option<String>,
    pub remote_revision: Option<String>,
    pub commits: Vec<IncomingCommit>,
    pub commits_truncated: bool,
    pub diff_stat: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingCommit {
    pub revision: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyUpdateReport {
    pub ok: bool,
    pub action: String,
    pub updated: Vec<String>,
    pub failed: Vec<UpdateFailure>,
    pub skipped: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateFailure {
    pub id: String,
    pub error: String,
}

pub fn inspect(paths: &AppPaths, selected_id: Option<&str>) -> Result<UpdateReport> {
    if !command_exists("git") {
        bail!("git is required to inspect plugin updates");
    }
    let installed = installed_git_plugins(paths, selected_id)?;
    let mut plugins = Vec::with_capacity(installed.len());
    for (id, directory) in installed {
        plugins.push(inspect_one(paths, &id, &directory));
    }
    let available = plugins.iter().filter(|plugin| plugin.updateable).count();
    let blocked = plugins
        .iter()
        .filter(|plugin| {
            matches!(
                plugin.state.as_str(),
                "dirty" | "local-ahead" | "diverged" | "fetch-failed" | "inspect-failed"
            )
        })
        .count();
    Ok(UpdateReport {
        ok: true,
        available,
        blocked,
        plugins,
    })
}

pub fn apply_one(
    paths: &AppPaths,
    id: &str,
    reviewed_revision: &str,
    confirmed: bool,
) -> Result<ApplyUpdateReport> {
    require_confirmation(confirmed)?;
    validate_revision(reviewed_revision)?;
    let report = inspect(paths, Some(id))?;
    let plugin = report
        .plugins
        .first()
        .with_context(|| format!("plugin '{id}' is not a git-managed installation"))?;
    if !plugin.updateable {
        if plugin.state == "up-to-date" {
            return Ok(ApplyUpdateReport {
                ok: true,
                action: "update".to_owned(),
                updated: Vec::new(),
                failed: Vec::new(),
                skipped: vec![id.to_owned()],
                message: format!("{id} is already up to date"),
            });
        }
        bail!(
            "plugin '{id}' cannot be updated safely: {}{}",
            plugin.state,
            plugin
                .error
                .as_deref()
                .map(|error| format!(" ({error})"))
                .unwrap_or_default()
        );
    }
    if plugin.remote_revision.as_deref() != Some(reviewed_revision) {
        bail!("plugin '{id}' changed since review; run updates again before applying it");
    }
    apply_reviewed(paths, plugin, true)?;
    Ok(ApplyUpdateReport {
        ok: true,
        action: "update".to_owned(),
        updated: vec![id.to_owned()],
        failed: Vec::new(),
        skipped: Vec::new(),
        message: format!("Updated {id}; Omarchy validation passed"),
    })
}

pub fn apply_all(
    paths: &AppPaths,
    reviewed_values: &[String],
    confirmed: bool,
) -> Result<ApplyUpdateReport> {
    require_confirmation(confirmed)?;
    let reviewed = parse_reviewed(reviewed_values)?;
    if reviewed.is_empty() {
        bail!("no reviewed updates supplied; pass --reviewed ID=REVISION after review");
    }
    let inspection = inspect(paths, None)?;
    let mut updated = Vec::new();
    let mut failed = Vec::new();
    let mut skipped = Vec::new();
    for plugin in inspection.plugins {
        let Some(reviewed_revision) = reviewed.get(&plugin.id) else {
            continue;
        };
        if !plugin.updateable {
            skipped.push(plugin.id);
            continue;
        }
        if plugin.remote_revision.as_deref() != Some(reviewed_revision.as_str()) {
            failed.push(UpdateFailure {
                id: plugin.id,
                error: "remote revision changed since review".to_owned(),
            });
            continue;
        }
        match apply_reviewed(paths, &plugin, false) {
            Ok(()) => updated.push(plugin.id),
            Err(error) => failed.push(UpdateFailure {
                id: plugin.id,
                error: format!("{error:#}"),
            }),
        }
    }
    for id in reviewed.keys() {
        if !updated.contains(id)
            && !skipped.contains(id)
            && !failed.iter().any(|failure| &failure.id == id)
        {
            failed.push(UpdateFailure {
                id: id.clone(),
                error: "plugin is no longer a Git-managed installation".to_owned(),
            });
        }
    }
    if !updated.is_empty()
        && let Err(error) = rescan_shell(paths)
    {
        failed.push(UpdateFailure {
            id: "shell-rescan".to_owned(),
            error: format!("updates were validated but the shell rescan failed: {error:#}"),
        });
    }
    let ok = failed.is_empty();
    let message = if updated.is_empty() && failed.is_empty() {
        if skipped.is_empty() {
            "All Git-managed plugins are up to date".to_owned()
        } else {
            format!(
                "No safe updates applied; {} plugin(s) need attention",
                skipped.len()
            )
        }
    } else {
        format!(
            "Updated {} plugin(s); {} failed; {} skipped",
            updated.len(),
            failed.len(),
            skipped.len()
        )
    };
    Ok(ApplyUpdateReport {
        ok,
        action: "update-all".to_owned(),
        updated,
        failed,
        skipped,
        message,
    })
}

fn require_confirmation(confirmed: bool) -> Result<()> {
    if !confirmed {
        bail!("refusing to update without explicit confirmation; pass --yes after review");
    }
    Ok(())
}

fn parse_reviewed(values: &[String]) -> Result<BTreeMap<String, String>> {
    let mut reviewed = BTreeMap::new();
    for value in values {
        let (id, revision) = value
            .split_once('=')
            .with_context(|| format!("invalid reviewed update '{value}'; expected ID=REVISION"))?;
        if !valid_plugin_id(id) {
            bail!("invalid plugin id '{id}'");
        }
        validate_revision(revision)?;
        if reviewed
            .insert(id.to_owned(), revision.to_owned())
            .is_some()
        {
            bail!("plugin '{id}' was reviewed more than once");
        }
    }
    Ok(reviewed)
}

fn validate_revision(revision: &str) -> Result<()> {
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("reviewed revision must be a full 40-character Git object id");
    }
    Ok(())
}

fn installed_git_plugins(
    paths: &AppPaths,
    selected_id: Option<&str>,
) -> Result<Vec<(String, PathBuf)>> {
    let Some(selected_id) = selected_id else {
        if !paths.plugins_dir.exists() {
            return Ok(Vec::new());
        }
        let metadata = fs::symlink_metadata(&paths.plugins_dir)
            .context("inspect Omarchy plugins directory")?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("Omarchy plugins path is not a normal directory");
        }
        let mut plugins = Vec::new();
        let mut entries_seen = 0usize;
        for entry in fs::read_dir(&paths.plugins_dir).context("read Omarchy plugins directory")? {
            let entry = entry.context("read installed plugin entry")?;
            entries_seen += 1;
            if entries_seen > MAX_INSTALLED_PLUGINS {
                bail!(
                    "more than {MAX_INSTALLED_PLUGINS} installed plugins; narrow the request by id"
                );
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            if !valid_plugin_id(&id) {
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path())?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                continue;
            }
            let git = entry.path().join(".git");
            if fs::symlink_metadata(&git).is_ok_and(|metadata| metadata.is_dir()) {
                plugins.push((id, entry.path()));
            }
        }
        plugins.sort_by(|left, right| left.0.cmp(&right.0));
        return Ok(plugins);
    };

    if !valid_plugin_id(selected_id) {
        bail!("invalid plugin id '{selected_id}'");
    }
    let directory = paths.plugins_dir.join(selected_id);
    let metadata = fs::symlink_metadata(&directory)
        .with_context(|| format!("plugin '{selected_id}' is not installed"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("plugin '{selected_id}' is not a Git-managed installation");
    }
    if !fs::symlink_metadata(directory.join(".git")).is_ok_and(|metadata| metadata.is_dir()) {
        bail!("plugin '{selected_id}' is not a Git-managed installation");
    }
    Ok(vec![(selected_id.to_owned(), directory)])
}

fn inspect_one(paths: &AppPaths, id: &str, directory: &Path) -> PluginUpdate {
    match inspect_one_result(paths, id, directory) {
        Ok(plugin) => plugin,
        Err(error) => failed_update(id, "inspect-failed", format!("{error:#}")),
    }
}

fn inspect_one_result(paths: &AppPaths, id: &str, directory: &Path) -> Result<PluginUpdate> {
    let current_revision = git_stdout(paths, directory, &["rev-parse", "HEAD"])?;
    let dirty = !git_stdout(
        paths,
        directory,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?
    .is_empty();
    let fetch = run_git(paths, directory, &["fetch", "--quiet", "origin", "HEAD"])?;
    if !fetch.ok {
        return Ok(failed_update(id, "fetch-failed", check_output(&fetch)));
    }
    let remote_revision = git_stdout(paths, directory, &["rev-parse", "FETCH_HEAD"])?;
    let counts = git_stdout(
        paths,
        directory,
        &["rev-list", "--left-right", "--count", "HEAD...FETCH_HEAD"],
    )?;
    let mut fields = counts.split_whitespace();
    let ahead = fields
        .next()
        .context("Git did not report the local commit count")?
        .parse::<u64>()
        .context("parse local commit count")?;
    let behind = fields
        .next()
        .context("Git did not report the remote commit count")?
        .parse::<u64>()
        .context("parse remote commit count")?;
    if fields.next().is_some() {
        bail!("Git reported unexpected revision counts");
    }

    let state = if ahead > 0 && behind > 0 {
        "diverged"
    } else if ahead > 0 {
        "local-ahead"
    } else if behind > 0 && dirty {
        "dirty"
    } else if behind > 0 {
        "update-available"
    } else if dirty {
        "dirty"
    } else {
        "up-to-date"
    };
    let updateable = state == "update-available";
    let (commits, commits_truncated, diff_stat) = if behind > 0 {
        let log = git_stdout(
            paths,
            directory,
            &[
                "log",
                "--format=%h%x09%s",
                &format!("--max-count={}", MAX_COMMITS + 1),
                "HEAD..FETCH_HEAD",
            ],
        )?;
        let mut commits = log
            .lines()
            .filter_map(|line| {
                let (revision, subject) = line.split_once('\t')?;
                Some(IncomingCommit {
                    revision: revision.to_owned(),
                    subject: subject.to_owned(),
                })
            })
            .collect::<Vec<_>>();
        let commits_truncated = commits.len() > MAX_COMMITS;
        commits.truncate(MAX_COMMITS);
        let diff_stat = git_stdout(
            paths,
            directory,
            &["diff", "--stat", "--no-ext-diff", "HEAD", "FETCH_HEAD"],
        )?;
        (commits, commits_truncated, diff_stat)
    } else {
        (Vec::new(), false, String::new())
    };
    Ok(PluginUpdate {
        id: id.to_owned(),
        state: state.to_owned(),
        updateable,
        dirty,
        ahead,
        behind,
        current_revision: Some(current_revision),
        remote_revision: Some(remote_revision),
        commits,
        commits_truncated,
        diff_stat,
        error: None,
    })
}

fn failed_update(id: &str, state: &str, error: String) -> PluginUpdate {
    PluginUpdate {
        id: id.to_owned(),
        state: state.to_owned(),
        updateable: false,
        dirty: false,
        ahead: 0,
        behind: 0,
        current_revision: None,
        remote_revision: None,
        commits: Vec::new(),
        commits_truncated: false,
        diff_stat: String::new(),
        error: Some(error),
    }
}

fn valid_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && !id.contains("..")
        && id.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && matches!(character, '.' | '_' | '-'))
        })
}

fn run_git(paths: &AppPaths, directory: &Path, args: &[&str]) -> Result<CheckResult> {
    let mut argv = vec![
        "git".to_owned(),
        "-c".to_owned(),
        "credential.interactive=false".to_owned(),
        "-c".to_owned(),
        "core.hooksPath=/dev/null".to_owned(),
        "-c".to_owned(),
        "core.fsmonitor=false".to_owned(),
    ];
    argv.extend(args.iter().map(|argument| (*argument).to_owned()));
    run_check(
        &CheckSpec {
            name: "plugin-update-inspection".to_owned(),
            argv,
            timeout_seconds: GIT_TIMEOUT_SECONDS,
        },
        directory,
        &paths.state_dir.join("command-output"),
    )
}

fn git_stdout(paths: &AppPaths, directory: &Path, args: &[&str]) -> Result<String> {
    let result = run_git(paths, directory, args)?;
    if !result.ok {
        bail!("Git command failed: {}", check_output(&result));
    }
    Ok(result.stdout.trim().to_owned())
}

fn apply_reviewed(paths: &AppPaths, plugin: &PluginUpdate, rescan: bool) -> Result<()> {
    let directory = paths.plugins_dir.join(&plugin.id);
    let expected_current = plugin
        .current_revision
        .as_deref()
        .context("inspection did not include the current revision")?;
    let expected_remote = plugin
        .remote_revision
        .as_deref()
        .context("inspection did not include the remote revision")?;
    if git_stdout(paths, &directory, &["rev-parse", "HEAD"])? != expected_current {
        bail!("local revision changed since review");
    }
    if !git_stdout(
        paths,
        &directory,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?
    .is_empty()
    {
        bail!("working tree changed since review");
    }
    let merge = run_git(paths, &directory, &["merge", "--ff-only", expected_remote])?;
    if !merge.ok {
        bail!(
            "cannot fast-forward to the reviewed revision: {}",
            check_output(&merge)
        );
    }
    if git_stdout(paths, &directory, &["rev-parse", "HEAD"])? != expected_remote {
        let _ = run_git(paths, &directory, &["reset", "--hard", expected_current]);
        bail!("Git did not land on the reviewed revision; the update was rolled back");
    }
    if let Err(error) = validate_updated_plugin(paths, &directory) {
        let rollback = run_git(paths, &directory, &["reset", "--hard", expected_current])?;
        if !rollback.ok {
            bail!(
                "Omarchy validation failed ({error:#}) and rollback also failed: {}",
                check_output(&rollback)
            );
        }
        bail!("Omarchy validation failed; the update was rolled back: {error:#}");
    }
    if rescan {
        rescan_shell(paths)?;
    }
    Ok(())
}

fn validate_updated_plugin(paths: &AppPaths, directory: &Path) -> Result<()> {
    if !command_exists("omarchy") {
        bail!("omarchy is required to validate plugin updates");
    }
    let directory = directory.to_string_lossy().into_owned();
    let result = run_check(
        &CheckSpec {
            name: "validate-plugin-update".to_owned(),
            argv: vec![
                "omarchy".to_owned(),
                "plugin".to_owned(),
                "validate".to_owned(),
                directory,
            ],
            timeout_seconds: UPDATE_TIMEOUT_SECONDS,
        },
        &paths.plugins_dir,
        &paths.state_dir.join("command-output"),
    )?;
    if !result.ok {
        bail!("{}", check_output(&result));
    }
    Ok(())
}

fn rescan_shell(paths: &AppPaths) -> Result<()> {
    if !command_exists("omarchy-shell") {
        bail!("omarchy-shell is required to activate plugin updates");
    }
    let result = run_check(
        &CheckSpec {
            name: "rescan-plugin-updates".to_owned(),
            argv: vec![
                "omarchy-shell".to_owned(),
                "shell".to_owned(),
                "rescanPlugins".to_owned(),
            ],
            timeout_seconds: 60,
        },
        &paths.plugins_dir,
        &paths.state_dir.join("command-output"),
    )?;
    if !result.ok {
        bail!("{}", check_output(&result));
    }
    Ok(())
}

fn check_output(result: &CheckResult) -> String {
    let output = [result.stdout.trim(), result.stderr.trim()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if result.timed_out {
        format!(
            "command timed out{}",
            if output.is_empty() {
                String::new()
            } else {
                format!(": {output}")
            }
        )
    } else if output.is_empty() {
        format!("command exited with {:?}", result.exit_code)
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_id_matches_the_omarchy_update_boundary() {
        assert!(valid_plugin_id("io.example.plugin-1"));
        assert!(!valid_plugin_id("../plugin"));
        assert!(!valid_plugin_id("plugin/name"));
        assert!(!valid_plugin_id("-plugin"));
        assert!(!valid_plugin_id(""));
    }
}
