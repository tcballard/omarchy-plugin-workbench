use serde_json::Value;
use std::fs;
use std::os::unix::fs::FileTypeExt;
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
    let project = harness.project.to_string_lossy().into_owned();
    let added = harness.json(&["add", &project, "--json"]);
    assert_eq!(added["ok"], true);

    let status = harness.json(&["status", "--json"]);
    assert_eq!(status[0]["deployment"], "not-deployed");

    let linked = harness.json(&["link", "io.test.workbench-demo", "--json"]);
    assert_eq!(linked["action"], "link");
    assert_eq!(
        fs::read_link(harness.installed_target()).unwrap(),
        harness.project
    );

    fs::write(
        harness.project.join("Panel.qml"),
        "import QtQuick\nItem { objectName: \"snapshot\" }\n",
    )
    .unwrap();
    let snapped = harness.json(&["snapshot", "io.test.workbench-demo", "--json"]);
    assert_eq!(snapped["action"], "snapshot");
    let snapshot_target = fs::read_link(harness.installed_target()).unwrap();
    assert_ne!(snapshot_target, harness.project);
    assert!(snapshot_target.starts_with(harness.home.join(".local/state")));

    let rolled_back = harness.json(&["rollback", "io.test.workbench-demo", "--json"]);
    assert_eq!(rolled_back["action"], "rollback");
    assert_eq!(
        fs::read_link(harness.installed_target()).unwrap(),
        harness.project
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
    fs::write(
        harness.project.join(".omarchy-workbench.json"),
        r#"{
          "schemaVersion": 1,
          "pluginPath": ".",
          "checks": [
            {"name": "truth", "argv": ["/bin/true"], "timeoutSeconds": 5}
          ]
        }"#,
    )
    .unwrap();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);

    let refused = harness.run(&["check", "io.test.workbench-demo", "--json"]);
    assert!(!refused.status.success());
    let error: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert!(error["error"].as_str().unwrap().contains("not trusted"));

    harness.json(&["trust", "io.test.workbench-demo", "--json"]);
    let report = harness.json(&["check", "io.test.workbench-demo", "--json"]);
    assert_eq!(report["ok"], true);
    assert_eq!(report["results"].as_array().unwrap().len(), 2);
}
