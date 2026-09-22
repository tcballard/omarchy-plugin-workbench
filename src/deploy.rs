use crate::manifest::validate_plugin;
use crate::model::{
    ActionReport, DeploymentEntry, DeploymentMode, DeploymentReceipt, GitState, Project,
    RECEIPT_SCHEMA, ValidationReport,
};
use crate::paths::{AppPaths, open_directory, secure_dir};
use crate::process::{capture_tool, command_exists};
use crate::registry::{RegistryLock, now_unix};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

#[derive(Serialize, Deserialize)]
struct DeploymentJournal {
    target: PathBuf,
    previous: Option<DeploymentReceipt>,
}

fn journal_path(paths: &AppPaths, id: &str) -> PathBuf {
    paths.receipts_dir.join(format!("{id}.journal"))
}

fn publish_deployment(
    paths: &AppPaths,
    id: &str,
    link: &Path,
    source: &Path,
    previous: Option<DeploymentReceipt>,
    next: &DeploymentReceipt,
) -> Result<()> {
    let journal = journal_path(paths, id);
    write_atomic_private(
        &journal,
        &serde_json::to_vec(&DeploymentJournal {
            target: link.to_path_buf(),
            previous,
        })?,
    )?;
    let operation = (|| -> Result<()> {
        atomic_link(link, source)?;
        save_receipt(&paths.receipt_path(id), next)?;
        Ok(())
    })();
    if let Err(error) = operation {
        recover_journal(paths, id).context("restore interrupted deployment")?;
        return Err(error).context("publish deployment; previous link and receipt restored");
    }
    unlink_private(&journal)?;
    let parent = open_directory(&paths.receipts_dir, false)?;
    if unsafe { libc::fsync(parent.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error())
            .context("sync completed deployment transaction");
    }
    Ok(())
}

fn recover_journal(paths: &AppPaths, id: &str) -> Result<()> {
    let path = journal_path(paths, id);
    let Some(bytes) = read_private_file(&path)? else {
        return Ok(());
    };
    let journal: DeploymentJournal =
        serde_json::from_slice(&bytes).context("parse deployment journal")?;
    if journal.target != paths.plugins_dir.join(id) {
        bail!("deployment journal target mismatch");
    }
    match journal.previous {
        Some(receipt) => {
            let prior = &receipt
                .history
                .get(receipt.active_index)
                .context("invalid previous deployment in journal")?
                .target;
            atomic_link(&journal.target, prior)?;
            save_receipt(&paths.receipt_path(id), &receipt)?;
        }
        None => {
            if journal.target.is_symlink() {
                remove_link(&journal.target)?;
            }
            let receipt = paths.receipt_path(id);
            if receipt.exists() {
                unlink_private(&receipt)?;
            }
        }
    }
    unlink_private(&path)?;
    Ok(())
}

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
    recover_journal(paths, &project.id)?;
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
    recover_journal(paths, &project.id)?;
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
    if content_fingerprint(&temporary)? != fingerprint
        || content_fingerprint(&project.plugin_root)? != fingerprint
    {
        fs::remove_dir_all(&temporary)?;
        bail!("plugin source changed during snapshot copy");
    }
    let mut staged_project = project.clone();
    staged_project.plugin_root = temporary.clone();
    validate(&staged_project).context("validate copied snapshot before publication")?;
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
    recover_journal(paths, &project.id)?;
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
    let previous_receipt = receipt.clone();
    receipt.active_index = next_index;
    publish_deployment(
        paths,
        &project.id,
        &receipt.managed_target,
        &target,
        Some(previous_receipt),
        &receipt,
    )?;
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
    recover_journal(paths, &project.id)?;
    let receipt_path = paths.receipt_path(&project.id);
    let receipt = load_receipt(&receipt_path)?
        .with_context(|| format!("project '{}' has no managed deployment", project.id))?;
    verify_managed_target(&receipt)?;
    remove_link(&receipt.managed_target)?;
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
    let _lock = RegistryLock::acquire(paths)?;
    recover_journal(paths, id)?;
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
    let mut receipt = existing_receipt.clone().unwrap_or(DeploymentReceipt {
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
    publish_deployment(
        paths,
        &project.id,
        &receipt.managed_target,
        &entry.target,
        existing_receipt,
        &receipt,
    )?;
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
    let parent = open_directory(
        receipt
            .managed_target
            .parent()
            .context("managed target has no parent")?,
        false,
    )?;
    let name = CString::new(
        receipt
            .managed_target
            .file_name()
            .context("managed target has no name")?
            .as_encoded_bytes(),
    )?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error()).context("inspect managed plugin link");
    }
    let stat = unsafe { stat.assume_init() };
    if stat.st_mode & libc::S_IFMT != libc::S_IFLNK {
        bail!(
            "managed target was replaced outside Workbench: {}",
            receipt.managed_target.display()
        );
    }
    let mut buffer = vec![0_u8; 4096];
    let size = unsafe {
        libc::readlinkat(
            parent.as_raw_fd(),
            name.as_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if size < 0 || size as usize == buffer.len() {
        bail!("cannot read complete managed plugin link");
    }
    buffer.truncate(size as usize);
    let actual = PathBuf::from(std::ffi::OsString::from_vec(buffer));
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
    let _source = open_directory(source, false)?;
    let parent = target.parent().context("plugin target has no parent")?;
    let parent = open_directory(parent, false)?;
    let name = CString::new(
        target
            .file_name()
            .context("plugin target has no name")?
            .as_encoded_bytes(),
    )?;
    let temp = CString::new(format!(".workbench-link.{}.tmp", std::process::id()))?;
    let source_name = CString::new(source.as_os_str().as_encoded_bytes())?;
    if unsafe { libc::symlinkat(source_name.as_ptr(), parent.as_raw_fd(), temp.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("create temporary plugin link");
    }
    if unsafe {
        libc::renameat(
            parent.as_raw_fd(),
            temp.as_ptr(),
            parent.as_raw_fd(),
            name.as_ptr(),
        )
    } != 0
    {
        unsafe { libc::unlinkat(parent.as_raw_fd(), temp.as_ptr(), 0) };
        return Err(std::io::Error::last_os_error()).context("switch plugin link");
    }
    if unsafe { libc::fsync(parent.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("sync plugin directory");
    }
    Ok(())
}

fn remove_link(target: &Path) -> Result<()> {
    let parent = open_directory(
        target.parent().context("plugin target has no parent")?,
        false,
    )?;
    let name = CString::new(
        target
            .file_name()
            .context("plugin target has no name")?
            .as_encoded_bytes(),
    )?;
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error()).context("unlink managed plugin");
    }
    if unsafe { libc::fsync(parent.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("sync plugin directory");
    }
    Ok(())
}

fn unlink_private(path: &Path) -> Result<()> {
    let parent = open_directory(path.parent().context("private file has no parent")?, false)?;
    let name = CString::new(
        path.file_name()
            .context("private file has no name")?
            .as_encoded_bytes(),
    )?;
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error()).context("remove private file");
    }
    if unsafe { libc::fsync(parent.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("sync private directory");
    }
    Ok(())
}

fn load_receipt(path: &Path) -> Result<Option<DeploymentReceipt>> {
    let Some(bytes) = read_private_file(path)? else {
        return Ok(None);
    };
    let receipt = serde_json::from_slice(&bytes)
        .with_context(|| format!("parse deployment receipt {}", path.display()))?;
    Ok(Some(receipt))
}

fn read_private_file(path: &Path) -> Result<Option<Vec<u8>>> {
    let parent = open_directory(path.parent().context("receipt has no parent")?, false)?;
    let name = CString::new(
        path.file_name()
            .context("receipt has no name")?
            .as_encoded_bytes(),
    )?;
    let raw = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::NotFound {
            return Ok(None);
        }
        return Err(error).context("open deployment receipt");
    }
    let file = unsafe { File::from_raw_fd(raw) };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > 1024 * 1024 {
        bail!("invalid deployment receipt: {}", path.display());
    }
    let mut bytes = Vec::new();
    file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        bail!("deployment receipt exceeds size limit");
    }
    Ok(Some(bytes))
}

fn save_receipt(path: &Path, receipt: &DeploymentReceipt) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(receipt)?;
    write_atomic_private(path, &bytes)
}

fn write_atomic_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = open_directory(path.parent().context("receipt has no parent")?, false)?;
    let name = CString::new(
        path.file_name()
            .context("receipt has no name")?
            .as_encoded_bytes(),
    )?;
    let temporary = CString::new(format!(
        "{}.tmp.{}",
        name.to_string_lossy(),
        std::process::id()
    ))?;
    let raw = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            temporary.as_ptr(),
            libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if raw < 0 {
        return Err(std::io::Error::last_os_error()).context("create private receipt stage");
    }
    let mut file = unsafe { File::from_raw_fd(raw) };
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        if unsafe {
            libc::renameat(
                parent.as_raw_fd(),
                temporary.as_ptr(),
                parent.as_raw_fd(),
                name.as_ptr(),
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error()).context("publish deployment receipt");
        }
        if unsafe { libc::fsync(parent.as_raw_fd()) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("sync deployment receipts directory");
        }
        Ok(())
    })();
    if result.is_err() {
        unsafe { libc::unlinkat(parent.as_raw_fd(), temporary.as_ptr(), 0) };
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
    let source_fd = open_directory(source, false)?;
    let destination_parent = open_directory(
        destination.parent().context("snapshot has no parent")?,
        false,
    )?;
    let name = CString::new(
        destination
            .file_name()
            .context("snapshot has no name")?
            .as_encoded_bytes(),
    )?;
    if unsafe { libc::mkdirat(destination_parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        return Err(std::io::Error::last_os_error()).context("create snapshot stage");
    }
    let destination_fd = open_child_directory(destination_parent.as_raw_fd(), &name)?;
    copy_directory(&source_fd, &destination_fd)
}

fn open_child_directory(parent: i32, name: &CString) -> Result<OwnedFd> {
    let raw = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(std::io::Error::last_os_error()).context("open snapshot child directory");
    }
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

fn copy_directory(source: &OwnedFd, destination: &OwnedFd) -> Result<()> {
    for entry in fs::read_dir(format!("/proc/self/fd/{}", source.as_raw_fd()))? {
        let entry = entry?;
        let name_os = entry.file_name();
        let name = CString::new(name_os.as_encoded_bytes())?;
        let raw = unsafe {
            libc::openat(
                source.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if raw < 0 {
            return Err(std::io::Error::last_os_error())
                .context("open snapshot source without following links");
        }
        let mut input = unsafe { File::from_raw_fd(raw) };
        let metadata = input.metadata()?;
        if metadata.is_dir() {
            if name_os == ".git" || name_os == "target" {
                continue;
            }
            if unsafe { libc::mkdirat(destination.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                return Err(std::io::Error::last_os_error()).context("create snapshot directory");
            }
            let source_child = open_child_directory(source.as_raw_fd(), &name)?;
            let destination_child = open_child_directory(destination.as_raw_fd(), &name)?;
            copy_directory(&source_child, &destination_child)?;
        } else if metadata.is_file() {
            let mode = if metadata.permissions().mode() & 0o111 != 0 {
                0o700
            } else {
                0o600
            };
            let output = unsafe {
                libc::openat(
                    destination.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_WRONLY
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    mode,
                )
            };
            if output < 0 {
                return Err(std::io::Error::last_os_error()).context("create snapshot file");
            }
            let mut output = unsafe { File::from_raw_fd(output) };
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
        } else {
            bail!(
                "snapshot contains unsupported file: {}",
                entry.path().display()
            );
        }
    }
    if unsafe { libc::fsync(destination.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error()).context("sync snapshot directory");
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
    use std::os::unix::fs::symlink;
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

    #[test]
    fn snapshot_copy_rejects_linked_source_files() {
        let root = tempdir().unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(root.path().join("outside"), "secret").unwrap();
        symlink(root.path().join("outside"), source.join("linked")).unwrap();
        assert!(copy_tree(&source, &root.path().join("stage")).is_err());
        assert!(!root.path().join("stage/linked").exists());
    }

    #[test]
    fn receipt_failure_restores_the_preceding_link() {
        let root = tempdir().unwrap();
        let paths = AppPaths::from_bases(
            root.path().join("home"),
            root.path().join("config"),
            root.path().join("state"),
        );
        paths.ensure().unwrap();
        secure_dir(&paths.plugins_dir).unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        fs::create_dir(&old).unwrap();
        fs::create_dir(&new).unwrap();
        let target = paths.plugins_dir.join("io.test.plugin");
        symlink(&old, &target).unwrap();
        let receipt_path = paths.receipt_path("io.test.plugin");
        let old_entry = DeploymentEntry {
            mode: DeploymentMode::Snapshot,
            target: old.clone(),
            revision: None,
            dirty: false,
            deployed_at_unix: 0,
        };
        let receipt = DeploymentReceipt {
            schema_version: RECEIPT_SCHEMA,
            plugin_id: "io.test.plugin".to_owned(),
            managed_target: target.clone(),
            active_index: 0,
            history: vec![old_entry],
        };
        save_receipt(&receipt_path, &receipt).unwrap();
        fs::create_dir(
            receipt_path.with_file_name(format!("io.test.plugin.json.tmp.{}", std::process::id())),
        )
        .unwrap();
        let project = Project {
            id: "io.test.plugin".to_owned(),
            name: "Test".to_owned(),
            project_root: root.path().to_path_buf(),
            plugin_root: root.path().to_path_buf(),
            checks: vec![],
            workflows: vec![],
            environment: vec![],
            project_checks_trusted: false,
            trusted_definition_digest: None,
            definition_digest: None,
            approved_capabilities: vec![],
            added_at_unix: 0,
        };
        let entry = DeploymentEntry {
            mode: DeploymentMode::Snapshot,
            target: new,
            revision: None,
            dirty: false,
            deployed_at_unix: 0,
        };
        assert!(switch_deployment(&paths, &project, entry, "test").is_err());
        assert_eq!(fs::read_link(target).unwrap(), old);
    }

    #[test]
    fn interrupted_link_switch_recovers_receipt_and_link() {
        let root = tempdir().unwrap();
        let paths = AppPaths::from_bases(
            root.path().join("home"),
            root.path().join("config"),
            root.path().join("state"),
        );
        paths.ensure().unwrap();
        secure_dir(&paths.plugins_dir).unwrap();
        let old = root.path().join("old");
        let new = root.path().join("new");
        fs::create_dir(&old).unwrap();
        fs::create_dir(&new).unwrap();
        let id = "io.test.plugin";
        let link = paths.plugins_dir.join(id);
        symlink(&old, &link).unwrap();
        let receipt = DeploymentReceipt {
            schema_version: RECEIPT_SCHEMA,
            plugin_id: id.to_owned(),
            managed_target: link.clone(),
            active_index: 0,
            history: vec![DeploymentEntry {
                mode: DeploymentMode::Snapshot,
                target: old.clone(),
                revision: None,
                dirty: false,
                deployed_at_unix: 0,
            }],
        };
        save_receipt(&paths.receipt_path(id), &receipt).unwrap();
        write_atomic_private(
            &journal_path(&paths, id),
            &serde_json::to_vec(&DeploymentJournal {
                target: link.clone(),
                previous: Some(receipt),
            })
            .unwrap(),
        )
        .unwrap();
        atomic_link(&link, &new).unwrap();
        recover_journal(&paths, id).unwrap();
        assert_eq!(fs::read_link(link).unwrap(), old);
        assert_eq!(
            load_receipt(&paths.receipt_path(id))
                .unwrap()
                .unwrap()
                .active_index,
            0
        );
        assert!(!journal_path(&paths, id).exists());
    }
}
