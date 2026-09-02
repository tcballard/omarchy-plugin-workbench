use crate::model::{CheckResult, CheckSpec, ToolResult};
use anyhow::{Context, Result, bail};
use std::ffi::OsStr;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread;
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;

const OUTPUT_LIMIT: usize = 64 * 1024;
const PIPE_CHUNK: usize = 8 * 1024;
const PIPE_QUEUE_DEPTH: usize = 4;
const TOOL_TIMEOUT: Duration = Duration::from_secs(60);
const TERMINATION_GRACE: Duration = Duration::from_millis(250);
const REAP_GRACE: Duration = Duration::from_secs(2);
const FIXED_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

#[derive(Clone, Copy)]
enum EnvironmentPolicy {
    Inherited,
    Trusted,
}

#[derive(Clone, Copy)]
enum Stream {
    Stdout,
    Stderr,
}

enum PipeEvent {
    Data(Stream, Vec<u8>),
    Eof(Stream),
    Error(Stream, String),
}

struct CommandOutcome {
    status: ExitStatus,
    timed_out: bool,
    output_limited: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

pub fn command_exists(name: &str) -> bool {
    resolve_trusted_tool(name).is_some()
}

pub fn trusted_command(name: &str) -> Option<Command> {
    let executable = resolve_trusted_tool(name)?;
    let mut command = Command::new(executable);
    apply_trusted_environment(&mut command);
    Some(command)
}

pub fn capture_tool(command: &str, args: &[&str], cwd: Option<&Path>) -> ToolResult {
    let Some(executable) = resolve_trusted_tool(command) else {
        return unavailable_tool();
    };
    tool_result(run_command(
        &executable,
        args,
        cwd,
        TOOL_TIMEOUT,
        EnvironmentPolicy::Trusted,
    ))
}

pub fn capture_project_tool(command: &str, args: &[&str], cwd: Option<&Path>) -> ToolResult {
    let Some(executable) = project_tool_path(command) else {
        return unavailable_tool();
    };
    tool_result(run_command(
        &executable,
        args,
        cwd,
        TOOL_TIMEOUT,
        EnvironmentPolicy::Inherited,
    ))
}

pub fn run_check(check: &CheckSpec, cwd: &Path, _temporary_dir: &Path) -> Result<CheckResult> {
    let executable = check.argv.first().context("empty check argv")?;
    reject_privilege_escalation(executable)?;
    let executable = project_tool_path(executable)
        .with_context(|| format!("project check executable is unavailable: {executable}"))?;
    run_check_with(check, cwd, &executable, EnvironmentPolicy::Inherited)
}

pub fn run_trusted_check(
    check: &CheckSpec,
    cwd: &Path,
    _temporary_dir: &Path,
) -> Result<CheckResult> {
    let executable = check.argv.first().context("empty trusted tool argv")?;
    reject_privilege_escalation(executable)?;
    let executable = resolve_trusted_tool(executable)
        .with_context(|| format!("trusted tool is unavailable: {executable}"))?;
    run_check_with(check, cwd, &executable, EnvironmentPolicy::Trusted)
}

fn run_check_with(
    check: &CheckSpec,
    cwd: &Path,
    executable: &Path,
    environment: EnvironmentPolicy,
) -> Result<CheckResult> {
    let started = Instant::now();
    let outcome = run_command(
        executable,
        &check.argv[1..],
        Some(cwd),
        Duration::from_secs(check.timeout_seconds),
        environment,
    )
    .with_context(|| format!("run check '{}'", check.name))?;
    let (stdout, stdout_truncated) = bounded_text(&outcome.stdout);
    let (mut stderr, stderr_truncated) = bounded_text(&outcome.stderr);
    if outcome.output_limited {
        append_notice(&mut stderr, "output limit exceeded; process group terminated");
    }
    if outcome.timed_out {
        append_notice(&mut stderr, "deadline exceeded; process group terminated");
    }
    Ok(CheckResult {
        name: check.name.clone(),
        argv: check.argv.clone(),
        ok: !outcome.timed_out && !outcome.output_limited && outcome.status.success(),
        exit_code: outcome.status.code(),
        timed_out: outcome.timed_out,
        duration_ms: started.elapsed().as_millis(),
        stdout,
        stderr,
        output_truncated: outcome.output_limited || stdout_truncated || stderr_truncated,
    })
}

fn run_command<S: AsRef<OsStr>>(
    executable: &Path,
    args: &[S],
    cwd: Option<&Path>,
    timeout: Duration,
    environment: EnvironmentPolicy,
) -> Result<CommandOutcome> {
    enable_child_subreaper()?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    if matches!(environment, EnvironmentPolicy::Trusted) {
        apply_trusted_environment(&mut command);
    }
    command.process_group(0);
    let mut child = command
        .spawn()
        .with_context(|| format!("start executable {}", executable.display()))?;
    let process_group = child.id() as i32;
    let (sender, receiver) = sync_channel(PIPE_QUEUE_DEPTH);
    spawn_pipe_reader(
        child.stdout.take().context("capture child stdout")?,
        Stream::Stdout,
        sender.clone(),
    );
    spawn_pipe_reader(
        child.stderr.take().context("capture child stderr")?,
        Stream::Stderr,
        sender,
    );

    let started = Instant::now();
    let mut stdout = Vec::with_capacity(OUTPUT_LIMIT.min(8192));
    let mut stderr = Vec::with_capacity(OUTPUT_LIMIT.min(8192));
    let mut stdout_open = true;
    let mut stderr_open = true;
    let mut timed_out = false;
    let mut output_limited = false;
    loop {
        match receiver.recv_timeout(Duration::from_millis(20)) {
            Ok(PipeEvent::Data(stream, bytes)) => {
                let destination = match stream {
                    Stream::Stdout => &mut stdout,
                    Stream::Stderr => &mut stderr,
                };
                let remaining = OUTPUT_LIMIT.saturating_sub(destination.len());
                destination.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
                if bytes.len() > remaining {
                    output_limited = true;
                    break;
                }
            }
            Ok(PipeEvent::Eof(Stream::Stdout)) => stdout_open = false,
            Ok(PipeEvent::Eof(Stream::Stderr)) => stderr_open = false,
            Ok(PipeEvent::Error(stream, error)) => {
                let destination = match stream {
                    Stream::Stdout => &mut stdout,
                    Stream::Stderr => &mut stderr,
                };
                append_notice_bytes(destination, &format!("pipe read failed: {error}"));
                match stream {
                    Stream::Stdout => stdout_open = false,
                    Stream::Stderr => stderr_open = false,
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                stdout_open = false;
                stderr_open = false;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
        if !stdout_open && !stderr_open && leader_exited(child.id()) {
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            break;
        }
    }

    let status = terminate_and_reap(&mut child, process_group)?;
    drain_available(&receiver, &mut stdout, &mut stderr, &mut output_limited);
    Ok(CommandOutcome {
        status,
        timed_out,
        output_limited,
        stdout,
        stderr,
    })
}

fn spawn_pipe_reader<R: Read + Send + 'static>(
    mut reader: R,
    stream: Stream,
    sender: SyncSender<PipeEvent>,
) {
    thread::spawn(move || loop {
        let mut buffer = vec![0_u8; PIPE_CHUNK];
        match reader.read(&mut buffer) {
            Ok(0) => {
                let _ = sender.send(PipeEvent::Eof(stream));
                break;
            }
            Ok(read) => {
                buffer.truncate(read);
                if sender.send(PipeEvent::Data(stream, buffer)).is_err() {
                    break;
                }
            }
            Err(error) => {
                let _ = sender.send(PipeEvent::Error(stream, error.to_string()));
                break;
            }
        }
    });
}

fn terminate_and_reap(child: &mut Child, process_group: i32) -> Result<ExitStatus> {
    signal_group(process_group, libc::SIGTERM);
    let status = child
        .wait_timeout(TERMINATION_GRACE)
        .context("wait for command leader after SIGTERM")?;
    let status = if let Some(status) = status {
        status
    } else {
        signal_group(process_group, libc::SIGKILL);
        child
            .wait_timeout(REAP_GRACE)
            .context("wait for command leader after SIGKILL")?
            .context("command leader did not exit after SIGKILL")?
    };
    let deadline = Instant::now() + TERMINATION_GRACE;
    while Instant::now() < deadline && group_exists(process_group) {
        thread::sleep(Duration::from_millis(10));
    }
    if group_exists(process_group) {
        signal_group(process_group, libc::SIGKILL);
    }
    reap_adopted_group(process_group);
    Ok(status)
}

fn signal_group(process_group: i32, signal: i32) {
    // SAFETY: the child was placed in a new process group whose id is its pid.
    unsafe {
        libc::kill(-process_group, signal);
    }
}

fn group_exists(process_group: i32) -> bool {
    // SAFETY: signal zero performs existence/permission checking only.
    unsafe { libc::kill(-process_group, 0) == 0 }
}

fn reap_adopted_group(process_group: i32) {
    let deadline = Instant::now() + REAP_GRACE;
    loop {
        let mut status = 0;
        // SAFETY: negative pid restricts waitpid to adopted children in this command group.
        let result = unsafe { libc::waitpid(-process_group, &mut status, libc::WNOHANG) };
        if result < 0 {
            break;
        }
        if result == 0 {
            if Instant::now() >= deadline {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn enable_child_subreaper() -> Result<()> {
    // SAFETY: PR_SET_CHILD_SUBREAPER changes only descendant reparenting for this process.
    let result = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error()).context("enable child process reaping")
    }
}

fn leader_exited(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return true;
    };
    stat.rfind(')')
        .and_then(|close| stat.as_bytes().get(close + 2))
        .is_some_and(|state| *state == b'Z' || *state == b'X')
}

fn drain_available(
    receiver: &Receiver<PipeEvent>,
    stdout: &mut Vec<u8>,
    stderr: &mut Vec<u8>,
    output_limited: &mut bool,
) {
    while let Ok(event) = receiver.try_recv() {
        if let PipeEvent::Data(stream, bytes) = event {
            let destination = match stream {
                Stream::Stdout => &mut *stdout,
                Stream::Stderr => &mut *stderr,
            };
            let remaining = OUTPUT_LIMIT.saturating_sub(destination.len());
            destination.extend_from_slice(&bytes[..bytes.len().min(remaining)]);
            *output_limited |= bytes.len() > remaining;
        }
    }
}

fn apply_trusted_environment(command: &mut Command) {
    let home = std::env::var_os("HOME");
    let xdg_config = std::env::var_os("XDG_CONFIG_HOME");
    let xdg_state = std::env::var_os("XDG_STATE_HOME");
    let xdg_cache = std::env::var_os("XDG_CACHE_HOME");
    command.env_clear().env("PATH", FIXED_PATH).env("LANG", "C.UTF-8");
    for (name, value) in [
        ("HOME", home),
        ("XDG_CONFIG_HOME", xdg_config),
        ("XDG_STATE_HOME", xdg_state),
        ("XDG_CACHE_HOME", xdg_cache),
    ] {
        if let Some(value) = value {
            command.env(name, value);
        }
    }
    apply_test_environment(command);
}

#[cfg(debug_assertions)]
fn apply_test_environment(command: &mut Command) {
    for name in ["OMARCHY_TEST_LOG", "OMARCHY_MARKETPLACE_FIXTURE"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

#[cfg(not(debug_assertions))]
fn apply_test_environment(_command: &mut Command) {}

pub fn resolve_trusted_tool(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name.contains('/') {
        return None;
    }
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("OMARCHY_WORKBENCH_TEST_TOOLS") {
        if let Some(path) = executable_file(&PathBuf::from(root).join(name)) {
            return Some(path);
        }
    }
    let mut candidates = vec![
        PathBuf::from("/usr/local/bin").join(name),
        PathBuf::from("/usr/bin").join(name),
        PathBuf::from("/bin").join(name),
    ];
    if matches!(name, "omarchy" | "omarchy-shell") {
        if let Some(home) = std::env::var_os("HOME") {
            candidates.insert(0, PathBuf::from(home).join(".local/share/omarchy/bin").join(name));
        }
        candidates.insert(0, PathBuf::from("/usr/share/omarchy/bin").join(name));
    }
    candidates.into_iter().find_map(|path| executable_file(&path))
}

fn project_tool_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        return executable_file(Path::new(name));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(name))
        .find_map(|path| executable_file(&path))
}

fn executable_file(path: &Path) -> Option<PathBuf> {
    let canonical = path.canonicalize().ok()?;
    let metadata = canonical.metadata().ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(canonical)
}

fn reject_privilege_escalation(executable: &str) -> Result<()> {
    let name = Path::new(executable)
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or(executable);
    if matches!(name, "sudo" | "doas" | "su" | "pkexec") {
        bail!("privilege-escalation command is not allowed: {name}");
    }
    Ok(())
}

fn unavailable_tool() -> ToolResult {
    ToolResult {
        available: false,
        ok: false,
        exit_code: None,
        output: "not installed in a trusted executable location".to_owned(),
    }
}

fn tool_result(result: Result<CommandOutcome>) -> ToolResult {
    match result {
        Ok(outcome) => {
            let mut combined = outcome.stdout;
            combined.extend_from_slice(&outcome.stderr);
            let (mut output, _) = bounded_text(&combined);
            if outcome.output_limited {
                append_notice(&mut output, "output limit exceeded; process group terminated");
            }
            if outcome.timed_out {
                append_notice(&mut output, "deadline exceeded; process group terminated");
            }
            ToolResult {
                available: true,
                ok: !outcome.timed_out && !outcome.output_limited && outcome.status.success(),
                exit_code: outcome.status.code(),
                output: output.trim().to_owned(),
            }
        }
        Err(error) => ToolResult {
            available: true,
            ok: false,
            exit_code: None,
            output: format!("{error:#}"),
        },
    }
}

fn append_notice(output: &mut String, notice: &str) {
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    output.push_str(notice);
}

fn append_notice_bytes(output: &mut Vec<u8>, notice: &str) {
    if !output.is_empty() && !output.ends_with(b"\n") {
        output.push(b'\n');
    }
    let remaining = OUTPUT_LIMIT.saturating_sub(output.len());
    output.extend_from_slice(&notice.as_bytes()[..notice.len().min(remaining)]);
}

fn bounded_text(bytes: &[u8]) -> (String, bool) {
    let truncated = bytes.len() > OUTPUT_LIMIT;
    let slice = if truncated { &bytes[..OUTPUT_LIMIT] } else { bytes };
    (String::from_utf8_lossy(slice).into_owned(), truncated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn producer_quota_stops_an_unlimited_writer() {
        let temporary = tempdir().unwrap();
        let result = run_check(
            &CheckSpec {
                name: "unlimited-writer".to_owned(),
                argv: vec![
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    "while :; do printf 0123456789abcdef; done".to_owned(),
                ],
                timeout_seconds: 10,
            },
            temporary.path(),
            temporary.path(),
        )
        .unwrap();
        assert!(!result.ok);
        assert!(result.output_truncated);
        assert!(result.stderr.contains("output limit exceeded"));
        assert!(result.duration_ms < 5_000);
    }

    #[test]
    fn leader_exit_does_not_leave_a_live_descendant() {
        let temporary = tempdir().unwrap();
        let result = run_check(
            &CheckSpec {
                name: "background-descendant".to_owned(),
                argv: vec![
                    "/bin/sh".to_owned(),
                    "-c".to_owned(),
                    "(trap '' TERM; while :; do sleep 1; done) & exit 0".to_owned(),
                ],
                timeout_seconds: 1,
            },
            temporary.path(),
            temporary.path(),
        )
        .unwrap();
        assert!(!result.ok);
        assert!(result.timed_out);
        assert!(result.duration_ms < 5_000);
    }
}
