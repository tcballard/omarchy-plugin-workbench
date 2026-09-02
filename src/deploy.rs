use crate::manifest::validate_plugin;
use crate::model::{
    ActionReport, DeploymentEntry, DeploymentMode, DeploymentReceipt, GitState, Project,
    RECEIPT_SCHEMA, ValidationReport,
};
use crate::paths::{AppPaths, secure_dir};
use crate::process::{capture_tool, command_exists};
use crate::registry::{RegistryLock, now_unix};
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

pub fn validate(project: &Project) -> Result<ValidationReport> {
    let manifest = validate_plugin(&project.plugin_root)?;
    let plugin_root = project.plugin_root.to_string_lossy().into_owned();
    let omarchy_validation = capture_tool(
        "omarchy",
        &["plugin", "validate", &plugin_root],
        Some(&project.project_root),
    );
    if omarchy_validation.available && !omarchy_validation.ok {
        bail!(
            "official Omarchy validation failed: {}",
            omarchy_validation.output
        );
    }
    Ok(ValidationReport {
        ok: true,
        plugin_id: manifest.id,
        plugin_name: manifest.name,
        plugin_version: manifest.version,
        kinds: manifest.kinds,
        plugin_root: project.plugin_root.clone(),
        internal_validation: "passed".to_owned(),
        omarchy_validation,
    })
}

pub fn git_state(project_root: &Path) -> GitState {
    if !command_exists("git") || !project_root.join(".git").exists() {
        return GitState {
            revision: None,
            dirty: false,
        };
    }
    let root = project_root.to_string_lossy();
    let revision = capture_tool("git", &["-C", &root, "rev-parse", "HEAD"], None);
    let revision = revision
        .ok
        .then(|| revision.output.trim().to_owned())
        .filter(|value| !value.is_empty());
    let status = capture_tool(
        "git",
        &[
            "-C",
            &root,
            "status",
            "--porcelain=v1",
            "--untracked-files=normal",
        ],
        None,
    );
    let dirty = status.ok && !status.output.is_empty();
    GitState { revision, dirty }
}

pub fn deploy_live(paths: &AppPaths, project: &Project) -> Result<ActionReport> {
    let _lock = RegistryLock::acquire(paths)?;
    validate(project)?;
    secure_dir(&paths.plugins_dir)?;
    let git = git_state(&project.project_root);
    let entry = DeploymentEntry {
        mode: DeploymentMode::LiveLink,
        target: project.plugin_root.clone(),
        revision: git.revision,
        dirty: git.dirty,
        deployed_at_unix: now_unix(),
    };
    switch_deployment(paths, project, entry, "linked live checkout")
}

pub fn deploy_snapshot(paths: &AppPaths, project: &Project) -> Result<ActionReport> {
    let _lock = RegistryLock::acquire(paths)?;
    validate(project)?;
    secure_dir(&paths.plugins_dir)?;
    let git = git_state(&project.project_root);
    let fingerprint = content_fingerprint(&project.plugin_root)?;
    let snapshot_parent = paths.snapshots_dir.join(&project.id);
    secure_dir(&snapshot_parent)?;
    let snapshot_name = format!("{}-{}", now_unix(), &fingerprint[..12]);
    let snapshot = unique_path(&snapshot_parent, &snapshot_name);
    let temporary = snapshot_parent.join(format!(".stage.{}", std::process::id()));
    if temporary.exists() {
        bail!("staging path already exists: {}", temporary.display());
    }
    let copy_result = copy_tree(&project.plugin_root, &temporary);
    if let Err(error) = copy_result {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }
    fs::rename(&temporary, &snapshot).context("publish immutable plugin snapshot")?;
    let entry = DeploymentEntry {
        mode: DeploymentMode::Snapshot,
        target: snapshot,
        revision: git.revision,
        dirty: git.dirty,
        deployed_at_unix: now_unix(),
    };
    switch_deployment(paths, project, entry, "deployed immutable snapshot")
}

pub fn rollback(paths: &AppPaths, project: &Project) -> Result<ActionReport> {
    let _lock = RegistryLock::acquire(paths)?;
    let receipt_path = paths.receipt_path(&project.id);
    let mut receipt = load_receipt(&receipt_path)?
        .with_context(|| format!("project '{}' has no managed deployment", project.id))?;
    verify_managed_target(&receipt)?;
    if receipt.active_index == 0 {
        bail!(
            "project '{}' has no earlier deployment to roll back to",
            project.id
        );
    }
    let next_index = receipt.active_index - 1;
    let target = receipt.history[next_index].target.clone();
    if !target.is_dir() {
        bail!("rollback target no longer exists: {}", target.display());
    }
    atomic_link(&receipt.managed_target, &target)?;
    receipt.active_index = next_index;
    save_receipt(&receipt_path, &receipt)?;
    let warnings = rescan_warning();
    Ok(ActionReport {
        ok: true,
        action: "rollback".to_owned(),
        project_id: project.id.clone(),
        message: format!("rolled back to {}", target.display()),
        warnings,
    })
}

pub fn undeploy(paths: &AppPaths, project: &Project) -> Result<ActionReport> {
    let _lock = RegistryLock::acquire(paths)?;
    let receipt_path = paths.receipt_path(&project.id);
    let receipt = load_receipt(&receipt_path)?
        .with_context(|| format!("project '{}' has no managed deployment", project.id))?;
    verify_managed_target(&receipt)?;
    fs::remove_file(&receipt.managed_target)
        .with_context(|| format!("unlink {}", receipt.managed_target.display()))?;
    let warnings = rescan_warning();
    Ok(ActionReport {
        ok: true,
        action: "undeploy".to_owned(),
        project_id: project.id.clone(),
        message: "removed managed plugin link; snapshots were retained".to_owned(),
        warnings,
    })
}

pub fn load_receipt_for(paths: &AppPaths, id: &str) -> Result<Option<DeploymentReceipt>> {
    load_receipt(&paths.receipt_path(id))
}

fn switch_deployment(
    paths: &AppPaths,
    project: &Project,
    entry: DeploymentEntry,
    message: &str,
) -> Result<ActionReport> {
    let target = paths.plugins_dir.join(&project.id);
    let receipt_path = paths.receipt_path(&project.id);
    let existing_receipt = load_receipt(&receipt_path)?;
    if target.exists() || target.is_symlink() {
        let receipt = existing_receipt.as_ref().with_context(|| {
            format!(
                "refusing to replace unmanaged plugin target {}",
                target.display()
            )
        })?;
        verify_managed_target(receipt)?;
    }
    atomic_link(&target, &entry.target)?;

    let mut receipt = existing_receipt.unwrap_or(DeploymentReceipt {
        schema_version: RECEIPT_SCHEMA,
        plugin_id: project.id.clone(),
        managed_target: target,
        active_index: 0,
        history: Vec::new(),
    });
    if !receipt.history.is_empty() {
        receipt.history.truncate(receipt.active_index + 1);
    }
    receipt.history.push(entry.clone());
    receipt.active_index = receipt.history.len() - 1;
    save_receipt(&receipt_path, &receipt)?;
    let warnings = rescan_warning();
    Ok(ActionReport {
        ok: true,
        action: match entry.mode {
            DeploymentMode::LiveLink => "link",
            DeploymentMode::Snapshot => "snapshot",
        }
        .to_owned(),
        project_id: project.id.clone(),
        message: format!("{message}: {}", entry.target.display()),
        warnings,
    })
}

fn verify_managed_target(receipt: &DeploymentReceipt) -> Result<()> {
    if receipt.schema_version != RECEIPT_SCHEMA {
        bail!("unsupported deployment receipt schema");
    }
    let current = receipt
        .history
        .get(receipt.active_index)
        .context("deployment receipt active index is invalid")?;
    let meta = fs::symlink_metadata(&receipt.managed_target).with_context(|| {
        format!(
            "managed target is missing: {}",
            receipt.managed_target.display()
        )
    })?;
    if !meta.file_type().is_symlink() {
        bail!(
            "managed target was replaced outside Workbench: {}",
            receipt.managed_target.display()
        );
    }
    let actual = fs::read_link(&receipt.managed_target)?;
    if actual != current.target {
        bail!(
            "managed target changed outside Workbench: expected {}, found {}",
            current.target.display(),
            actual.display()
        );
    }
    Ok(())
}

fn atomic_link(target: &Path, source: &Path) -> Result<()> {
    if !source.is_dir() {
        bail!("deployment source is not a directory: {}", source.display());
    }
    let parent = target.parent().context("plugin target has no parent")?;
    let temp = parent.join(format!(".workbench-link.{}.tmp", std::process::id()));
    if temp.exists() || temp.is_symlink() {
        fs::remove_file(&temp).context("remove stale temporary plugin link")?;
    }
    symlink(source, &temp).with_context(|| format!("link snapshot {}", source.display()))?;
    let result = fs::rename(&temp, target).context("atomically switch plugin deployment");
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn load_receipt(path: &Path) -> Result<Option<DeploymentReceipt>> {
    if !path.exists() {
        return Ok(None);
    }
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > 1024 * 1024 {
        bail!("invalid deployment receipt: {}", path.display());
    }
    let receipt = serde_json::from_slice(&fs::read(path)?)
        .with_context(|| format!("parse deployment receipt {}", path.display()))?;
    Ok(Some(receipt))
}

fn save_receipt(path: &Path, receipt: &DeploymentReceipt) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(receipt)?;
    write_atomic_private(path, &bytes)
}

fn write_atomic_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn rescan_warning() -> Vec<String> {
    let result = capture_tool("omarchy-shell", &["shell", "rescanPlugins"], None);
    if !result.available {
        vec!["omarchy-shell is unavailable; rescan the shell on the Omarchy host".to_owned()]
    } else if !result.ok {
        vec![format!(
            "plugin switched, but shell rescan failed: {}",
            result.output
        )]
    } else {
        Vec::new()
    }
}

fn should_descend(entry: &DirEntry) -> bool {
    if !entry.file_type().is_dir() {
        return true;
    }
    !matches!(entry.file_name().to_str(), Some(".git" | "target"))
}

fn content_fingerprint(root: &Path) -> Result<String> {
    let mut entries = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(should_descend)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by(|left, right| left.path().cmp(right.path()));
    let mut hash = Sha256::new();
    for entry in entries {
        if !entry.file_type().is_file() {
            continue;
        }
        let relative = entry.path().strip_prefix(root)?;
        hash.update(relative.as_os_str().as_encoded_bytes());
        hash.update([0]);
        hash.update(fs::read(entry.path())?);
        hash.update([0]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir(destination).with_context(|| format!("create {}", destination.display()))?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o700))?;
    for entry in WalkDir::new(source)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_entry(should_descend)
    {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir(&target)?;
            fs::set_permissions(&target, fs::Permissions::from_mode(0o700))?;
        } else if entry.file_type().is_file() {
            fs::copy(entry.path(), &target)?;
            let source_mode = entry.metadata()?.permissions().mode();
            let mode = if source_mode & 0o111 != 0 {
                0o700
            } else {
                0o600
            };
            fs::set_permissions(&target, fs::Permissions::from_mode(mode))?;
        } else {
            bail!(
                "snapshot contains unsupported file: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn unique_path(parent: &Path, base: &str) -> PathBuf {
    let candidate = parent.join(base);
    if !candidate.exists() {
        return candidate;
    }
    parent.join(format!("{base}-{}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn fingerprint_changes_with_content() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("one"), "a").unwrap();
        let first = content_fingerprint(dir.path()).unwrap();
        fs::write(dir.path().join("one"), "b").unwrap();
        let second = content_fingerprint(dir.path()).unwrap();
        assert_ne!(first, second);
    }
}
