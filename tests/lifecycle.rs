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
        let mut search_path = vec![tools.to_path_buf()];
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
            .env(
                "OMARCHY_MARKETPLACE_FIXTURE",
                self.root.path().join("marketplace-fixture"),
            )
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

    fn import_security_review(
        &self,
        result: &str,
        findings: Value,
        blockers: Value,
        artifacts: Value,
    ) -> Value {
        let revision = git(&self.project, &["rev-parse", "HEAD"]);
        let report = self
            .root
            .path()
            .join(format!("security-review-{revision}-{result}.json"));
        fs::write(
            &report,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schemaVersion": 1,
                "projectId": "io.test.workbench-demo",
                "revision": revision,
                "result": result,
                "reviewer": "Workbench integration test",
                "reviewedAtUnix": 0,
                "findings": findings,
                "confirmedFixes": [],
                "remainingBlockers": blockers,
                "residualRisks": ["manual review evidence is represented by this fixture"],
                "untestedAreas": [],
                "commandsNotRun": ["all repository-provided executable code"],
                "executableArtifacts": artifacts
            }))
            .unwrap(),
        )
        .unwrap();
        self.json(&[
            "security-review-import",
            "io.test.workbench-demo",
            "--file",
            &report.to_string_lossy(),
            "--confirm-manual-review",
            "--json",
        ])
    }

    fn import_ready_security_review(&self) -> Value {
        self.import_security_review(
            "ready",
            Value::Array(Vec::new()),
            Value::Array(Vec::new()),
            Value::Array(Vec::new()),
        )
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

const MARKETPLACE_ID: &str = "io.test.marketplace";
const MARKETPLACE_REPO: &str = "https://github.com/acme/reviewed-plugin";
const MARKETPLACE_REVISION: &str = "0123456789abcdef0123456789abcdef01234567";

fn write_marketplace_catalog(harness: &Harness) -> PathBuf {
    write_marketplace_catalog_at(harness, MARKETPLACE_REVISION)
}

fn write_marketplace_catalog_at(harness: &Harness, reviewed_revision: &str) -> PathBuf {
    let cache = harness
        .home
        .join(".local/state/omarchy/plugin-workbench/marketplace/catalog.json");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(
        &cache,
        serde_json::to_vec(&serde_json::json!({
            "generatedAt": "2026-09-01T12:00:00Z",
            "stateSchemaVersion": 2,
            "mode": "production",
            "warnings": [],
            "plugins": [
                {
                    "id": MARKETPLACE_ID,
                    "name": "Reviewed Search Tool",
                    "description": "Search and test Omarchy plugins",
                    "author": "Workbench Tests",
                    "version": "1.2.3",
                    "category": "Development",
                    "tags": ["search", "testing"],
                    "kind": "panel",
                    "status": "active",
                    "repo": MARKETPLACE_REPO,
                    "sourceType": "community",
                    "builtIn": false,
                    "installAvailable": true,
                    "repositoryLayout": "root-plugin",
                    "verificationStatus": "verified",
                    "verificationSnapshotStatus": "current",
                    "verificationCoverage": "full",
                    "listingValidatedCommit": reviewed_revision,
                    "listingValidatedAt": "2026-09-01T11:00:00Z",
                    "repositoryUpdatedAt": "2026-09-01T10:00:00Z",
                    "stars": 42
                },
                {
                    "id": "io.omarchy.builtin-example",
                    "name": "Built-in Example",
                    "description": "A plugin supplied by Omarchy",
                    "author": "Omarchy",
                    "version": "1.0.0",
                    "category": "System",
                    "tags": ["builtin"],
                    "kind": "service",
                    "status": "active",
                    "repo": "https://github.com/basecamp/omarchy",
                    "sourceType": "builtin",
                    "builtIn": true,
                    "installAvailable": false,
                    "verificationStatus": "verified"
                }
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    cache
}

fn fake_marketplace_git(harness: &Harness, tools: &Path) {
    let fixture = harness.root.path().join("marketplace-fixture");
    fs::create_dir_all(&fixture).unwrap();
    fs::write(
        fixture.join("manifest.json"),
        format!(
            r#"{{
              "schemaVersion": 1,
              "id": "{MARKETPLACE_ID}",
              "name": "Reviewed Search Tool",
              "version": "1.2.3",
              "kinds": ["panel"],
              "entryPoints": {{"panel": "Panel.qml"}}
            }}"#
        ),
    )
    .unwrap();
    fs::write(fixture.join("Panel.qml"), "import QtQuick\nItem {}\n").unwrap();

    let git = tools.join("git");
    fs::write(
        &git,
        format!(
            r#"#!/bin/sh
printf 'git %s\n' "$*" >> "$OMARCHY_TEST_LOG"
action=
for argument in "$@"; do
  case "$argument" in
    clone|checkout|rev-parse|status|fetch|merge-base|merge|reset) action="$argument" ;;
  esac
  destination="$argument"
done
case "$action" in
  clone)
    mkdir -p "$destination/.git"
    cp -R "$OMARCHY_MARKETPLACE_FIXTURE"/. "$destination"/
    printf '%s\n' "{MARKETPLACE_REVISION}" > "$destination/.git/workbench-head"
    ;;
  checkout|merge|reset) printf '%s\n' "$destination" > .git/workbench-head ;;
  rev-parse) cat .git/workbench-head ;;
  status) : ;;
esac
"#
        ),
    )
    .unwrap();
    fs::set_permissions(&git, fs::Permissions::from_mode(0o755)).unwrap();
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
    git(
        &harness.project,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/acme/workbench-demo.git",
        ],
    );
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
    assert!(blocked["blockers"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item.as_str().unwrap().contains("security review")));

    harness.json(&["check", "io.test.workbench-demo", "--json"]);
    harness.import_ready_security_review();
    let ready = harness.json(&["release-check", "io.test.workbench-demo", "--json"]);
    assert_eq!(ready["ok"], true);
    assert_eq!(ready["version"], "0.1.0");
    assert_eq!(ready["clean"], true);
    assert_eq!(ready["currentRevisionHasReadySecurityReview"], true);
    assert_eq!(ready["securityReviewStatus"], "ready");
    assert_eq!(ready["tagExists"], false);

    let plan = harness.json(&["release-plan", "io.test.workbench-demo", "--json"]);
    assert_eq!(plan["ok"], true);
    assert_eq!(plan["tag"], "v0.1.0");
    assert_eq!(plan["repository"], "https://github.com/acme/workbench-demo");
    assert!(PathBuf::from(plan["planFile"].as_str().unwrap()).is_file());
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

#[test]
fn marketplace_searches_the_cached_official_catalogue_and_marks_installed_plugins() {
    let harness = Harness::new();
    write_marketplace_catalog(&harness);

    let search = harness.json(&[
        "marketplace-search",
        "search testing",
        "--verified",
        "--json",
    ]);
    assert_eq!(search["total"], 2);
    assert_eq!(search["matched"], 1);
    assert_eq!(search["plugins"][0]["id"], MARKETPLACE_ID);
    assert_eq!(
        search["plugins"][0]["reviewedRevision"],
        MARKETPLACE_REVISION
    );
    assert_eq!(search["plugins"][0]["installable"], true);

    fs::create_dir_all(
        harness
            .home
            .join(format!(".config/omarchy/plugins/{MARKETPLACE_ID}")),
    )
    .unwrap();
    let installed = harness.json(&["marketplace-search", "--installed", "--json"]);
    assert_eq!(installed["matched"], 2);
    let installed_plugin = installed["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["id"] == MARKETPLACE_ID)
        .unwrap();
    assert_eq!(installed_plugin["installed"], true);
    assert_eq!(installed_plugin["installable"], false);

    let built_in = harness.json(&["marketplace-search", "--built-in", "--json"]);
    assert_eq!(built_in["matched"], 1);
    assert_eq!(built_in["plugins"][0]["id"], "io.omarchy.builtin-example");
    assert_eq!(built_in["plugins"][0]["builtIn"], true);
}

#[test]
fn marketplace_installs_and_enables_only_the_exact_reviewed_revision() {
    let harness = Harness::new();
    write_marketplace_catalog(&harness);
    let (tools, log) = fake_omarchy_tools(&harness, true);
    fake_marketplace_git(&harness, &tools);

    let output = harness.run_with_tools(
        &[
            "marketplace-install",
            MARKETPLACE_ID,
            "--repo",
            MARKETPLACE_REPO,
            "--revision",
            MARKETPLACE_REVISION,
            "--enable",
            "--yes",
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["revision"], MARKETPLACE_REVISION);
    assert_eq!(report["installed"], true);
    assert_eq!(report["enabled"], true);
    assert!(
        harness
            .home
            .join(format!(
                ".config/omarchy/plugins/{MARKETPLACE_ID}/Panel.qml"
            ))
            .is_file()
    );
    let calls = fs::read_to_string(log).unwrap();
    assert!(calls.contains(&format!("checkout --detach {MARKETPLACE_REVISION}")));
    assert!(calls.contains("omarchy plugin validate"));
    assert!(calls.contains("omarchy-shell shell rescanPlugins"));
    assert!(calls.contains(&format!("omarchy plugin enable {MARKETPLACE_ID}")));
}

#[test]
fn marketplace_install_refuses_missing_confirmation_and_stale_review() {
    let harness = Harness::new();
    write_marketplace_catalog(&harness);
    let (tools, log) = fake_omarchy_tools(&harness, true);
    fake_marketplace_git(&harness, &tools);

    let unconfirmed = harness.run_with_tools(
        &[
            "marketplace-install",
            MARKETPLACE_ID,
            "--repo",
            MARKETPLACE_REPO,
            "--revision",
            MARKETPLACE_REVISION,
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(!unconfirmed.status.success());
    let error: Value = serde_json::from_slice(&unconfirmed.stdout).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("explicit confirmation")
    );

    let stale_revision = "ffffffffffffffffffffffffffffffffffffffff";
    let stale = harness.run_with_tools(
        &[
            "marketplace-install",
            MARKETPLACE_ID,
            "--repo",
            MARKETPLACE_REPO,
            "--revision",
            stale_revision,
            "--yes",
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(!stale.status.success());
    let error: Value = serde_json::from_slice(&stale.stdout).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("changed since review")
    );
    assert!(!log.exists() || !fs::read_to_string(&log).unwrap().contains("git "));
}

#[test]
fn marketplace_rejects_a_symlinked_catalogue_cache() {
    let harness = Harness::new();
    let cache = write_marketplace_catalog(&harness);
    let external = harness.root.path().join("external-catalog.json");
    fs::rename(&cache, &external).unwrap();
    symlink(&external, &cache).unwrap();

    let output = harness.run(&["marketplace-search", "--json"]);
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        error["error"]
            .as_str()
            .unwrap()
            .contains("not a normal file")
    );
}

#[test]
fn marketplace_receipts_drive_verified_updates_and_exclude_generic_updates() {
    let harness = Harness::new();
    write_marketplace_catalog(&harness);
    let (tools, log) = fake_omarchy_tools(&harness, true);
    fake_marketplace_git(&harness, &tools);
    let installed = harness.run_with_tools(
        &[
            "marketplace-install",
            MARKETPLACE_ID,
            "--repo",
            MARKETPLACE_REPO,
            "--revision",
            MARKETPLACE_REVISION,
            "--yes",
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(installed.status.success());
    let receipt = harness.home.join(format!(
        ".local/state/omarchy/plugin-workbench/marketplace/receipts/{MARKETPLACE_ID}.json"
    ));
    assert!(receipt.is_file());

    let next = "89abcdef0123456789abcdef0123456789abcdef";
    write_marketplace_catalog_at(&harness, next);
    let managed = harness.run_with_tools(&["marketplace-managed", "--json"], &tools, &log);
    assert!(managed.status.success());
    let managed: Value = serde_json::from_slice(&managed.stdout).unwrap();
    assert_eq!(managed["updatesAvailable"], 1);
    assert_eq!(managed["plugins"][0]["catalogueRevision"], next);

    let generic = harness.run_with_tools(&["updates", "--json"], &tools, &log);
    assert!(generic.status.success());
    let generic: Value = serde_json::from_slice(&generic.stdout).unwrap();
    assert_eq!(generic["plugins"], Value::Array(Vec::new()));

    let updated = harness.run_with_tools(
        &[
            "marketplace-update",
            MARKETPLACE_ID,
            "--revision",
            next,
            "--yes",
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stdout)
    );
    let updated: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(updated["revision"], next);
    let receipt: Value = serde_json::from_slice(&fs::read(receipt).unwrap()).unwrap();
    assert_eq!(receipt["installedRevision"], next);
}

#[test]
fn marketplace_repair_and_uninstall_retain_recovery_copies() {
    let harness = Harness::new();
    write_marketplace_catalog(&harness);
    let (tools, log) = fake_omarchy_tools(&harness, true);
    fake_marketplace_git(&harness, &tools);
    let installed = harness.run_with_tools(
        &[
            "marketplace-install",
            MARKETPLACE_ID,
            "--repo",
            MARKETPLACE_REPO,
            "--revision",
            MARKETPLACE_REVISION,
            "--yes",
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(installed.status.success());
    let target = harness
        .home
        .join(format!(".config/omarchy/plugins/{MARKETPLACE_ID}"));
    fs::write(target.join("damaged.txt"), "recover me").unwrap();

    let repaired = harness.run_with_tools(
        &["marketplace-repair", MARKETPLACE_ID, "--yes", "--json"],
        &tools,
        &log,
    );
    assert!(repaired.status.success());
    let repaired: Value = serde_json::from_slice(&repaired.stdout).unwrap();
    let repair_backup = PathBuf::from(repaired["retainedBackup"].as_str().unwrap());
    assert_eq!(
        fs::read_to_string(repair_backup.join("damaged.txt")).unwrap(),
        "recover me"
    );

    let removed = harness.run_with_tools(
        &["marketplace-uninstall", MARKETPLACE_ID, "--yes", "--json"],
        &tools,
        &log,
    );
    assert!(removed.status.success());
    let removed: Value = serde_json::from_slice(&removed.stdout).unwrap();
    assert!(!target.exists());
    assert!(PathBuf::from(removed["retainedBackup"].as_str().unwrap()).is_dir());
    assert!(
        !harness
            .home
            .join(format!(
                ".local/state/omarchy/plugin-workbench/marketplace/receipts/{MARKETPLACE_ID}.json"
            ))
            .exists()
    );
}

#[test]
fn marketplace_lifecycle_refuses_symlink_target_drift() {
    let harness = Harness::new();
    write_marketplace_catalog(&harness);
    let (tools, log) = fake_omarchy_tools(&harness, true);
    fake_marketplace_git(&harness, &tools);
    let installed = harness.run_with_tools(
        &[
            "marketplace-install",
            MARKETPLACE_ID,
            "--repo",
            MARKETPLACE_REPO,
            "--revision",
            MARKETPLACE_REVISION,
            "--yes",
            "--json",
        ],
        &tools,
        &log,
    );
    assert!(installed.status.success());
    let target = harness
        .home
        .join(format!(".config/omarchy/plugins/{MARKETPLACE_ID}"));
    let external = harness.root.path().join("external-managed-plugin");
    fs::rename(&target, &external).unwrap();
    symlink(&external, &target).unwrap();

    for action in ["marketplace-repair", "marketplace-uninstall"] {
        let output =
            harness.run_with_tools(&[action, MARKETPLACE_ID, "--yes", "--json"], &tools, &log);
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(
            error["error"]
                .as_str()
                .unwrap()
                .contains("not a normal directory")
        );
        assert!(target.is_symlink());
        assert!(external.join("Panel.qml").is_file());
    }
}

#[test]
fn submission_prepare_emits_the_current_official_form_without_publishing() {
    let harness = Harness::new();
    write_marketplace_catalog(&harness);
    fs::write(
        harness.project.join("README.md"),
        "Install and remove instructions",
    )
    .unwrap();
    fs::write(harness.project.join("LICENSE"), "MIT").unwrap();
    harness.initialise_git();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);
    harness.import_ready_security_review();

    let draft = harness.json(&[
        "submission-prepare",
        "io.test.workbench-demo",
        "--repo",
        "https://github.com/acme/workbench-demo",
        "--category",
        "Developer Tools",
        "--tag",
        "quickshell",
        "--tag",
        "bar",
        "--confirm-checklist",
        "--json",
    ]);
    assert_eq!(draft["ok"], true);
    assert_eq!(draft["title"], "[Plugin]: Workbench Demo");
    assert!(
        draft["body"]
            .as_str()
            .unwrap()
            .contains("### Submission checklist")
    );
    assert!(PathBuf::from(draft["draftFile"].as_str().unwrap()).is_file());
    assert_eq!(draft["securityReviewStatus"], "ready");
}

#[test]
fn security_review_is_read_only_exact_commit_bound_and_stales_on_change() {
    let harness = Harness::new();
    harness.initialise_git();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);

    let prepared = harness.json(&[
        "security-review-prepare",
        "io.test.workbench-demo",
        "--json",
    ]);
    let revision = git(&harness.project, &["rev-parse", "HEAD"]);
    assert_eq!(prepared["revision"], revision);
    assert_eq!(prepared["inventory"]["truncated"], false);
    let prompt = fs::read_to_string(prepared["promptFile"].as_str().unwrap()).unwrap();
    assert!(prompt.contains("Remain read-only"));
    assert!(prompt.contains("Do not run plugin code, tests, builds"));
    assert!(prompt.contains(&revision));

    let imported = harness.import_ready_security_review();
    assert_eq!(imported["status"], "ready");
    let current = harness.json(&[
        "security-review-status",
        "io.test.workbench-demo",
        "--json",
    ]);
    assert_eq!(current["status"], "ready");

    fs::write(
        harness.project.join("Panel.qml"),
        "import QtQuick\nItem { objectName: \"changed\" }\n",
    )
    .unwrap();
    let stale = harness.json(&[
        "security-review-status",
        "io.test.workbench-demo",
        "--json",
    ]);
    assert_eq!(stale["status"], "stale");
    assert_eq!(stale["ready"], false);
}

#[test]
fn ready_security_review_requires_provenance_for_every_executable() {
    let harness = Harness::new();
    let executable = harness.project.join("helper");
    fs::write(&executable, b"\x7fELFfixture").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
    harness.initialise_git();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);

    let rejected_report = harness.root.path().join("missing-provenance.json");
    let revision = git(&harness.project, &["rev-parse", "HEAD"]);
    fs::write(
        &rejected_report,
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1,
            "projectId": "io.test.workbench-demo",
            "revision": revision,
            "result": "ready",
            "reviewer": "Workbench integration test",
            "reviewedAtUnix": 0,
            "findings": [],
            "confirmedFixes": [],
            "remainingBlockers": [],
            "residualRisks": [],
            "untestedAreas": [],
            "commandsNotRun": ["all executable code"],
            "executableArtifacts": []
        }))
        .unwrap(),
    )
    .unwrap();
    let rejected = harness.run(&[
        "security-review-import",
        "io.test.workbench-demo",
        "--file",
        &rejected_report.to_string_lossy(),
        "--confirm-manual-review",
        "--json",
    ]);
    assert!(!rejected.status.success());
    let error: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert!(error["error"].as_str().unwrap().contains("omits executable artifact 'helper'"));

    let accepted = harness.import_security_review(
        "ready",
        Value::Array(Vec::new()),
        Value::Array(Vec::new()),
        serde_json::json!([{
            "path": "helper",
            "kind": "elf",
            "status": "attested",
            "evidence": "attestation fixture bound to the exact reviewed source"
        }]),
    );
    assert_eq!(accepted["status"], "ready");
}

#[test]
fn fix_review_brief_requires_prior_findings_and_a_new_commit() {
    let harness = Harness::new();
    harness.initialise_git();
    let project = harness.project.to_string_lossy().into_owned();
    harness.json(&["add", &project, "--json"]);
    harness.import_security_review(
        "needs-fixes",
        serde_json::json!([{
            "id": "SEC-001",
            "severity": "high",
            "file": "Panel.qml",
            "line": 2,
            "summary": "Untrusted content reaches a sensitive sink",
            "untrustedSource": "remote label",
            "sensitiveSink": "QML rich text",
            "attackPath": "remote response to rendered label",
            "impact": "markup injection",
            "remediation": "render as plain text",
            "verification": "inspect every dynamic Text sink"
        }]),
        serde_json::json!(["SEC-001 remains open"]),
        Value::Array(Vec::new()),
    );

    fs::write(
        harness.project.join("Panel.qml"),
        "import QtQuick\nItem { property int textFormat: Text.PlainText }\n",
    )
    .unwrap();
    git(&harness.project, &["add", "Panel.qml"]);
    git(&harness.project, &["commit", "-m", "fix security finding"]);
    let prepared = harness.json(&[
        "security-review-prepare",
        "io.test.workbench-demo",
        "--verify-fixes",
        "--json",
    ]);
    assert_eq!(prepared["verifyFixes"], true);
    let prompt = fs::read_to_string(prepared["promptFile"].as_str().unwrap()).unwrap();
    assert!(prompt.contains("fix-verification review"));
    assert!(prompt.contains("confirmed`, `partial`, or `not-fixed"));
}
