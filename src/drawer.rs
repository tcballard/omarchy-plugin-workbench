use crate::process::capture_tool;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const ID: &str = "spencerbull.drawer";

pub fn status() -> Result<Value> {
    let inventory = capture_tool("omarchy", &["plugin", "list", "--json"], None);
    if !inventory.ok {
        return Ok(json!({"ok":true,"supported":false,"reason":"Omarchy inventory unavailable"}));
    }
    let inventory: Value = serde_json::from_str(&inventory.output)?;
    if !inventory.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["id"] == ID && item["enabled"] == true)
    }) {
        return Ok(json!({"ok":true,"supported":false,"reason":"Drawer is not enabled"}));
    }
    let result = capture_tool("omarchy-shell", &[ID, "status"], None);
    if !result.ok {
        return Ok(json!({"ok":true,"supported":false,"reason":"Drawer interface unavailable"}));
    }
    let mut value: Value = serde_json::from_str(&result.output).context("invalid Drawer status")?;
    if value["version"] != 1 || !value["profiles"].is_array() {
        bail!("unsupported Drawer interface version");
    }
    let profiles = value["profiles"]
        .as_array()
        .context("invalid Drawer profiles")?;
    if profiles.len() > 32
        || profiles.iter().any(|p| {
            p["id"].as_str().is_none_or(|s| s.len() > 256)
                || p["name"].as_str().is_none_or(|s| s.len() > 240)
        })
    {
        bail!("invalid Drawer profiles");
    }
    value["ok"] = Value::Bool(true);
    Ok(value)
}

pub fn configure(profile: Option<&str>) -> Result<Value> {
    let state = status()?;
    if state["supported"] != true {
        bail!("Drawer requires a compatible enabled host extension");
    }
    let mut args = vec![ID, "open"];
    if let Some(id) = profile {
        if !state["profiles"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == id))
        {
            bail!("unknown Drawer profile");
        }
        args = vec![ID, "selectProfile", id];
    }
    let result = capture_tool("omarchy-shell", &args, None);
    if !result.ok || result.output.trim() == "false" {
        bail!("Drawer rejected request: {}", result.output);
    }
    Ok(json!({"ok":true,"message":"Drawer request applied"}))
}
