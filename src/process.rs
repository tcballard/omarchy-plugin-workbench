use crate::model::{CheckResult, CheckSpec, ToolResult};
use anyhow::{Context, Result, bail};
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;

const OUTPUT_LIMIT: usize = 64 * 1024;

pub fn command_exists(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|directory| {
        let candidate = directory.join(name);
        candidate.is_file()
    })
}

pub fn capture_tool(command: &str, args: &[&str], cwd: Option<&Path>) -> ToolResult {
    if !command_exists(command) {
        return ToolResult {
            available: false,
            ok: false,
            exit_code: None,
            output: "not installed".to_owned(),
        };
    }
    let mut invocation = Command::new(command);
    invocation.args(args);
    if let Some(cwd) = cwd {
        invocation.current_dir(cwd);
    }
    match invocation.output() {
        Ok(output) => {
            let mut combined = output.stdout;
            combined.extend_from_slice(&output.stderr);
            let (text, _) = bounded_text(&combined);
            ToolResult {
                available: true,
                ok: output.status.success(),
                exit_code: output.status.code(),
                output: text.trim().to_owned(),
            }
        }
        Err(error) => ToolResult {
            available: true,
            ok: false,
            exit_code: None,
            output: error.to_string(),
        },
    }
}

pub fn run_check(check: &CheckSpec, cwd: &Path, temporary_dir: &Path) -> Result<CheckResult> {
    let executable = check.argv.first().context("empty check argv")?;
    if matches!(executable.as_str(), "sudo" | "doas" | "su" | "pkexec") {
        bail!("privilege-escalation command is not allowed: {executable}");
    }
    fs::create_dir_all(temporary_dir).context("create check output directory")?;
    let nonce = format!("{}-{}", std::process::id(), crate::registry::now_unix());
    let stdout_path = temporary_dir.join(format!("{nonce}.stdout"));
    let stderr_path = temporary_dir.join(format!("{nonce}.stderr"));
    let stdout_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&stdout_path)
        .context("create bounded stdout file")?;
    let stderr_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&stderr_path)
        .context("create bounded stderr file")?;

    let started = Instant::now();
    let mut command = Command::new(executable);
    command
        .args(&check.argv[1..])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout_file))
        .stderr(Stdio::from(stderr_file));
    command.process_group(0);
    let mut child = command
        .spawn()
        .with_context(|| format!("start check '{}'", check.name))?;
    let status = child
        .wait_timeout(Duration::from_secs(check.timeout_seconds))
        .context("wait for project check")?;
    let (exit_code, timed_out) = if let Some(status) = status {
        (status.code(), false)
    } else {
        let process_group = -(child.id() as i32);
        // SAFETY: process_group is the negative id of the child group created above.
        unsafe {
            libc::kill(process_group, libc::SIGTERM);
        }
        thread::sleep(Duration::from_millis(250));
        if child.try_wait()?.is_none() {
            // SAFETY: same process group, now force-stopped after a grace period.
            unsafe {
                libc::kill(process_group, libc::SIGKILL);
            }
        }
        let status = child.wait().context("reap timed-out project check")?;
        (status.code(), true)
    };
    let duration_ms = started.elapsed().as_millis();
    let (stdout, stdout_truncated) = read_bounded(&stdout_path)?;
    let (stderr, stderr_truncated) = read_bounded(&stderr_path)?;
    let _ = fs::remove_file(&stdout_path);
    let _ = fs::remove_file(&stderr_path);
    let ok = !timed_out && exit_code == Some(0);
    Ok(CheckResult {
        name: check.name.clone(),
        argv: check.argv.clone(),
        ok,
        exit_code,
        timed_out,
        duration_ms,
        stdout,
        stderr,
        output_truncated: stdout_truncated || stderr_truncated,
    })
}

fn read_bounded(path: &Path) -> Result<(String, bool)> {
    let mut file = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(OUTPUT_LIMIT.min(8192));
    file.by_ref()
        .take((OUTPUT_LIMIT + 1) as u64)
        .read_to_end(&mut bytes)?;
    Ok(bounded_text(&bytes))
}

fn bounded_text(bytes: &[u8]) -> (String, bool) {
    let truncated = bytes.len() > OUTPUT_LIMIT;
    let slice = if truncated {
        &bytes[..OUTPUT_LIMIT]
    } else {
        bytes
    };
    (String::from_utf8_lossy(slice).into_owned(), truncated)
}
