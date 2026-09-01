use serde_json::Value;
use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

struct Harness {
    root: TempDir,
    home: PathBuf,
    project: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let project = root.path().join("demo");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project).unwrap();
        fs::write(
            project.join("manifest.json"),
            r#"{
              "schemaVersion": 1,
              "id": "io.test.workbench-demo",
              "name": "Workbench Demo",
              "version": "0.1.0",
              "kinds": ["panel"],
              "entryPoints": {"panel": "Panel.qml"}
            }"#,
        )
        .unwrap();
        fs::write(project.join("Panel.qml"), "import QtQuick\nItem {}\n").unwrap();
        Self {
            root,
            home,
            project,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_omarchy-plugin-workbench"))
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .output()
            .unwrap()
    }

    fn json(&self, args: &[&str]) -> Value {
        let output = self.run(args);
        assert!(
            output.status.success(),
            "command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn run_with_tools(&self, args: &[&str], tools: &Path, log: &Path) -> Output {
        let mut search_path = vec![tools.clone()];
        search_path.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(search_path).unwrap();
        Command::new(env!("CARGO_BIN_EXE_omarchy-plugin-workbench"))
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("PATH", path)
            .env("OMARCHY_TEST_LOG", log)
            .output()
            .unwrap()
    }

    fn installed_target(&self) -> PathBuf {
        self.home
            .join(".config/omarchy/plugins/io.test.workbench-demo")
    }

    fn initialise_git(&self) {
        let run = |args: &[&str]| {
            let output = Command::new("git")
                .args(args)
                .current_dir(&self.project)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.name", "Workbench Test"]);
        run(&["config", "user.email", "workbench@example.invalid"]);
        run(&["add", "."]);
        run(&["commit", "-m", "initial"]);
    }
}

fn git(cwd: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn setup_installed_update(harness: &Harness) -> (PathBuf, String, String) {
    harness.initialise_git();
    let origin = harness.root.path().join("origin.git");
    fs::create_dir_all(&origin).unwrap();
    git(&origin, &["init", "--bare", "--initial-branch=main"]);
    git(
        &harness.project,
        &["remote", "add", "origin", &origin.to_string_lossy()],
    );
    git(&harness.project, &["push", "-u", "origin", "main"]);
    fs::create_dir_all(harness.installed_target().parent().unwrap()).unwrap();
    git(
        harness.installed_target().parent().unwrap(),
        &[
            "clone",
            &origin.to_string_lossy(),
            &harness.installed_target().to_string_lossy(),
        ],
    );
    let current = git(&harness.installed_target(), &["rev-parse", "HEAD"]);

    fs::write(
        harness.project.join("Panel.qml"),
        "import QtQuick\nItem { objectName: \"updated\" }\n",
    )
    .unwrap();
    git(&harness.project, &["add", "Panel.qml"]);
    git(&harness.project, &["commit", "-m", "show reviewed update"]);
    git(&harness.project, &["push", "origin", "main"]);
    let remote = git(&harness.project, &["rev-parse", "HEAD"]);
    (origin, current, remote)
}

fn fake_omarchy_tools(harness: &Harness, validation_succeeds: bool) -> (PathBuf, PathBuf) {
    let tools = harness.root.path().join("fake-tools");
    let log = harness.root.path().join("tool.log");
    fs::create_dir_all(&tools).unwrap();
    let omarchy = tools.join("omarchy");
    fs::write(
        &omarchy,
        format!(
            "#!/bin/sh\nprintf 'omarchy %s\\n' \"$*\" >> \"$OMARCHY_TEST_LOG\"\nexit {}\n",
            if validation_succeeds { 0 } else { 1 }
        ),
    )
    .unwrap();
    fs::set_permissions(&omarchy, fs::Permissions::from_mode(0o755)).unwrap();
    let shell = tools.join("omarchy-shell");
    fs::write(
        &shell,
        "#!/bin/sh\nprintf 'omarchy-shell %s\\n' \"$*\" >> \"$OMARCHY_TEST_LOG\"\n",
    )
    .unwrap();
    fs::set_permissions(&shell, fs::Permissions::from_mode(0o755)).unwrap();
    (tools, log)
}

#[test]
fn register_link_snapshot_rollback_and_undeploy() {
    let harness = Harness::new();
    let canonical_project = harness.project.canonicalize().unwrap();
    let project = harness.project.to_string_lossy().into_owned();
    let added = harness.json(&["add", &project, "--json"]);
    assert_eq!(added["ok"], true);

    let status = harness.json(&["status", "--json"]);
    assert_eq!(status[0]["deployment"], "not-deployed");

    let linked = harness.json(&["link", "io.test.workbench-demo", "--json"]);
    assert_eq!(linked["action"], "link");
    assert_eq!(
        fs::read_link(harness.installed_target()).unwrap(),
        canonical_project
    );

    fs::write(
        harness.project.join("Panel.qml"),
        "import QtQuick\nItem { objectName: \"snapshot\" }\n",
    )
    .unwrap();
    let snapped = harness.json(&["snapshot", "io.test.workbench-demo", "--json"]);
    assert_eq!(snapped["action"], "snapshot");
    let snapshot_target = fs::read_link(harness.installed_target()).unwrap();
    assert_ne!(snapshot_target, canonical_project);
    assert!(snapshot_target.starts_with(harness.home.join(".local/state")));

    let rolled_back = harness.json(&["rollback", "io.test.workbench-demo", "--json"]);
    assert_eq!(rolled_back["action"], "rollback");
    assert_eq!(
        fs::read_link(harness.installed_target()).unwrap(),
        canonical_project
    );

    harness.json(&["undeploy", "io.test.workbench-demo", "--json"]);
    assert!(!harness.installed_target().exists());
    harness.json(&["remove", "io.test.workbench-demo", "--json"]);
    assert_eq!(harness.json(&["list", "--json"]), Value::Array(Vec::new()));

    assert!(harness.root.path().exists());
}

#[test]
fn refuses_to_replace_an_unmanaged_install() {
    let harness = Harness::new();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);
    fs::create_dir_all(harness.installed_target()).unwrap();
    fs::write(harness.installed_target().join("keep.txt"), "mine").unwrap();

    let output = harness.run(&["link", "io.test.workbench-demo", "--json"]);
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(error["error"].as_str().unwrap().contains("unmanaged"));
    assert_eq!(
        fs::read_to_string(harness.installed_target().join("keep.txt")).unwrap(),
        "mine"
    );
}

#[test]
fn installed_target_is_a_symlink_not_a_special_device() {
    let harness = Harness::new();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);
    harness.json(&["link", "io.test.workbench-demo", "--json"]);
    let file_type = fs::symlink_metadata(harness.installed_target())
        .unwrap()
        .file_type();
    assert!(file_type.is_symlink());
    assert!(!file_type.is_char_device());
}

#[test]
fn project_checks_require_an_explicit_trust_decision() {
    let harness = Harness::new();
    fs::create_dir_all(harness.project.join("tests")).unwrap();
    let test_runner = harness.project.join("tests/run");
    fs::write(&test_runner, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&test_runner, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        harness.project.join(".omarchy-workbench.json"),
        r#"{
          "schemaVersion": 1,
          "pluginPath": ".",
          "checks": [
            {"name": "project-tests", "argv": ["./tests/run"], "timeoutSeconds": 300}
          ]
        }"#,
    )
    .unwrap();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);

    let before = harness.json(&["status", "--json"]);
    assert_eq!(before[0]["checks"], 1);
    assert_eq!(before[0]["projectChecksTrusted"], false);

    let refused = harness.run(&["check", "io.test.workbench-demo", "--json"]);
    assert!(!refused.status.success());
    let error: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert!(error["error"].as_str().unwrap().contains("not trusted"));

    harness.json(&["trust", "io.test.workbench-demo", "--json"]);
    let report = harness.json(&["check", "io.test.workbench-demo", "--json"]);
    assert_eq!(report["ok"], true);
    assert_eq!(report["results"].as_array().unwrap().len(), 2);
    assert_eq!(report["results"][1]["argv"][0], "./tests/run");
}

#[test]
fn doctor_detects_builder_only_from_a_managed_receipt() {
    let harness = Harness::new();
    let skills = harness.home.join(".agents/skills");
    fs::create_dir_all(&skills).unwrap();
    let receipt = skills.join(".build-omarchy-plugins-receipt.json");
    fs::write(
        &receipt,
        r#"{
          "schemaVersion": 1,
          "manager": "build-omarchy-plugins",
          "source": {
            "repository": "https://github.com/tcballard/build-omarchy-plugins",
            "version": "0.2.3"
          },
          "skills": {"omarchy-plugin-scaffold": {}}
        }"#,
    )
    .unwrap();

    let report = harness.json(&["doctor", "--json"]);
    let companion = &report["builderCompanion"];
    assert_eq!(companion["detected"], true);
    assert_eq!(companion["supportedProjectSchema"], 1);
    assert_eq!(companion["installations"][0]["target"], "agents/codex");
    assert_eq!(companion["installations"][0]["version"], "0.2.3");
    assert_eq!(
        companion["installations"][0]["receipt"],
        receipt.display().to_string()
    );
}

#[test]
fn workflows_are_capability_gated_and_definition_changes_revoke_trust() {
    let harness = Harness::new();
    fs::create_dir_all(harness.project.join("scripts")).unwrap();
    let preview = harness.project.join("scripts/preview");
    fs::write(&preview, "#!/bin/sh\nprintf 'preview-ready\\n'\n").unwrap();
    fs::set_permissions(&preview, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        harness.project.join(".omarchy-workbench.json"),
        r#"{
          "schemaVersion": 1,
          "pluginPath": ".",
          "environment": [
            {"name": "git", "argv": ["git", "--version"], "required": true}
          ],
          "workflows": [
            {"name": "preview", "capability": "preview", "argv": ["./scripts/preview"]}
          ]
        }"#,
    )
    .unwrap();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);

    let refused = harness.run(&["workflow", "io.test.workbench-demo", "preview", "--json"]);
    assert!(!refused.status.success());
    harness.json(&["trust", "io.test.workbench-demo", "--json"]);
    harness.json(&["approve", "io.test.workbench-demo", "preview", "--json"]);
    let environment = harness.json(&["environment", "io.test.workbench-demo", "--json"]);
    assert_eq!(environment["ok"], true);
    let workflow = harness.json(&["workflow", "io.test.workbench-demo", "preview", "--json"]);
    assert_eq!(workflow["ok"], true);
    assert_eq!(workflow["result"]["stdout"], "preview-ready\n");

    let evidence = harness.json(&["evidence", "io.test.workbench-demo", "--json"]);
    assert_eq!(evidence[0]["kind"], "workflow");
    fs::write(
        harness.project.join(".omarchy-workbench.json"),
        r#"{"schemaVersion":1,"pluginPath":".","workflows":[]}"#,
    )
    .unwrap();
    let changed = harness.run(&["workflow", "io.test.workbench-demo", "preview", "--json"]);
    assert!(!changed.status.success());
    let error: Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("changed since trust")
    );
    let refreshed = harness.json(&["refresh", "io.test.workbench-demo", "--json"]);
    assert_eq!(
        refreshed["project"]["workflows"].as_array().unwrap().len(),
        0
    );
    assert_eq!(refreshed["project"]["projectChecksTrusted"], false);
    assert!(
        refreshed["project"]["approvedCapabilities"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn isolated_sessions_and_handoffs_remain_agent_neutral() {
    let harness = Harness::new();
    harness.initialise_git();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);
    let started = harness.json(&[
        "session-start",
        "io.test.workbench-demo",
        "--task",
        "fix-preview",
        "--agent",
        "opencode",
        "--objective",
        "Repair the preview workflow",
        "--json",
    ]);
    let session = &started["session"];
    assert_eq!(session["branch"], "codex/fix-preview");
    assert_eq!(session["agent"], "opencode");
    assert!(PathBuf::from(session["worktree"].as_str().unwrap()).is_dir());
    let session_id = session["id"].as_str().unwrap();
    let handoff = harness.json(&[
        "handoff",
        session_id,
        "--decision",
        "Keep the contract agent-neutral",
        "--next-action",
        "Run the project checks",
        "--json",
    ]);
    assert_eq!(handoff["handoff"]["projectId"], "io.test.workbench-demo");
    assert_eq!(
        handoff["handoff"]["decisions"][0],
        "Keep the contract agent-neutral"
    );
    harness.json(&["session-close", session_id, "--json"]);
    let sessions = harness.json(&["sessions", "io.test.workbench-demo", "--json"]);
    assert!(sessions[0]["closedAtUnix"].is_number());
    assert!(PathBuf::from(sessions[0]["worktree"].as_str().unwrap()).is_dir());
}

#[test]
fn release_readiness_requires_current_clean_check_evidence() {
    let harness = Harness::new();
    fs::write(
        harness.project.join("CHANGELOG.md"),
        "# Changelog\n\n## 0.1.0\n",
    )
    .unwrap();
    harness.initialise_git();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);

    let before = harness.run(&["release-check", "io.test.workbench-demo", "--json"]);
    assert!(!before.status.success());
    let blocked: Value = serde_json::from_slice(&before.stdout).unwrap();
    assert!(
        blocked["blockers"][0]
            .as_str()
            .unwrap()
            .contains("passing check evidence")
    );

    harness.json(&["check", "io.test.workbench-demo", "--json"]);
    let ready = harness.json(&["release-check", "io.test.workbench-demo", "--json"]);
    assert_eq!(ready["ok"], true);
    assert_eq!(ready["version"], "0.1.0");
    assert_eq!(ready["clean"], true);
    assert_eq!(ready["tagExists"], false);
}

#[test]
fn update_review_reports_the_pinned_revision_commits_and_diff_stat() {
    let harness = Harness::new();
    let (_origin, current, remote) = setup_installed_update(&harness);

    let report = harness.json(&["updates", "--json"]);
    assert_eq!(report["ok"], true);
    assert_eq!(report["available"], 1);
    assert_eq!(report["blocked"], 0);
    let update = &report["plugins"][0];
    assert_eq!(update["id"], "io.test.workbench-demo");
    assert_eq!(update["state"], "update-available");
    assert_eq!(update["updateable"], true);
    assert_eq!(update["currentRevision"], current);
    assert_eq!(update["remoteRevision"], remote);
    assert_eq!(update["behind"], 1);
    assert_eq!(update["commits"][0]["subject"], "show reviewed update");
    assert!(update["diffStat"].as_str().unwrap().contains("Panel.qml"));
}

#[test]
fn dirty_and_live_link_plugins_are_never_offered_for_update() {
    let harness = Harness::new();
    setup_installed_update(&harness);
    fs::write(harness.installed_target().join("local.txt"), "keep me").unwrap();

    let report = harness.json(&["updates", "--json"]);
    assert_eq!(report["available"], 0);
    assert_eq!(report["blocked"], 1);
    assert_eq!(report["plugins"][0]["state"], "dirty");
    assert_eq!(report["plugins"][0]["updateable"], false);

    fs::remove_dir_all(harness.installed_target()).unwrap();
    symlink(&harness.project, harness.installed_target()).unwrap();
    let linked = harness.json(&["updates", "--json"]);
    assert_eq!(linked["plugins"], Value::Array(Vec::new()));
}

#[test]
fn update_applies_only_the_reviewed_revision_then_validates_and_rescans() {
    let harness = Harness::new();
    let (_origin, _current, remote) = setup_installed_update(&harness);
    let (tools, log) = fake_omarchy_tools(&harness, true);

    let output = harness.run_with_tools(
        &[
            "update",
            "io.test.workbench-demo",
            "--revision",
            &remote,
            "--yes",
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["updated"][0], "io.test.workbench-demo");
    assert_eq!(
        git(&harness.installed_target(), &["rev-parse", "HEAD"]),
        remote
    );
    let calls = fs::read_to_string(log).unwrap();
    assert!(calls.contains("omarchy plugin validate"));
    assert!(calls.contains("omarchy-shell shell rescanPlugins"));
}

#[test]
fn update_refuses_stale_review_and_rolls_back_failed_validation() {
    let harness = Harness::new();
    let (_origin, current, remote) = setup_installed_update(&harness);
    let (tools, log) = fake_omarchy_tools(&harness, false);

    let stale = harness.run_with_tools(
        &[
            "update",
            "io.test.workbench-demo",
            "--revision",
            &"0".repeat(40),
            "--yes",
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(!stale.status.success());
    let stale_error: Value = serde_json::from_slice(&stale.stdout).unwrap();
    assert!(
        stale_error["error"]
            .as_str()
            .unwrap()
            .contains("changed since review")
    );
    assert_eq!(
        git(&harness.installed_target(), &["rev-parse", "HEAD"]),
        current
    );

    let failed = harness.run_with_tools(
        &[
            "update",
            "io.test.workbench-demo",
            "--revision",
            &remote,
            "--yes",
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(!failed.status.success());
    let failure: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert!(failure["error"].as_str().unwrap().contains("rolled back"));
    assert_eq!(
        git(&harness.installed_target(), &["rev-parse", "HEAD"]),
        current
    );
    assert!(!fs::read_to_string(log).unwrap().contains("omarchy-shell"));
}
