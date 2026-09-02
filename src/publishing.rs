use crate::manifest;
use crate::model::CheckSpec;
use crate::model::Project;
use crate::paths::{AppPaths, secure_dir};
use crate::process::run_check;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const MARKETPLACE_FORM: &str =
    "https://github.com/omacom/omarchy-plugin-marketplace/issues/new?template=submit-plugin.yml";
const MAX_DRAFT_BYTES: usize = 128 * 1024;
const CATEGORIES: &[&str] = &[
    "Appearance",
    "Desktop",
    "Developer Tools",
    "Hardware",
    "Kids",
    "Productivity",
    "System",
    "Widgets",
    "Other",
];
const TAGS: &[&str] = &[
    "ai",
    "bar",
    "education",
    "games",
    "hyprland",
    "kids",
    "launcher",
    "media",
    "power-management",
    "quickshell",
    "security",
    "system",
    "workspaces",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleasePlan {
    pub ok: bool,
    pub project_id: String,
    pub version: String,
    pub tag: String,
    pub revision: Option<String>,
    pub repository: Option<String>,
    pub readiness: crate::model::ReleaseReadinessReport,
    pub exact_commands: Vec<Vec<String>>,
    pub plan_file: PathBuf,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionDraft {
    pub ok: bool,
    pub project_id: String,
    pub title: String,
    pub repository: String,
    pub category: String,
    pub tags: Vec<String>,
    pub collision: Option<String>,
    pub form_url: &'static str,
    pub draft_file: PathBuf,
    pub body: String,
    pub blockers: Vec<String>,
    pub security_review_status: String,
    pub security_review_revision: Option<String>,
}

pub fn release_plan(paths: &AppPaths, project: &Project) -> Result<ReleasePlan> {
    let readiness = crate::workbench::release_readiness(paths, project)?;
    let repository = git_origin(paths, project)
        .ok()
        .and_then(normalize_github_origin);
    let tag = format!("v{}", readiness.version);
    let mut exact_commands = Vec::new();
    if let Some(revision) = &readiness.revision {
        exact_commands.push(vec![
            "git".to_owned(),
            "-C".to_owned(),
            project.project_root.display().to_string(),
            "tag".to_owned(),
            "--annotate".to_owned(),
            tag.clone(),
            revision.clone(),
            "--message".to_owned(),
            format!("Release {tag}"),
        ]);
        exact_commands.push(vec![
            "git".to_owned(),
            "-C".to_owned(),
            project.project_root.display().to_string(),
            "push".to_owned(),
            "origin".to_owned(),
            format!("refs/tags/{tag}:refs/tags/{tag}"),
        ]);
        if let Some(repository) = &repository {
            exact_commands.push(vec![
                "gh".to_owned(),
                "release".to_owned(),
                "create".to_owned(),
                tag.clone(),
                "--repo".to_owned(),
                repository
                    .trim_start_matches("https://github.com/")
                    .to_owned(),
                "--verify-tag".to_owned(),
                "--generate-notes".to_owned(),
            ]);
        }
    }
    secure_dir(&paths.publishing_dir)?;
    let plan_file = paths
        .publishing_dir
        .join(format!("{}-{tag}-release-plan.json", project.id));
    let plan = ReleasePlan {
        ok: readiness.ok && repository.is_some() && !readiness.tag_exists,
        project_id: project.id.clone(),
        version: readiness.version.clone(),
        tag,
        revision: readiness.revision.clone(),
        repository,
        readiness,
        exact_commands,
        plan_file: plan_file.clone(),
    };
    write_json(&plan_file, &plan)?;
    Ok(plan)
}

#[allow(clippy::too_many_arguments)]
pub fn submission_draft(
    paths: &AppPaths,
    project: &Project,
    repository: &str,
    category: &str,
    tags: &[String],
    suggested_tag: Option<&str>,
    notes: Option<&str>,
    checklist_confirmed: bool,
) -> Result<SubmissionDraft> {
    validate_github_repository(repository)?;
    if !CATEGORIES.contains(&category) {
        bail!("unsupported marketplace category '{category}'");
    }
    if tags.is_empty() || tags.len() > 3 {
        bail!("marketplace submission requires one to three tags");
    }
    let mut unique_tags = Vec::new();
    for tag in tags {
        let normalized = tag.to_ascii_lowercase();
        if !TAGS.contains(&normalized.as_str()) {
            bail!("unsupported marketplace tag '{tag}'");
        }
        if !unique_tags.contains(&normalized) {
            unique_tags.push(normalized);
        }
    }
    if unique_tags.len() != tags.len() {
        bail!("marketplace submission tags must be unique");
    }
    if suggested_tag.is_some_and(|value| value.trim().is_empty() || value.len() > 64) {
        bail!("suggested tag must be 1-64 characters");
    }
    if notes.is_some_and(|value| value.len() > 4096) {
        bail!("maintainer notes exceed 4096 characters");
    }
    let manifest = manifest::validate_plugin(&project.plugin_root)?;
    let mut blockers = Vec::new();
    let security = crate::security::status(paths, project)?;
    if project.plugin_root != project.project_root {
        blockers.push(
            "marketplace submissions require manifest.json in the repository root".to_owned(),
        );
    }
    if !has_named_file(&project.project_root, &["README.md", "README", "readme.md"]) {
        blockers.push("repository root has no README".to_owned());
    }
    if !has_named_file(
        &project.project_root,
        &["LICENSE", "LICENSE.md", "COPYING", "COPYING.md"],
    ) {
        blockers.push("repository root has no license file".to_owned());
    }
    if !checklist_confirmed {
        blockers.push("submission checklist has not been explicitly confirmed".to_owned());
    }
    if !security.ready {
        blockers.push(match security.status.as_str() {
            "stale" => "manual security review is stale for the current source".to_owned(),
            "needs-fixes" => "manual security review still has blocking findings".to_owned(),
            "incomplete" => "manual security review is incomplete".to_owned(),
            _ => "no current Ready manual security review exists".to_owned(),
        });
    }
    let collision = crate::marketplace::submission_collision(paths, &manifest.id, repository)?;
    if let Some(collision) = &collision {
        blockers.push(collision.clone());
    }
    let body = format!(
        "### Repository URL\n\n{repository}\n\n### Category\n\n{category}\n\n### Tags\n\n{}\n\n### Suggest a missing tag\n\n{}\n\n### Maintainer notes\n\n{}\n\n### Submission checklist\n\n- [x] The repository is public and contains installation and removal instructions.\n- [x] I have documented the plugin license and any external dependencies.\n- [x] I confirm that I own or have permission to submit this plugin and its preview assets.\n- [x] The plugin does not overwrite user configuration without explicit consent.\n- [x] I understand that approval is for listing and is not a security review.\n",
        unique_tags.join(", "),
        form_answer(suggested_tag),
        form_answer(notes)
    );
    if body.len() > MAX_DRAFT_BYTES {
        bail!("marketplace submission draft exceeds its size boundary");
    }
    secure_dir(&paths.publishing_dir)?;
    let draft_file = paths
        .publishing_dir
        .join(format!("{}-marketplace-submission.md", manifest.id));
    write_private(&draft_file, body.as_bytes())?;
    Ok(SubmissionDraft {
        ok: blockers.is_empty(),
        project_id: project.id.clone(),
        title: format!("[Plugin]: {}", manifest.name),
        repository: repository.to_owned(),
        category: category.to_owned(),
        tags: unique_tags,
        collision,
        form_url: MARKETPLACE_FORM,
        draft_file,
        body,
        blockers,
        security_review_status: security.status,
        security_review_revision: security.reviewed_revision,
    })
}

fn git_origin(paths: &AppPaths, project: &Project) -> Result<String> {
    let result = run_check(
        &CheckSpec {
            name: "release-origin".to_owned(),
            argv: vec![
                "git".to_owned(),
                "-C".to_owned(),
                project.project_root.display().to_string(),
                "remote".to_owned(),
                "get-url".to_owned(),
                "origin".to_owned(),
            ],
            timeout_seconds: 30,
        },
        &project.project_root,
        &paths.state_dir.join("command-output"),
    )?;
    if !result.ok {
        bail!("Git origin is unavailable");
    }
    Ok(result.stdout.trim().to_owned())
}

fn normalize_github_origin(origin: String) -> Option<String> {
    if let Some(path) = origin.strip_prefix("git@github.com:") {
        return Some(format!(
            "https://github.com/{}",
            path.trim_end_matches(".git")
        ));
    }
    origin
        .strip_prefix("https://github.com/")
        .map(|path| format!("https://github.com/{}", path.trim_end_matches(".git")))
}

fn validate_github_repository(repository: &str) -> Result<()> {
    let Some(path) = repository.strip_prefix("https://github.com/") else {
        bail!("marketplace repository must use a GitHub HTTPS root URL");
    };
    let path = path.trim_end_matches(".git");
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        })
    {
        bail!("marketplace repository must be a GitHub repository root URL");
    }
    Ok(())
}

fn has_named_file(root: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| root.join(name).is_file())
}

fn form_answer(value: Option<&str>) -> &str {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("_No response_")
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    write_private(path, &bytes)
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.is_symlink() {
        bail!("publishing artifact path is a symlink");
    }
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&temporary)
        .with_context(|| format!("create publishing artifact {}", temporary.display()))?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
