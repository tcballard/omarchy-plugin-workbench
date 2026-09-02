use crate::manifest::{ValidatedManifest, validate_plugin};
use crate::model::{Project, TestSessionRecord, TestSessionStatus};
use crate::paths::{AppPaths, secure_dir};
use crate::process::{command_exists, trusted_command};
use crate::registry::{RegistryLock, now_unix};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, symlink};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::process::Command;
use std::process::{Child, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const SESSION_SCHEMA: u32 = 1;
const MAX_RECORD_BYTES: u64 = 256 * 1024;
const START_TIMEOUT: Duration = Duration::from_secs(12);
const STOP_TIMEOUT: Duration = Duration::from_secs(3);
const ISOLATION_NOTICE: &str =
    "isolated HOME and XDG persistent state; same-user process, not a VM or security sandbox";

pub fn start(paths: &AppPaths, project: &Project) -> Result<TestSessionRecord> {
    let _lock = RegistryLock::acquire(paths)?;
    if list_unlocked(paths, Some(&project.id))?
        .iter()
        .any(|entry| entry.running)
    {
        bail!("project already has a running nested test session; stop it first");
    }
    require_host()?;
    let manifest = validate_plugin(&project.plugin_root)?;
    let omarchy_path = omarchy_path()?;
    let shell_path = omarchy_path.join("shell");
    if !shell_path.join("shell.qml").is_file() {
        bail!("Omarchy shell not found at {}", shell_path.display());
    }

    let id = format!("{}-{}-{}", project.id, now_unix(), std::process::id());
    let root = paths.test_sessions_dir.join(&id);
    prepare_root(paths, &root)?;
    let result = start_prepared(project, &manifest, &omarchy_path, &shell_path, &id, &root);
    if result.is_err() {
        let _ = remove_session_root(paths, &root);
    }
    result
}

fn start_prepared(
    project: &Project,
    manifest: &ValidatedManifest,
    omarchy_path: &Path,
    shell_path: &Path,
    id: &str,
    root: &Path,
) -> Result<TestSessionRecord> {
    let home = root.join("home");
    let config_home = home.join(".config");
    for dir in [
        &home,
        &config_home,
        &home.join(".cache"),
        &home.join(".local/state"),
        &home.join(".local/share"),
        &config_home.join("omarchy/plugins"),
    ] {
        secure_dir(dir)?;
    }
    symlink(
        &project.plugin_root,
        config_home.join("omarchy/plugins").join(&project.id),
    )
    .context("link live plugin source into disposable HOME")?;

    let disabled = first_party_plugin_ids(shell_path)?;
    write_private(
        &config_home.join("omarchy/shell.json"),
        &serde_json::to_vec_pretty(&shell_config(manifest, &disabled))?,
    )?;
    let compositor_config = root.join("hyprland.conf");
    write_private(&compositor_config, nested_hyprland_config().as_bytes())?;
    let log_path = root.join("session.log");
    let log = OpenOptions::new()
        .create_new(true)
        .append(true)
        .mode(0o600)
        .open(&log_path)
        .context("create nested session log")?;

    let mut compositor = trusted_command("Hyprland").context("trusted Hyprland is unavailable")?;
    compositor
        .args(["--config", compositor_config.to_string_lossy().as_ref()])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("OMARCHY_PATH", omarchy_path)
        .env_remove("HYPRLAND_INSTANCE_SIGNATURE")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log.try_clone()?));
    compositor.process_group(0);
    let mut compositor = compositor.spawn().context("start nested Hyprland")?;
    let compositor_pid = compositor.id();
    let compositor_start_ticks =
        process_start_ticks(compositor_pid).context("read nested Hyprland process identity")?;

    let (instance, display) = match wait_for_instance(&mut compositor, compositor_pid) {
        Ok(value) => value,
        Err(error) => {
            terminate_group(&mut compositor, compositor_start_ticks);
            return Err(error.context(format!("see {}", log_path.display())));
        }
    };

    let mut shell = trusted_command("quickshell").context("trusted quickshell is unavailable")?;
    shell
        .args(["-n", "-p", shell_path.to_string_lossy().as_ref()])
        .current_dir(shell_path)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &config_home)
        .env("XDG_CACHE_HOME", home.join(".cache"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("OMARCHY_PATH", omarchy_path)
        .env("HYPRLAND_INSTANCE_SIGNATURE", &instance)
        .env("WAYLAND_DISPLAY", &display)
        .env("XDG_CURRENT_DESKTOP", "Hyprland")
        .env("XDG_SESSION_DESKTOP", "Hyprland")
        .env("XDG_SESSION_TYPE", "wayland")
        .env("QS_NO_RELOAD_POPUP", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log));
    shell.process_group(0);
    let mut shell = match shell.spawn() {
        Ok(child) => child,
        Err(error) => {
            terminate_group(&mut compositor, compositor_start_ticks);
            return Err(error).context("start isolated Omarchy shell");
        }
    };
    let shell_pid = shell.id();
    let shell_start_ticks = match process_start_ticks(shell_pid) {
        Ok(ticks) => ticks,
        Err(error) => {
            terminate_fresh_child(&mut shell);
            terminate_group(&mut compositor, compositor_start_ticks);
            return Err(error).context("read shell process identity");
        }
    };
    thread::sleep(Duration::from_millis(800));
    match shell.try_wait().context("check isolated shell startup") {
        Ok(Some(status)) => {
            terminate_group(&mut compositor, compositor_start_ticks);
            bail!(
                "isolated Omarchy shell exited during startup ({status}); see {}",
                log_path.display()
            );
        }
        Err(error) => {
            terminate_group(&mut shell, shell_start_ticks);
            terminate_group(&mut compositor, compositor_start_ticks);
            return Err(error);
        }
        Ok(None) => {}
    }

    if manifest
        .kinds
        .iter()
        .any(|kind| matches!(kind.as_str(), "panel" | "overlay" | "menu"))
    {
        summon_plugin(
            shell_path,
            omarchy_path,
            &home,
            &config_home,
            &instance,
            &display,
            &manifest.id,
            &log_path,
        );
    }

    let record = TestSessionRecord {
        schema_version: SESSION_SCHEMA,
        id: id.to_owned(),
        project_id: project.id.clone(),
        root: root.to_path_buf(),
        compositor_pid,
        compositor_start_ticks,
        shell_pid,
        shell_start_ticks,
        hyprland_instance: instance,
        wayland_display: display,
        started_at_unix: now_unix(),
        live_source: true,
        isolation: ISOLATION_NOTICE.to_owned(),
    };
    if let Err(error) = write_private(
        &root.join("session.json"),
        &serde_json::to_vec_pretty(&record)?,
    ) {
        terminate_group(&mut shell, shell_start_ticks);
        terminate_group(&mut compositor, compositor_start_ticks);
        return Err(error).context("persist nested session ownership record");
    }
    Ok(record)
}

pub fn list(paths: &AppPaths, project_id: Option<&str>) -> Result<Vec<TestSessionStatus>> {
    let _lock = RegistryLock::acquire(paths)?;
    list_unlocked(paths, project_id)
}

pub fn active_count(paths: &AppPaths, project_id: &str) -> Result<usize> {
    Ok(list(paths, Some(project_id))?
        .into_iter()
        .filter(|entry| entry.running)
        .count())
}

fn list_unlocked(paths: &AppPaths, project_id: Option<&str>) -> Result<Vec<TestSessionStatus>> {
    let mut sessions = Vec::new();
    for entry in fs::read_dir(&paths.test_sessions_dir).context("read test sessions")? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() || !entry.file_type()?.is_dir() {
            continue;
        }
        let record_path = entry.path().join("session.json");
        if !record_path.exists() {
            continue;
        }
        let metadata = fs::symlink_metadata(&record_path)
            .with_context(|| format!("inspect {}", record_path.display()))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!(
                "nested session record is not a regular file: {}",
                record_path.display()
            );
        }
        if metadata.len() > MAX_RECORD_BYTES {
            bail!("nested session record exceeds {MAX_RECORD_BYTES} bytes");
        }
        let record: TestSessionRecord = serde_json::from_slice(&fs::read(&record_path)?)
            .with_context(|| format!("parse {}", record_path.display()))?;
        if record.schema_version != SESSION_SCHEMA || record.root != entry.path() {
            bail!("invalid nested session record at {}", record_path.display());
        }
        if project_id.is_some_and(|id| id != record.project_id) {
            continue;
        }
        let running = process_matches(record.compositor_pid, record.compositor_start_ticks)
            && process_matches(record.shell_pid, record.shell_start_ticks);
        sessions.push(TestSessionStatus {
            session: record,
            running,
        });
    }
    sessions.sort_by_key(|entry| entry.session.started_at_unix);
    Ok(sessions)
}

pub fn stop(paths: &AppPaths, project: &Project) -> Result<TestSessionRecord> {
    let _lock = RegistryLock::acquire(paths)?;
    let mut candidates = list_unlocked(paths, Some(&project.id))?;
    candidates.sort_by_key(|entry| std::cmp::Reverse(entry.session.started_at_unix));
    let status = candidates
        .into_iter()
        .next()
        .context("project has no nested test session")?;
    let record = status.session;
    terminate_pid_group(record.shell_pid, record.shell_start_ticks)?;
    terminate_pid_group(record.compositor_pid, record.compositor_start_ticks)?;
    remove_session_root(paths, &record.root)?;
    Ok(record)
}

fn require_host() -> Result<()> {
    for tool in ["Hyprland", "hyprctl", "quickshell"] {
        if !command_exists(tool) {
            bail!("{tool} is required for nested test sessions");
        }
    }
    if std::env::var_os("WAYLAND_DISPLAY").is_none() {
        bail!("WAYLAND_DISPLAY is not set; start this from an active Wayland session");
    }
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        bail!("XDG_RUNTIME_DIR is not set");
    }
    Ok(())
}

fn omarchy_path() -> Result<PathBuf> {
    let path = std::env::var_os("OMARCHY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"));
    let canonical = path
        .canonicalize()
        .with_context(|| format!("resolve OMARCHY_PATH {}", path.display()))?;
    if !canonical.is_dir() {
        bail!("OMARCHY_PATH is not a directory: {}", canonical.display());
    }
    Ok(canonical)
}

fn prepare_root(paths: &AppPaths, root: &Path) -> Result<()> {
    if !root.starts_with(&paths.test_sessions_dir) || root == paths.test_sessions_dir {
        bail!("refusing unsafe nested session root {}", root.display());
    }
    secure_dir(root)
}

fn remove_session_root(paths: &AppPaths, root: &Path) -> Result<()> {
    if !root.starts_with(&paths.test_sessions_dir) || root == paths.test_sessions_dir {
        bail!("refusing unsafe nested session cleanup {}", root.display());
    }
    let meta = fs::symlink_metadata(root).with_context(|| format!("inspect {}", root.display()))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        bail!(
            "nested session root is not a real directory: {}",
            root.display()
        );
    }
    fs::remove_dir_all(root).with_context(|| format!("erase disposable session {}", root.display()))
}

fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        secure_dir(parent)?;
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write {}", path.display()))?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

fn nested_hyprland_config() -> String {
    r#"monitor = ,1280x720@60,auto,1
animations { enabled = false }
decoration { shadow { enabled = false } }
misc {
  disable_hyprland_logo = true
  disable_splash_rendering = true
  force_default_wallpaper = 0
}
input { kb_layout = us }
bind = SUPER SHIFT, Q, exit,
"#
    .to_owned()
}

fn shell_config(manifest: &ValidatedManifest, disabled: &[String]) -> Value {
    let mut center = Vec::new();
    let mut plugins = Vec::new();
    let mut bar = json!({
        "position": "top",
        "transparent": false,
        "layout": {"left": [], "center": center, "right": []}
    });
    if manifest.kinds.iter().any(|kind| kind == "bar") {
        bar["id"] = json!(manifest.id);
    } else if manifest.kinds.iter().any(|kind| kind == "bar-widget") {
        center.push(json!({"id": manifest.id}));
        bar["layout"]["center"] = json!(center);
    }
    if manifest
        .kinds
        .iter()
        .any(|kind| matches!(kind.as_str(), "panel" | "overlay" | "menu" | "service"))
    {
        plugins.push(json!({"id": manifest.id}));
    }
    json!({
        "version": 1,
        "idle": {"screensaver": 0, "lock": 0},
        "bar": bar,
        "plugins": plugins,
        "disabledPlugins": disabled
    })
}

fn first_party_plugin_ids(shell_path: &Path) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    for entry in walkdir::WalkDir::new(shell_path.join("plugins")).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file()
            || !entry
                .file_name()
                .to_string_lossy()
                .ends_with("manifest.json")
        {
            continue;
        }
        let value: Value = match serde_json::from_slice(&fs::read(entry.path())?) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if let Some(id) = value.get("id").and_then(Value::as_str) {
            ids.push(id.to_owned());
        }
    }
    ids.sort();
    ids.dedup();
    Ok(ids)
}

#[allow(clippy::too_many_arguments)]
fn summon_plugin(
    shell_path: &Path,
    omarchy_path: &Path,
    home: &Path,
    config_home: &Path,
    instance: &str,
    display: &str,
    plugin_id: &str,
    log_path: &Path,
) {
    for _ in 0..20 {
        let Some(mut command) = trusted_command("quickshell") else {
            return;
        };
        let output = command
            .args([
                "ipc",
                "-p",
                shell_path.to_string_lossy().as_ref(),
                "call",
                "shell",
                "summon",
                plugin_id,
                "{}",
            ])
            .env("HOME", home)
            .env("XDG_CONFIG_HOME", config_home)
            .env("XDG_CACHE_HOME", home.join(".cache"))
            .env("XDG_STATE_HOME", home.join(".local/state"))
            .env("XDG_DATA_HOME", home.join(".local/share"))
            .env("OMARCHY_PATH", omarchy_path)
            .env("HYPRLAND_INSTANCE_SIGNATURE", instance)
            .env("WAYLAND_DISPLAY", display)
            .output();
        if output.is_ok_and(|result| {
            result.status.success() && String::from_utf8_lossy(&result.stdout).contains("ok")
        }) {
            return;
        }
        thread::sleep(Duration::from_millis(250));
    }
    if let Ok(mut log) = OpenOptions::new().append(true).open(log_path) {
        let _ = writeln!(
            log,
            "workbench: plugin {plugin_id} was enabled but could not be summoned automatically"
        );
    }
}

fn wait_for_instance(child: &mut Child, pid: u32) -> Result<(String, String)> {
    let started = Instant::now();
    while started.elapsed() < START_TIMEOUT {
        if let Some(status) = child.try_wait()? {
            bail!("nested Hyprland exited during startup ({status})");
        }
        if let Some(found) = hyprland_instance(pid) {
            return Ok(found);
        }
        thread::sleep(Duration::from_millis(150));
    }
    bail!("timed out waiting for nested Hyprland IPC instance")
}

fn hyprland_instance(pid: u32) -> Option<(String, String)> {
    for args in [["instances", "-j"], ["-j", "instances"]] {
        let output = trusted_command("hyprctl")?.args(args).output().ok()?;
        if !output.status.success() {
            continue;
        }
        let values: Vec<Value> = serde_json::from_slice(&output.stdout).ok()?;
        for value in values {
            if value.get("pid").and_then(Value::as_u64) != Some(u64::from(pid)) {
                continue;
            }
            let instance = value
                .get("instance")
                .or_else(|| value.get("instance_signature"))?
                .as_str()?;
            let display = value
                .get("wl_socket")
                .or_else(|| value.get("waylandSocket"))?
                .as_str()?;
            return Some((instance.to_owned(), display.to_owned()));
        }
    }
    None
}

fn process_start_ticks(pid: u32) -> Result<u64> {
    Ok(process_identity(pid)?.1)
}

fn process_identity(pid: u32) -> Result<(char, u64)> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let close = stat.rfind(')').context("malformed /proc stat")?;
    let fields = stat[close + 2..].split_whitespace().collect::<Vec<_>>();
    let state = fields
        .first()
        .and_then(|value| value.chars().next())
        .context("missing process state")?;
    let ticks = fields
        .get(19)
        .context("missing process start time")?
        .parse()
        .context("parse process start time")?;
    Ok((state, ticks))
}

fn process_matches(pid: u32, expected_ticks: u64) -> bool {
    process_identity(pid).is_ok_and(|(state, ticks)| state != 'Z' && ticks == expected_ticks)
}

fn terminate_group(child: &mut Child, expected_ticks: u64) {
    let _ = terminate_pid_group(child.id(), expected_ticks);
    let _ = child.wait();
}

fn terminate_fresh_child(child: &mut Child) {
    // SAFETY: this child was spawned immediately above as its own process-group
    // leader and has not yet been exposed outside this function.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGTERM);
    }
    let _ = child.wait();
}

fn terminate_pid_group(pid: u32, expected_ticks: u64) -> Result<()> {
    if !process_matches(pid, expected_ticks) {
        return Ok(());
    }
    // SAFETY: the process identity was checked against its Linux start time and
    // every nested-session child is launched as leader of its own process group.
    unsafe {
        libc::kill(-(pid as i32), libc::SIGTERM);
    }
    let started = Instant::now();
    while started.elapsed() < STOP_TIMEOUT {
        if !process_matches(pid, expected_ticks) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(50));
    }
    if process_matches(pid, expected_ticks) {
        // SAFETY: identity is checked again immediately before escalation.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_config_enables_only_target_shapes() {
        let manifest = ValidatedManifest {
            id: "io.test.panel".into(),
            name: "Panel".into(),
            version: "1".into(),
            kinds: vec!["panel".into(), "bar-widget".into()],
        };
        let value = shell_config(&manifest, &["omarchy.lock".into()]);
        assert_eq!(value["bar"]["layout"]["center"][0]["id"], "io.test.panel");
        assert_eq!(value["plugins"][0]["id"], "io.test.panel");
        assert_eq!(value["disabledPlugins"][0], "omarchy.lock");
        assert_eq!(value["idle"]["lock"], 0);
    }

    #[test]
    fn current_process_identity_is_readable() {
        let pid = std::process::id();
        let ticks = process_start_ticks(pid).unwrap();
        assert!(process_matches(pid, ticks));
        assert!(!process_matches(pid, ticks.saturating_add(1)));
    }

    #[test]
    fn cleanup_rejects_parent_directory() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_bases(
            temp.path().join("home"),
            temp.path().join("config"),
            temp.path().join("state"),
        );
        paths.ensure().unwrap();
        assert!(remove_session_root(&paths, &paths.test_sessions_dir).is_err());
    }

    #[test]
    fn cleanup_rejects_symlink_root() {
        let temp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_bases(
            temp.path().join("home"),
            temp.path().join("config"),
            temp.path().join("state"),
        );
        paths.ensure().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        let link = paths.test_sessions_dir.join("forged");
        symlink(&outside, &link).unwrap();
        assert!(remove_session_root(&paths, &link).is_err());
        assert!(outside.is_dir());
    }

    #[test]
    fn process_identity_prevents_reused_pid_signalling() {
        let mut command = Command::new("sleep");
        command.arg("30");
        command.process_group(0);
        let mut child = command.spawn().unwrap();
        let ticks = process_start_ticks(child.id()).unwrap();
        terminate_pid_group(child.id(), ticks.saturating_add(1)).unwrap();
        assert!(child.try_wait().unwrap().is_none());
        terminate_pid_group(child.id(), ticks).unwrap();
        let _ = child.wait();
        assert!(!process_matches(child.id(), ticks));
    }
}
