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
