use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScaffoldResult {
    pub path: PathBuf,
    pub kind: String,
    pub files: Vec<String>,
    pub git_initialized: bool,
    pub git_warning: Option<String>,
}

pub fn create(target: &Path, id: &str, name: &str, kind: &str) -> Result<ScaffoldResult> {
    if !target.is_absolute() {
        bail!("new plugin path must be absolute");
    }
    if target.exists() || target.is_symlink() {
        bail!("new plugin path already exists: {}", target.display());
    }
    validate_name(name)?;
    let parent = target.parent().context("new plugin path has no parent")?;
    let parent = parent
        .canonicalize()
        .with_context(|| format!("resolve new plugin parent {}", parent.display()))?;
    if !parent.is_dir() {
        bail!("new plugin parent is not a directory: {}", parent.display());
    }
    let file_name = target
        .file_name()
        .context("new plugin path has no directory name")?;
    let target = parent.join(file_name);
    if target.exists() || target.is_symlink() {
        bail!("new plugin path already exists: {}", target.display());
    }

    let staging = parent.join(format!(
        ".workbench-new-{}-{}",
        std::process::id(),
        crate::registry::now_unix()
    ));
    if staging.exists() || staging.is_symlink() {
        bail!(
            "new plugin staging path already exists: {}",
            staging.display()
        );
    }
    fs::create_dir(&staging).context("create new plugin staging directory")?;
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o700))?;

    let result = (|| -> Result<(Vec<String>, String)> {
        let (kinds, entry_points, mut files, display_kind) = template(kind, id, name)?;
        if kind == "panel" {
            for (path, contents) in &mut files {
                if path.as_str() == "BarWidget.qml" {
                    *contents = contents.replace(
                        "  readonly property bool opened: panelLoader.item ? panelLoader.item.opened === true : false\n",
                        "  readonly property bool opened: panelLoader.item ? panelLoader.item.opened === true : false\n  implicitWidth: button.implicitWidth\n  implicitHeight: button.implicitHeight\n",
                    );
                } else if path.as_str() == "Panel.qml" {
                    *contents = contents.replace(
                        "  KeyboardPanel {\n    anchorItem:",
                        "  KeyboardPanel {\n    id: panel\n    anchorItem:",
                    );
                }
            }
        }
        let manifest = json!({
            "schemaVersion": 1,
            "id": id,
            "name": name,
            "version": "0.1.0",
            "description": format!("A personal Omarchy {display_kind} plugin."),
            "kinds": kinds,
            "entryPoints": entry_points
        });
        write_new(
            &staging.join("manifest.json"),
            format!("{}\n", serde_json::to_string_pretty(&manifest)?).as_bytes(),
            0o644,
        )?;
        write_new(
            &staging.join(".omarchy-workbench.json"),
            b"{\n  \"schemaVersion\": 1,\n  \"pluginPath\": \".\",\n  \"checks\": []\n}\n",
            0o644,
        )?;
        write_new(&staging.join(".gitignore"), b".DS_Store\n*.log\n", 0o644)?;
        let readme = format!(
            "# {name}\n\nA personal Omarchy {display_kind} plugin created with Discovery Build.\n\n## Build\n\nRegister, validate, live-link and test this checkout from Discovery.\n"
        );
        write_new(&staging.join("README.md"), readme.as_bytes(), 0o644)?;
        for (path, contents) in &files {
            write_new(&staging.join(path), contents.as_bytes(), 0o644)?;
        }
        crate::manifest::validate_plugin(&staging).context("validate generated plugin")?;
        let mut created = vec![
            "manifest.json".to_owned(),
            ".omarchy-workbench.json".to_owned(),
            ".gitignore".to_owned(),
            "README.md".to_owned(),
        ];
        created.extend(files.into_iter().map(|(path, _)| path));
        Ok((created, display_kind))
    })();

    let (files, _) = match result {
        Ok(value) => value,
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
    };
    fs::rename(&staging, &target).context("publish new plugin project")?;

    let git = crate::process::capture_tool(
        "git",
        &["-C", &target.to_string_lossy(), "init", "-b", "main"],
        None,
    );
    Ok(ScaffoldResult {
        path: target,
        kind: kind.to_owned(),
        files,
        git_initialized: git.available && git.ok,
        git_warning: (!git.available || !git.ok).then_some(if git.output.is_empty() {
            "Git is unavailable; initialise the project repository manually".to_owned()
        } else {
            format!("Git repository was not initialised: {}", git.output)
        }),
    })
}

type Template = (
    Vec<&'static str>,
    serde_json::Value,
    Vec<(String, String)>,
    String,
);

fn template(kind: &str, id: &str, name: &str) -> Result<Template> {
    let title = serde_json::to_string(name)?;
    match kind {
        "bar-widget" => Ok((
            vec!["bar-widget"],
            json!({"barWidget": "BarWidget.qml"}),
            vec![(
                "BarWidget.qml".to_owned(),
                format!(
                    "import QtQuick\nimport qs.Commons\nimport qs.Ui\n\nBarWidget {{\n  id: root\n  moduleName: \"{id}\"\n  implicitWidth: button.implicitWidth\n  implicitHeight: button.implicitHeight\n\n  WidgetButton {{\n    id: button\n    anchors.fill: parent\n    bar: root.bar\n    text: {title}\n    tooltipText: {title}\n  }}\n}}\n"
                ),
            )],
            "bar widget".to_owned(),
        )),
        "panel" => Ok((
            vec!["bar-widget", "panel"],
            json!({"barWidget": "BarWidget.qml", "panel": "Panel.qml"}),
            vec![
                (
                    "BarWidget.qml".to_owned(),
                    format!(
                        "import QtQuick\nimport qs.Commons\nimport qs.Ui\n\nBarWidget {{\n  id: root\n  moduleName: \"{id}\"\n  readonly property bool opened: panelLoader.item ? panelLoader.item.opened === true : false\n\n  Loader {{\n    id: panelLoader\n    active: true\n    source: Qt.resolvedUrl(\"Panel.qml\")\n    visible: false\n    onLoaded: {{\n      panelLoader.item.bar = root.bar\n      panelLoader.item.anchorItem = button\n      panelLoader.item.hostWidget = root\n    }}\n  }}\n\n  WidgetButton {{\n    id: button\n    anchors.fill: parent\n    bar: root.bar\n    text: {title}\n    tooltipText: {title}\n    onPressed: function(buttonCode) {{\n      if (buttonCode === Qt.LeftButton && panelLoader.item) panelLoader.item.toggle()\n    }}\n  }}\n}}\n"
                    ),
                ),
                (
                    "Panel.qml".to_owned(),
                    format!(
                        "import QtQuick\nimport qs.Commons\nimport qs.Ui\n\nPanel {{\n  id: root\n  moduleName: \"{id}\"\n  manageIpc: false\n  property var anchorItem: null\n  property var hostWidget: null\n\n  function toggle() {{\n    if (root.opened) root.controller.hide()\n    else root.controller.show()\n  }}\n\n  KeyboardPanel {{\n    anchorItem: root.anchorItem\n    owner: root.hostWidget || root\n    bar: root.bar\n    open: root.opened\n    contentWidth: panel.fittedContentWidth(Style.space(420))\n    contentHeight: panel.fittedContentHeight(Style.space(280))\n\n    Rectangle {{\n      anchors.fill: parent\n      color: root.bar ? root.bar.background : Color.background\n\n      Text {{\n        anchors.centerIn: parent\n        text: {title}\n        color: root.bar ? root.bar.foreground : Color.foreground\n        font.pixelSize: Style.font.title\n      }}\n    }}\n  }}\n}}\n"
                    ),
                ),
            ],
            "panel".to_owned(),
        )),
        "service" => Ok((
            vec!["service"],
            json!({"service": "Service.qml"}),
            vec![(
                "Service.qml".to_owned(),
                "import QtQuick\n\nItem {\n  id: root\n  property var shell: null\n}\n".to_owned(),
            )],
            "service".to_owned(),
        )),
        _ => bail!("plugin kind must be panel, bar-widget, or service"),
    }
}

fn validate_name(name: &str) -> Result<()> {
    if name.trim() != name
        || name.is_empty()
        || name.len() > 80
        || name
            .chars()
            .any(|character| matches!(character, '\n' | '\r'))
    {
        bail!("plugin name must be 1-80 trimmed characters on one line");
    }
    Ok(())
}

fn write_new(path: &Path, contents: &[u8], mode: u32) -> Result<()> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(mode)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(contents)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn creates_each_supported_plugin_kind() {
        let root = tempdir().unwrap();
        for kind in ["panel", "bar-widget", "service"] {
            let target = root.path().join(kind);
            let result =
                create(&target, &format!("io.test.{kind}"), "Personal Plugin", kind).unwrap();
            assert_eq!(result.path, target);
            assert!(result.files.contains(&"manifest.json".to_owned()));
            if kind == "panel" {
                assert!(result.files.contains(&"BarWidget.qml".to_owned()));
                assert!(result.files.contains(&"Panel.qml".to_owned()));
            }
            assert_eq!(
                crate::manifest::validate_plugin(&target).unwrap().id,
                format!("io.test.{kind}")
            );
        }
    }

    #[test]
    fn never_replaces_an_existing_path() {
        let root = tempdir().unwrap();
        let target = root.path().join("existing");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), "mine").unwrap();
        assert!(create(&target, "io.test.existing", "Existing", "panel").is_err());
        assert_eq!(fs::read_to_string(target.join("keep")).unwrap(), "mine");
    }
}
