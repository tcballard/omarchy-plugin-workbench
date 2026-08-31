use serde_json::Value;
use std::fs;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::PathBuf;
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
