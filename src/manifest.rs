use crate::model::OMARCHY_MANIFEST_SCHEMA;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};
use walkdir::WalkDir;

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ValidatedManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kinds: Vec<String>,
}

pub fn validate_plugin(plugin_root: &Path) -> Result<ValidatedManifest> {
    let root = plugin_root
        .canonicalize()
        .with_context(|| format!("resolve plugin root {}", plugin_root.display()))?;
    if !root.is_dir() {
        bail!("plugin root is not a directory: {}", root.display());
    }
    reject_symlinks_and_special_files(&root)?;

    let manifest_path = root.join("manifest.json");
    let meta = fs::metadata(&manifest_path)
        .with_context(|| format!("missing manifest.json in {}", root.display()))?;
    if !meta.is_file() {
        bail!("manifest.json is not a regular file");
    }
    if meta.len() > MAX_MANIFEST_BYTES {
        bail!("manifest.json exceeds {MAX_MANIFEST_BYTES} bytes");
    }
    let bytes = fs::read(&manifest_path).context("read manifest.json")?;
    let value: Value = serde_json::from_slice(&bytes).context("manifest.json is not valid JSON")?;
    let object = value
        .as_object()
        .context("manifest must be a JSON object")?;

    let schema = object
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .context("manifest schemaVersion must be the number 1")?;
    if schema != u64::from(OMARCHY_MANIFEST_SCHEMA) {
        bail!("unsupported manifest schemaVersion {schema}; expected 1");
    }
    let id = required_string(object, "id")?;
    validate_id(&id)?;
    let name = required_string(object, "name")?;
    let version = required_string(object, "version")?;
    let kinds_value = object.get("kinds").context("manifest missing kinds")?;
    let kinds_array = kinds_value
        .as_array()
        .context("manifest kinds must be an array")?;
    if kinds_array.is_empty() {
        bail!("manifest kinds must not be empty");
    }
    let kinds = kinds_array
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .context("manifest kind must be a string")
        })
        .collect::<Result<Vec<_>>>()?;

    let entry_points = object
        .get("entryPoints")
        .and_then(Value::as_object)
        .context("manifest entryPoints must be an object")?;
    for (key, value) in entry_points {
        let relative = value
            .as_str()
            .with_context(|| format!("entryPoints.{key} must be a string"))?;
        validate_entry_point(&root, key, relative)?;
    }
    for (kind, key) in [
        ("bar", "bar"),
        ("bar-widget", "barWidget"),
        ("menu", "menu"),
        ("overlay", "overlay"),
        ("panel", "panel"),
        ("service", "service"),
    ] {
        if kinds.iter().any(|candidate| candidate == kind) && !entry_points.contains_key(key) {
            bail!("kind '{kind}' requires entryPoints.{key}");
        }
    }
    if let Some(section) = object
        .get("barWidget")
        .and_then(Value::as_object)
        .and_then(|v| v.get("defaultSection"))
    {
        let section = section
            .as_str()
            .context("barWidget.defaultSection must be a string")?;
        if !matches!(section, "left" | "center" | "right") {
            bail!("barWidget.defaultSection must be left, center, or right");
        }
    }

    Ok(ValidatedManifest {
        id,
        name,
        version,
        kinds,
    })
}

fn required_string(object: &serde_json::Map<String, Value>, field: &str) -> Result<String> {
    let value = object
        .get(field)
        .with_context(|| format!("manifest missing {field}"))?
        .as_str()
        .with_context(|| format!("manifest {field} must be a string"))?;
    if value.is_empty() {
        bail!("manifest {field} must not be empty");
    }
    Ok(value.to_owned())
}

fn validate_id(id: &str) -> Result<()> {
    if id.starts_with("omarchy.") {
        bail!("plugin id '{id}' uses the reserved omarchy.* namespace");
    }
    if id.contains("..")
        || !id.chars().enumerate().all(|(index, c)| {
            c.is_ascii_alphanumeric() || (index > 0 && matches!(c, '.' | '_' | '-'))
        })
    {
        bail!("invalid plugin id '{id}'");
    }
    Ok(())
}

fn validate_entry_point(root: &Path, key: &str, relative: &str) -> Result<()> {
    if relative.is_empty() || relative.contains('\n') {
        bail!("entryPoints.{key} is empty or contains a newline");
    }
    let path = PathBuf::from(relative);
    if path.is_absolute()
        || path.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("entryPoints.{key} must be a safe relative path");
    }
    let full = root.join(path);
    let meta = fs::symlink_metadata(&full)
        .with_context(|| format!("entry point file not found: '{relative}'"))?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        bail!("entry point is not a regular file: '{relative}'");
    }
    Ok(())
}

fn reject_symlinks_and_special_files(root: &Path) -> Result<()> {
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| {
            !(entry.path() != root && entry.file_name() == ".git" && entry.file_type().is_dir())
        })
    {
        let entry = entry.with_context(|| format!("walk plugin tree at {}", root.display()))?;
        let kind = entry.file_type();
        if kind.is_symlink() {
            bail!(
                "symlinks are not allowed inside a plugin: {}",
                entry.path().display()
            );
        }
        if !kind.is_file() && !kind.is_dir() {
            bail!(
                "special files are not allowed inside a plugin: {}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    fn valid_plugin() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("manifest.json"),
            r#"{"schemaVersion":1,"id":"io.test.demo","name":"Demo","version":"0.1.0","kinds":["panel"],"entryPoints":{"panel":"Panel.qml"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("Panel.qml"), "import QtQuick\nItem {}\n").unwrap();
        dir
    }

    #[test]
    fn accepts_schema_one_plugin() {
        let dir = valid_plugin();
        let manifest = validate_plugin(dir.path()).unwrap();
        assert_eq!(manifest.id, "io.test.demo");
    }

    #[test]
    fn rejects_reserved_id() {
        let dir = valid_plugin();
        let manifest = fs::read_to_string(dir.path().join("manifest.json")).unwrap();
        fs::write(
            dir.path().join("manifest.json"),
            manifest.replace("io.test.demo", "omarchy.demo"),
        )
        .unwrap();
        assert!(
            validate_plugin(dir.path())
                .unwrap_err()
                .to_string()
                .contains("reserved")
        );
    }

    #[test]
    fn rejects_symlink_anywhere_in_tree() {
        let dir = valid_plugin();
        symlink("Panel.qml", dir.path().join("Alias.qml")).unwrap();
        assert!(
            validate_plugin(dir.path())
                .unwrap_err()
                .to_string()
                .contains("symlink")
        );
    }
}
