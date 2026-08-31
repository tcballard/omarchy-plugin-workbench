use crate::deploy::git_state;
use crate::model::{EvidenceRecord, HandoffRecord, Project, SessionRecord};
use crate::paths::{AppPaths, secure_dir};
use crate::registry::{RegistryLock, now_unix};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::process::Command;

const SESSION_SCHEMA: u32 = 1;
const HANDOFF_SCHEMA: u32 = 1;
const EVIDENCE_SCHEMA: u32 = 1;
const MAX_STATE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_EVIDENCE_BYTES: u64 = 64 * 1024 * 1024;

pub fn start_session(
    paths: &AppPaths,
    project: &Project,
    task: &str,
    agent: Option<&str>,
    objective: &str,
) -> Result<SessionRecord> {
    validate_slug(task)?;
    if objective.trim().is_empty() || objective.len() > 4096 {
        bail!("session objective must be 1-4096 characters");
    }
    if agent.is_some_and(|value| value.trim().is_empty() || value.len() > 80) {
        bail!("agent label must be 1-80 characters");
    }
    let source_git = git_state(&project.project_root);
    if source_git.revision.is_none() {
        bail!("isolated sessions require a Git repository with at least one commit");
    }
    if source_git.dirty {
        bail!("source checkout is dirty; commit or stash it before creating an isolated session");
    }
    let _lock = RegistryLock::acquire(paths)?;
    let mut sessions = load_sessions(paths)?;
    let branch = format!("codex/{task}");
    if sessions
        .iter()
        .any(|item| item.closed_at_unix.is_none() && item.branch == branch)
    {
        bail!("an active session already owns branch {branch}");
    }
    let project_dir = paths.sessions_dir.join(&project.id);
    secure_dir(&project_dir)?;
    let worktree = project_dir.join(task);
    if worktree.exists() || worktree.is_symlink() {
        bail!("session worktree already exists: {}", worktree.display());
    }
    let output = Command::new("git")
        .args(["-C"])
        .arg(&project.project_root)
        .args(["worktree", "add", "-b", &branch])
        .arg(&worktree)
        .output()
        .context("start git worktree session")?;
    if !output.status.success() {
        bail!(
            "git could not create session worktree: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let started_at_unix = now_unix();
    let record = SessionRecord {
        schema_version: SESSION_SCHEMA,
        id: format!("{}-{task}-{started_at_unix}", project.id),
        project_id: project.id.clone(),
        task: task.to_owned(),
        agent: agent.map(str::to_owned),
        objective: objective.to_owned(),
        branch,
        worktree,
        started_at_unix,
        closed_at_unix: None,
    };
    sessions.push(record.clone());
    save_json(&paths.sessions_file, &sessions)?;
    Ok(record)
}

pub fn list_sessions(paths: &AppPaths, project_id: Option<&str>) -> Result<Vec<SessionRecord>> {
    let mut sessions = load_sessions(paths)?;
    if let Some(id) = project_id {
        sessions.retain(|item| item.project_id == id);
    }
    sessions.sort_by_key(|item| std::cmp::Reverse(item.started_at_unix));
    Ok(sessions)
}

pub fn active_session_count(paths: &AppPaths, project_id: &str) -> Result<usize> {
    Ok(load_sessions(paths)?
        .iter()
        .filter(|item| item.project_id == project_id && item.closed_at_unix.is_none())
        .count())
}

pub fn close_session(paths: &AppPaths, session_id: &str) -> Result<SessionRecord> {
    let _lock = RegistryLock::acquire(paths)?;
    let mut sessions = load_sessions(paths)?;
    let session = sessions
        .iter_mut()
        .find(|item| item.id == session_id)
        .with_context(|| format!("session '{session_id}' does not exist"))?;
    if session.closed_at_unix.is_some() {
        bail!("session is already closed");
    }
    session.closed_at_unix = Some(now_unix());
    let updated = session.clone();
    save_json(&paths.sessions_file, &sessions)?;
    Ok(updated)
}

pub fn write_handoff(
    paths: &AppPaths,
    session_id: &str,
    decisions: Vec<String>,
    blockers: Vec<String>,
    next_action: &str,
) -> Result<HandoffRecord> {
    if next_action.trim().is_empty() || next_action.len() > 4096 {
        bail!("next action must be 1-4096 characters");
    }
    validate_notes(&decisions, "decision")?;
    validate_notes(&blockers, "blocker")?;
    let session = load_sessions(paths)?
        .into_iter()
        .find(|item| item.id == session_id)
        .with_context(|| format!("session '{session_id}' does not exist"))?;
    let git = git_state(&session.worktree);
    let handoff = HandoffRecord {
        schema_version: HANDOFF_SCHEMA,
        session_id: session.id,
        project_id: session.project_id,
        objective: session.objective,
        decisions,
        blockers,
        next_action: next_action.to_owned(),
        branch: session.branch,
        worktree: session.worktree,
        revision: git.revision,
        dirty: git.dirty,
        recorded_at_unix: now_unix(),
    };
    save_json(
        &paths
            .handoffs_dir
            .join(format!("{}.json", safe_id(session_id))),
        &handoff,
    )?;
    Ok(handoff)
}

pub fn append_evidence(paths: &AppPaths, record: &EvidenceRecord) -> Result<()> {
    let _lock = RegistryLock::acquire(paths)?;
    let path = evidence_path(paths, &record.project_id);
    let mut bytes = serde_json::to_vec(record).context("serialize evidence")?;
    bytes.push(b'\n');
    let existing_len = if path.exists() || path.is_symlink() {
        let meta = fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() || !meta.is_file() {
            bail!("evidence ledger is not a regular file");
        }
        meta.len()
    } else {
        0
    };
    if existing_len.saturating_add(bytes.len() as u64) > MAX_EVIDENCE_BYTES {
        bail!("evidence ledger exceeds {MAX_EVIDENCE_BYTES} bytes");
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)
        .with_context(|| format!("open evidence ledger {}", path.display()))?;
    file.write_all(&bytes).context("append evidence")?;
    file.sync_data().context("sync evidence ledger")?;
    Ok(())
}

pub fn evidence_record(
    project: &Project,
    kind: &str,
    name: &str,
    ok: bool,
    detail: serde_json::Value,
) -> EvidenceRecord {
    let git = git_state(&project.project_root);
    EvidenceRecord {
        schema_version: EVIDENCE_SCHEMA,
        project_id: project.id.clone(),
        kind: kind.to_owned(),
        name: name.to_owned(),
        ok,
        revision: git.revision,
        dirty: git.dirty,
        platform: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        recorded_at_unix: now_unix(),
        detail,
    }
}

pub fn read_evidence(
    paths: &AppPaths,
    project_id: &str,
    limit: usize,
) -> Result<Vec<EvidenceRecord>> {
    if limit == 0 || limit > 1000 {
        bail!("evidence limit must be between 1 and 1000");
    }
    let path = evidence_path(paths, project_id);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let meta = fs::symlink_metadata(&path)?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_EVIDENCE_BYTES {
        bail!("evidence ledger is not a bounded regular file");
    }
    let file = fs::File::open(&path)?;
    let mut records = BufReader::new(file)
        .lines()
        .map(|line| -> Result<EvidenceRecord> {
            serde_json::from_str(&line?).context("parse evidence record")
        })
        .collect::<Result<Vec<_>>>()?;
    records.reverse();
    records.truncate(limit);
    Ok(records)
}

pub fn has_passing_checks(
    paths: &AppPaths,
    project: &Project,
    revision: Option<&str>,
) -> Result<bool> {
    Ok(read_evidence(paths, &project.id, 1000)?
        .iter()
        .find(|item| item.kind == "check" && item.revision.as_deref() == revision)
        .is_some_and(|item| item.ok && !item.dirty))
}

fn load_sessions(paths: &AppPaths) -> Result<Vec<SessionRecord>> {
    if !paths.sessions_file.exists() {
        return Ok(Vec::new());
    }
    let meta = fs::symlink_metadata(&paths.sessions_file)?;
    if meta.file_type().is_symlink() || !meta.is_file() || meta.len() > MAX_STATE_BYTES {
        bail!("session registry is not a bounded regular file");
    }
    serde_json::from_slice(&fs::read(&paths.sessions_file)?).context("parse session registry")
}

fn save_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("state file has no parent")?;
    secure_dir(parent)?;
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        serde_json::to_writer_pretty(&mut file, value)?;
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

fn evidence_path(paths: &AppPaths, project_id: &str) -> std::path::PathBuf {
    paths
        .evidence_dir
        .join(format!("{}.jsonl", safe_id(project_id)))
}

fn validate_slug(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || value.ends_with('-')
        || value
            .chars()
            .any(|ch| !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'))
    {
        bail!("task must be a lowercase letters, digits, and hyphens slug");
    }
    Ok(())
}

fn validate_notes(notes: &[String], kind: &str) -> Result<()> {
    if notes.len() > 64
        || notes
            .iter()
            .any(|item| item.trim().is_empty() || item.len() > 4096)
    {
        bail!("{kind} entries must contain at most 64 non-empty values of 4096 characters");
    }
    Ok(())
}

fn safe_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
