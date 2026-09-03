use crate::marketplace::{self, SearchFilters};
use crate::model::{CheckSpec, ToolResult};
use crate::paths::{AppPaths, secure_dir};
use crate::process::{capture_tool, command_exists, run_trusted_check};
use crate::registry::now_unix;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::env;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const APP_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/tcballard/omarchy-apps/main/data/registry.json";
const APP_CATALOG_SCHEMA: u32 = 1;
const MAX_APP_CATALOG_BYTES: u64 = 2 * 1024 * 1024;
const MAX_ITEMS: usize = 5_000;
const MAX_SEARCH_LIMIT: usize = 100;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppCatalog {
    schema_version: u32,
    updated_at: String,
    apps: Vec<AppEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AppEntry {
    id: String,
    name: String,
    summary: String,
    description: String,
    category: String,
    maturity: String,
    integration: String,
    source_url: String,
    install: AppInstall,
    #[serde(default)]
    architectures: Vec<String>,
    verified_on: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AppInstall {
    channel: String,
    package: Option<String>,
    command: Option<String>,
    status: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshReport {
    pub ok: bool,
    pub generated_at: String,
    pub plugin_listings: usize,
    pub app_listings: usize,
    pub theme_listings: usize,
    pub warnings: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryItem {
    pub id: String,
    pub name: String,
    pub description: String,
    pub flavor: String,
    pub category: String,
    pub version: String,
    pub author: String,
    pub tags: Vec<String>,
    pub status: String,
    pub source: String,
    pub installed: bool,
    pub installable: bool,
    pub managed: bool,
    pub update_available: bool,
    pub verified: bool,
    pub built_in: bool,
    pub package: Option<String>,
    pub reviewed_revision: Option<String>,
    pub reviewed_at: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReport {
    pub ok: bool,
    pub generated_at: String,
    pub flavor: String,
    pub total: usize,
    pub matched: usize,
    pub returned: usize,
    pub items: Vec<DiscoveryItem>,
    pub warnings: Vec<String>,
}

pub struct SearchOptions<'a> {
    pub query: Option<&'a str>,
    pub flavor: &'a str,
    pub verified_only: bool,
    pub installable_only: bool,
    pub installed_only: bool,
    pub limit: usize,
}

pub fn refresh(paths: &AppPaths) -> Result<RefreshReport> {
    let mut warnings = Vec::new();
    let mut plugin_listings = 0;
    let mut app_listings = 0;
    let mut generated_at = String::new();

    match marketplace::refresh(paths) {
        Ok(report) => {
            plugin_listings = report.plugins;
            generated_at = report.generated_at;
        }
        Err(error) => warnings.push(format!("Plugin catalogue: {error:#}")),
    }

    match refresh_apps(paths) {
        Ok(catalog) => {
            app_listings = catalog.apps.len();
            if generated_at.is_empty() {
                generated_at = catalog.updated_at;
            }
        }
        Err(error) => warnings.push(format!("App catalogue: {error:#}")),
    }

    let theme_listings = theme_items(paths).len();
    if plugin_listings == 0 && app_listings == 0 && theme_listings == 0 {
        bail!("Discovery could not load any catalogue source: {}", warnings.join("; "));
    }
    Ok(RefreshReport {
        ok: warnings.is_empty(),
        generated_at,
        plugin_listings,
        app_listings,
        theme_listings,
        message: format!(
            "Discovery refreshed: {app_listings} apps, {plugin_listings} plugins, {theme_listings} themes"
        ),
        warnings,
    })
}

pub fn search(paths: &AppPaths, options: &SearchOptions<'_>) -> Result<SearchReport> {
    if options.limit == 0 || options.limit > MAX_SEARCH_LIMIT {
        bail!("Discovery search limit must be between 1 and {MAX_SEARCH_LIMIT}");
    }
    if !matches!(options.flavor, "all" | "app" | "plugin" | "theme") {
        bail!("Discovery flavor must be all, app, plugin, or theme");
    }

    let mut items = Vec::new();
    let mut warnings = Vec::new();
    let mut generated_at = String::new();

    if matches!(options.flavor, "all" | "plugin") {
        match marketplace::search(
            paths,
            &SearchFilters {
                query: options.query,
                category: None,
                tag: None,
                kind: None,
                built_in_only: false,
                verified_only: options.verified_only,
                installable_only: options.installable_only,
                installed_only: options.installed_only,
                limit: MAX_SEARCH_LIMIT,
            },
        ) {
            Ok(report) => {
                generated_at = report.generated_at;
                items.extend(report.plugins.into_iter().map(|plugin| DiscoveryItem {
                    id: plugin.id,
                    name: plugin.name,
                    description: plugin.description,
                    flavor: "plugin".to_owned(),
                    category: plugin.category,
                    version: plugin.version,
                    author: plugin.author,
                    tags: plugin.tags,
                    status: plugin.status,
                    source: plugin.repo,
                    installed: plugin.installed,
                    installable: plugin.installable,
                    managed: plugin.managed,
                    update_available: plugin.update_available,
                    verified: plugin.verification_status == "verified",
                    built_in: plugin.built_in,
                    package: None,
                    reviewed_revision: plugin.reviewed_revision,
                    reviewed_at: plugin.reviewed_at,
                }));
            }
            Err(error) => warnings.push(format!("Plugin catalogue: {error:#}")),
        }
    }

    if matches!(options.flavor, "all" | "app") {
        match read_app_catalog(paths) {
            Ok(catalog) => {
                if generated_at.is_empty() {
                    generated_at = catalog.updated_at.clone();
                }
                items.extend(catalog.apps.into_iter().map(app_item));
            }
            Err(error) => warnings.push(format!("App catalogue: {error:#}")),
        }
    }

    if matches!(options.flavor, "all" | "theme") {
        items.extend(theme_items(paths));
    }

    let query = options.query.unwrap_or_default().trim().to_ascii_lowercase();
    items.retain(|item| {
        (query.is_empty()
            || format!(
                "{} {} {} {} {}",
                item.name,
                item.description,
                item.category,
                item.author,
                item.tags.join(" ")
            )
            .to_ascii_lowercase()
            .contains(&query))
            && (!options.verified_only || item.verified)
            && (!options.installable_only || item.installable)
            && (!options.installed_only || item.installed)
    });
    items.sort_by(|left, right| {
        right
            .installed
            .cmp(&left.installed)
            .then_with(|| left.flavor.cmp(&right.flavor))
            .then_with(|| left.name.to_ascii_lowercase().cmp(&right.name.to_ascii_lowercase()))
    });
    let matched = items.len();
    items.truncate(options.limit);
    let returned = items.len();
    Ok(SearchReport {
        ok: warnings.is_empty(),
        generated_at,
        flavor: options.flavor.to_owned(),
        total: matched,
        matched,
        returned,
        items,
        warnings,
    })
}

pub fn apply_theme(paths: &AppPaths, id: &str) -> Result<ToolResult> {
    if !theme_items(paths).iter().any(|theme| theme.id == id) {
        bail!("theme '{id}' is not available from this Omarchy checkout");
    }
    let result = capture_tool("omarchy", &["theme", "set", id], None);
    if !result.available {
        bail!("omarchy is unavailable in a trusted executable location");
    }
    if !result.ok {
        bail!("could not apply theme '{id}': {}", result.output);
    }
    Ok(result)
}

fn refresh_apps(paths: &AppPaths) -> Result<AppCatalog> {
    if !command_exists("curl") {
        bail!("curl is required to refresh the app catalogue");
    }
    secure_dir(&paths.marketplace_dir)?;
    let temporary = paths
        .marketplace_dir
        .join(format!("apps.tmp.{}.{}", std::process::id(), now_unix()));
    drop(
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&temporary)
            .context("create temporary app catalogue")?,
    );
    let result = run_trusted_check(
        &CheckSpec {
            name: "discovery-app-refresh".to_owned(),
            argv: vec![
                "curl".to_owned(),
                "--fail".to_owned(),
                "--silent".to_owned(),
                "--show-error".to_owned(),
                "--location".to_owned(),
                "--proto".to_owned(),
                "=https".to_owned(),
                "--proto-redir".to_owned(),
                "=https".to_owned(),
                "--connect-timeout".to_owned(),
                "10".to_owned(),
                "--max-time".to_owned(),
                "60".to_owned(),
                "--max-filesize".to_owned(),
                MAX_APP_CATALOG_BYTES.to_string(),
                "--output".to_owned(),
                temporary.to_string_lossy().into_owned(),
                APP_CATALOG_URL.to_owned(),
            ],
            timeout_seconds: 90,
        },
        &paths.marketplace_dir,
        &paths.marketplace_dir,
    );
    match result {
        Ok(check) if check.ok => {}
        Ok(check) => {
            let _ = fs::remove_file(&temporary);
            bail!("download app catalogue: {}", check.stderr.trim());
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error).context("download app catalogue");
        }
    }
    let catalog = read_app_catalog_path(&temporary)?;
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    fs::rename(&temporary, &paths.app_catalog_file).context("publish app catalogue cache")?;
    Ok(catalog)
}

fn read_app_catalog(paths: &AppPaths) -> Result<AppCatalog> {
    if !paths.app_catalog_file.is_file() {
        bail!("app catalogue is not cached; refresh Discovery first");
    }
    read_app_catalog_path(&paths.app_catalog_file)
}

fn read_app_catalog_path(path: &Path) -> Result<AppCatalog> {
    let metadata = fs::symlink_metadata(path).context("inspect app catalogue")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("app catalogue cache must be a regular file");
    }
    if metadata.len() > MAX_APP_CATALOG_BYTES {
        bail!("app catalogue exceeds size limit");
    }
    let catalog: AppCatalog = serde_json::from_slice(&fs::read(path)?)
        .context("app catalogue is not valid JSON")?;
    if catalog.schema_version != APP_CATALOG_SCHEMA {
        bail!("unsupported app catalogue schema {}", catalog.schema_version);
    }
    if catalog.apps.len() > MAX_ITEMS {
        bail!("app catalogue exceeds item limit");
    }
    Ok(catalog)
}

fn app_item(app: AppEntry) -> DiscoveryItem {
    let package = app.install.package.filter(|name| valid_package(name));
    let command_matches = package.as_ref().is_some_and(|name| {
        app.install
            .command
            .as_deref()
            .is_some_and(|command| command == format!("omarchy pkg add {name}"))
    });
    let installed = package
        .as_deref()
        .is_some_and(|name| capture_tool("pacman", &["-Q", name], None).ok);
    DiscoveryItem {
        id: app.id,
        name: app.name,
        description: format!("{} {}", app.summary, app.description),
        flavor: "app".to_owned(),
        category: app.category,
        version: app.maturity,
        author: app.integration,
        tags: app.architectures,
        status: app.install.status,
        source: app.source_url,
        installed,
        installable: matches!(app.install.channel.as_str(), "arch" | "aur") && command_matches,
        managed: installed,
        update_available: false,
        verified: app.verified_on.is_some(),
        built_in: false,
        package,
        reviewed_revision: None,
        reviewed_at: app.verified_on,
    }
}

fn valid_package(package: &str) -> bool {
    !package.is_empty()
        && package
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"@._+-".contains(&byte))
}

fn theme_items(paths: &AppPaths) -> Vec<DiscoveryItem> {
    let root = env::var_os("OMARCHY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/share/omarchy"));
    let themes = root.join("themes");
    let current = fs::read_to_string(
        paths
            .home_dir
            .join(".local/state/omarchy/current/theme.name"),
    )
    .unwrap_or_default();
    let current = current.trim();
    let mut items = fs::read_dir(themes)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .map(|id| DiscoveryItem {
            name: title_case(&id),
            description: "A first-party Omarchy theme that coordinates the shell, terminal, editor, browser, and lock screen.".to_owned(),
            flavor: "theme".to_owned(),
            category: "Appearance".to_owned(),
            version: "Included".to_owned(),
            author: "Omarchy".to_owned(),
            tags: vec!["first-party".to_owned()],
            status: if id == current { "Current".to_owned() } else { "Available".to_owned() },
            source: root.join("themes").join(&id).to_string_lossy().into_owned(),
            installed: true,
            installable: false,
            managed: true,
            update_available: false,
            verified: true,
            built_in: true,
            package: None,
            reviewed_revision: None,
            reviewed_at: None,
            id,
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| left.name.cmp(&right.name));
    items
}

fn title_case(id: &str) -> String {
    id.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_names_are_bounded_before_the_ui_builds_an_install_command() {
        assert!(valid_package("signal-desktop"));
        assert!(valid_package("visual-studio-code-bin"));
        assert!(!valid_package("signal-desktop; reboot"));
        assert!(!valid_package("$(touch /tmp/nope)"));
        assert!(!valid_package(""));
    }

    #[test]
    fn theme_ids_become_human_readable_names() {
        assert_eq!(title_case("tokyo-night"), "Tokyo Night");
        assert_eq!(title_case("catppuccin"), "Catppuccin");
    }
}
