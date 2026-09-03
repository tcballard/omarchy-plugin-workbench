use crate::marketplace;
use crate::paths::AppPaths;
use crate::process::capture_tool;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledReport {
    pub ok: bool,
    pub count: usize,
    pub plugins: Vec<Value>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnabledReport {
    pub ok: bool,
    pub action: &'static str,
    pub plugin_id: String,
    pub enabled: bool,
    pub message: String,
}

pub fn inspect(paths: &AppPaths) -> Result<InstalledReport> {
    let discovered = capture_tool("omarchy", &["plugin", "list", "--json"], None);
    if !discovered.available {
        bail!("omarchy is unavailable in a trusted executable location");
    }
    if !discovered.ok {
        bail!("omarchy plugin inventory failed: {}", discovered.output);
    }
    let value: Value = serde_json::from_str(&discovered.output)
        .context("omarchy plugin inventory did not return valid JSON")?;
    let mut plugins = value
        .as_array()
        .context("omarchy plugin inventory must be an array")?
        .clone();
    let managed = marketplace::managed(paths)?;
    let managed_by_id = managed
        .plugins
        .iter()
        .map(|plugin| (plugin.id.as_str(), plugin))
        .collect::<HashMap<_, _>>();

    for plugin in &mut plugins {
        let object = plugin
            .as_object_mut()
            .context("omarchy plugin inventory entries must be objects")?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .context("omarchy plugin inventory entry is missing id")?
            .to_owned();
        let first_party = object
            .get("firstParty")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let managed_plugin = managed_by_id.get(id.as_str()).copied();
        let target = paths.plugins_dir.join(&id);
        let management = if first_party {
            "first-party"
        } else if managed_plugin.is_some() {
            "marketplace"
        } else if target.is_symlink() {
            "live-link"
        } else if target.join(".git").is_dir() {
            "git"
        } else {
            "local"
        };
        object.insert(
            "management".to_owned(),
            Value::String(management.to_owned()),
        );
        if let Some(managed_plugin) = managed_plugin {
            object.insert(
                "managedState".to_owned(),
                Value::String(managed_plugin.state.clone()),
            );
            object.insert(
                "installedRevision".to_owned(),
                Value::String(managed_plugin.installed_revision.clone()),
            );
            object.insert(
                "catalogueRevision".to_owned(),
                managed_plugin
                    .catalogue_revision
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            object.insert(
                "updateAvailable".to_owned(),
                Value::Bool(managed_plugin.update_available),
            );
            object.insert(
                "managementError".to_owned(),
                managed_plugin
                    .error
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
        }
    }

    plugins.sort_by_key(|plugin| {
        plugin
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| plugin.get("id").and_then(Value::as_str))
            .unwrap_or("")
            .to_ascii_lowercase()
    });
    Ok(InstalledReport {
        ok: managed.ok,
        count: plugins.len(),
        plugins,
    })
}

pub fn set_enabled(paths: &AppPaths, id: &str, enabled: bool) -> Result<EnabledReport> {
    let installed = inspect(paths)?;
    if !installed
        .plugins
        .iter()
        .any(|plugin| plugin.get("id").and_then(Value::as_str) == Some(id))
    {
        bail!("plugin '{id}' is not in Omarchy's installed inventory");
    }
    let action = if enabled { "enable" } else { "disable" };
    let result = capture_tool("omarchy", &["plugin", action, id], None);
    if !result.available {
        bail!("omarchy is unavailable in a trusted executable location");
    }
    if !result.ok {
        bail!("could not {action} plugin '{id}': {}", result.output);
    }
    Ok(EnabledReport {
        ok: true,
        action,
        plugin_id: id.to_owned(),
        enabled,
        message: format!("{} {id}", if enabled { "Enabled" } else { "Disabled" }),
    })
}
