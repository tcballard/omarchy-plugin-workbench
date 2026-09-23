use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

#[test]
fn doctor_uses_supported_version_command_and_preserves_desktop_context() {
    let root = tempfile::tempdir().unwrap();
    let tools = root.path().join("tools");
    let home = root.path().join("home");
    fs::create_dir_all(&tools).unwrap();
    fs::create_dir_all(&home).unwrap();
    for (name, body) in [
        ("omarchy", "test \"$1\" = version || exit 42\nprintf '4.0.3\\n'"),
        ("omarchy-shell", "test \"$XDG_RUNTIME_DIR\" = /test/runtime || exit 43\nexit 0"),
        ("hyprctl", "test \"$HYPRLAND_INSTANCE_SIGNATURE\" = test-instance || exit 44\ntest \"$XDG_RUNTIME_DIR\" = /test/runtime || exit 45\ntest \"${BASH_ENV-unset}\" = unset || exit 46\nexit 0"),
        ("Hyprland", "test \"$WAYLAND_DISPLAY\" = wayland-test || exit 47\nexit 0"),
    ] {
        let path = tools.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_omarchy-plugin-workbench"))
        .args(["doctor", "--json"])
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_STATE_HOME", home.join(".local/state"))
        .env("OMARCHY_WORKBENCH_TEST_TOOLS", &tools)
        .env("XDG_RUNTIME_DIR", "/test/runtime")
        .env("HYPRLAND_INSTANCE_SIGNATURE", "test-instance")
        .env("WAYLAND_DISPLAY", "wayland-test")
        .env("BASH_ENV", "/test/untrusted")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    for name in ["omarchy", "omarchy-shell", "hyprctl", "Hyprland"] {
        assert_eq!(report["tools"][name]["ok"], true, "{name}: {}", report["tools"][name]);
    }
    assert_eq!(report["ok"], true);
}
