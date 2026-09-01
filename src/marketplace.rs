use crate::manifest;
use crate::model::{CheckResult, CheckSpec};
use crate::paths::{AppPaths, secure_dir};
use crate::process::{command_exists, run_check};
use crate::registry::{RegistryLock, now_unix};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const CATALOG_URL: &str = "https://omarchyplugins.com/catalog.json";
const CATALOG_SCHEMA: u32 = 2;
const MAX_CATALOG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CATALOG_PLUGINS: usize = 5_000;
const MAX_SEARCH_LIMIT: usize = 100;
const NETWORK_TIMEOUT_SECONDS: u64 = 90;
const INSTALL_TIMEOUT_SECONDS: u64 = 300;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    generated_at: String,
    state_schema_version: u32,
    mode: String,
    plugins: Vec<CatalogPlugin>,
    #[serde(default)]
    warnings: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogPlugin {
    id: String,
    name: String,
    description: String,
    author: String,
    version: String,
    category: String,
    #[serde(default)]
    tags: Vec<String>,
    kind: String,
    status: String,
    repo: String,
    source_type: String,
    #[serde(default)]
    built_in: bool,
    #[serde(default)]
    install_available: bool,
    #[serde(default)]
    repository_layout: String,
    #[serde(default)]
    verification_status: String,
    #[serde(default)]
    verification_snapshot_status: String,
    #[serde(default)]
    verification_coverage: String,
    #[serde(default)]
    listing_validated_commit: String,
    #[serde(default)]
    listing_validated_at: String,
    #[serde(default)]
    repository_updated_at: String,
    #[serde(default)]
    stars: u64,
}

#[derive(Debug)]
pub struct SearchFilters<'a> {
    pub query: Option<&'a str>,
    pub category: Option<&'a str>,
    pub tag: Option<&'a str>,
    pub kind: Option<&'a str>,
    pub built_in_only: bool,
    pub verified_only: bool,
    pub installable_only: bool,
    pub installed_only: bool,
    pub limit: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshReport {
    pub ok: bool,
    pub source: &'static str,
    pub generated_at: String,
    pub plugins: usize,
    pub warnings: usize,
    pub cached_at_unix: u64,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchReport {
    pub ok: bool,
    pub source: &'static str,
    pub generated_at: String,
    pub total: usize,
    pub matched: usize,
    pub returned: usize,
    pub plugins: Vec<MarketplacePlugin>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketplacePlugin {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub category: String,
    pub tags: Vec<String>,
    pub kind: String,
    pub status: String,
    pub repo: String,
    pub source_type: String,
    pub built_in: bool,
    pub installed: bool,
    pub installable: bool,
    pub verification_status: String,
    pub verification_snapshot_status: String,
    pub verification_coverage: String,
    pub reviewed_revision: Option<String>,
    pub reviewed_at: Option<String>,
    pub repository_updated_at: Option<String>,
    pub stars: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReport {
    pub ok: bool,
    pub action: String,
    pub plugin_id: String,
    pub repo: String,
    pub revision: String,
    pub installed: bool,
    pub enabled: bool,
    pub message: String,
    pub warnings: Vec<String>,
}

pub fn refresh(paths: &AppPaths) -> Result<RefreshReport> {
    if !command_exists("curl") {
        bail!("curl is required to refresh the marketplace catalogue");
    }
    let _lock = RegistryLock::acquire(paths)?;
    secure_dir(&paths.marketplace_dir)?;
    let temporary =
        paths
            .marketplace_dir
            .join(format!("catalog.tmp.{}.{}", std::process::id(), now_unix()));
    let temporary_file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)
        .context("create temporary marketplace catalogue")?;
    drop(temporary_file);

    let output_path = temporary.to_string_lossy().into_owned();
    let download = run_check(
        &CheckSpec {
            name: "marketplace-refresh".to_owned(),
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
                MAX_CATALOG_BYTES.to_string(),
                "--output".to_owned(),
                output_path,
                CATALOG_URL.to_owned(),
            ],
            timeout_seconds: NETWORK_TIMEOUT_SECONDS,
        },
        &paths.marketplace_dir,
        &paths.state_dir.join("command-output"),
    );
    match download {
        Ok(result) if result.ok => {}
        Ok(result) => {
            let _ = fs::remove_file(&temporary);
            bail!("marketplace download failed: {}", check_output(&result));
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error).context("download marketplace catalogue");
        }
    }
    let catalog = match read_catalog_path(&temporary) {
        Ok(catalog) => catalog,
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    };
    fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600))?;
    fs::rename(&temporary, &paths.marketplace_catalog_file)
        .context("publish marketplace catalogue cache")?;
    let cached_at_unix = now_unix();
    Ok(RefreshReport {
        ok: true,
        source: CATALOG_URL,
        generated_at: catalog.generated_at.clone(),
        plugins: catalog.plugins.len(),
        warnings: catalog.warnings.len(),
        cached_at_unix,
        message: format!(
            "Cached {} official marketplace listings generated {}",
            catalog.plugins.len(),
            catalog.generated_at
        ),
    })
}

pub fn search(paths: &AppPaths, filters: &SearchFilters<'_>) -> Result<SearchReport> {
    if filters.limit == 0 || filters.limit > MAX_SEARCH_LIMIT {
        bail!("marketplace search limit must be between 1 and {MAX_SEARCH_LIMIT}");
    }
    let catalog = read_catalog(paths)?;
    let query_tokens = filters
        .query
        .unwrap_or_default()
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    let total = catalog.plugins.len();
    let mut matches = catalog
        .plugins
        .iter()
        .filter(|plugin| matches_filters(plugin, filters, &query_tokens))
        .map(|plugin| to_result(paths, plugin))
        .filter(|plugin| !filters.installed_only || plugin.installed)
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        right
            .installed
            .cmp(&left.installed)
            .then_with(|| {
                left.name
                    .to_ascii_lowercase()
                    .cmp(&right.name.to_ascii_lowercase())
            })
            .then_with(|| left.id.cmp(&right.id))
    });
    let matched = matches.len();
    matches.truncate(filters.limit);
    let returned = matches.len();
    Ok(SearchReport {
        ok: true,
        source: CATALOG_URL,
        generated_at: catalog.generated_at,
        total,
        matched,
        returned,
        plugins: matches,
    })
}

pub fn install(
    paths: &AppPaths,
    id: &str,
    reviewed_repo: &str,
    reviewed_revision: &str,
    enable: bool,
    confirmed: bool,
) -> Result<InstallReport> {
    if !confirmed {
        bail!("refusing to install without explicit confirmation; pass --yes after review");
    }
    validate_plugin_id(id)?;
    validate_revision(reviewed_revision)?;
    validate_github_repo(reviewed_repo)?;
    for command in ["git", "omarchy", "omarchy-shell"] {
        if !command_exists(command) {
            bail!("{command} is required to install marketplace plugins");
        }
    }
    let catalog = read_catalog(paths)?;
    let plugin = catalog
        .plugins
        .iter()
        .find(|plugin| plugin.id == id)
        .with_context(|| format!("plugin '{id}' is not in the cached marketplace catalogue"))?;
    if plugin.built_in || plugin.source_type == "builtin" {
        bail!("built-in plugins are managed by Omarchy and cannot be installed from Workbench");
    }
    if !plugin.install_available || plugin.repository_layout != "root-plugin" {
        bail!("plugin '{id}' is listed but is not an installable root plugin");
    }
    if plugin.repo != reviewed_repo || plugin.listing_validated_commit != reviewed_revision {
        bail!("marketplace entry changed since review; refresh and search again");
    }

    let _lock = RegistryLock::acquire(paths)?;
    ensure_plugins_directory(&paths.plugins_dir)?;
    let target = paths.plugins_dir.join(id);
    if target.exists() || target.is_symlink() {
        bail!("plugin '{id}' is already installed");
    }
    let stage = paths.plugins_dir.join(format!(
        ".workbench-marketplace.{}.{}",
        std::process::id(),
        now_unix()
    ));
    if stage.exists() || stage.is_symlink() {
        bail!(
            "marketplace staging path already exists: {}",
            stage.display()
        );
    }

    let install_result = (|| -> Result<()> {
        let clone = run_git(
            paths,
            &paths.plugins_dir,
            &[
                "clone",
                "--no-checkout",
                "--",
                reviewed_repo,
                &stage.to_string_lossy(),
            ],
        )?;
        if !clone.ok {
            bail!(
                "could not clone reviewed repository: {}",
                check_output(&clone)
            );
        }
        let checkout = run_git(paths, &stage, &["checkout", "--detach", reviewed_revision])?;
        if !checkout.ok {
            bail!(
                "reviewed revision is unavailable: {}",
                check_output(&checkout)
            );
        }
        let actual_revision = git_stdout(paths, &stage, &["rev-parse", "HEAD"])?;
        if actual_revision != reviewed_revision {
            bail!("Git did not check out the reviewed revision");
        }
        let validated = manifest::validate_plugin(&stage)?;
        if validated.id != id {
            bail!(
                "catalogue id '{id}' does not match reviewed manifest id '{}'",
                validated.id
            );
        }
        validate_with_omarchy(paths, &stage)?;
        if target.exists() || target.is_symlink() {
            bail!("plugin target appeared during installation; refusing to replace it");
        }
        fs::rename(&stage, &target).context("publish reviewed plugin checkout")?;
        Ok(())
    })();
    if let Err(error) = install_result {
        remove_owned_stage(&paths.plugins_dir, &stage);
        return Err(error);
    }

    let mut warnings = Vec::new();
    if let Err(error) = rescan_shell(paths) {
        warnings.push(format!(
            "installed and validated, but shell rescan failed: {error:#}"
        ));
    }
    let enabled = if enable && warnings.is_empty() {
        match enable_plugin(paths, id) {
            Ok(()) => true,
            Err(error) => {
                warnings.push(format!("installed but could not enable: {error:#}"));
                false
            }
        }
    } else {
        false
    };
    let message = if enabled {
        format!("Installed and enabled {id} at reviewed revision {reviewed_revision}")
    } else {
        format!("Installed {id} at reviewed revision {reviewed_revision}")
    };
    Ok(InstallReport {
        ok: true,
        action: "marketplace-install".to_owned(),
        plugin_id: id.to_owned(),
        repo: reviewed_repo.to_owned(),
        revision: reviewed_revision.to_owned(),
        installed: true,
        enabled,
        message,
        warnings,
    })
}

fn read_catalog(paths: &AppPaths) -> Result<Catalog> {
    if !paths.marketplace_catalog_file.exists() {
        bail!("marketplace catalogue is not cached; run marketplace-refresh first");
    }
    read_catalog_path(&paths.marketplace_catalog_file)
}

fn read_catalog_path(path: &Path) -> Result<Catalog> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect marketplace catalogue {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("marketplace catalogue is not a normal file");
    }
    if metadata.len() == 0 || metadata.len() > MAX_CATALOG_BYTES {
        bail!("marketplace catalogue exceeds the allowed size boundary");
    }
    let bytes = fs::read(path).context("read marketplace catalogue")?;
    let catalog: Catalog = serde_json::from_slice(&bytes).context("parse marketplace catalogue")?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

fn validate_catalog(catalog: &Catalog) -> Result<()> {
    if catalog.state_schema_version != CATALOG_SCHEMA {
        bail!(
            "unsupported marketplace state schema {}; expected {CATALOG_SCHEMA}",
            catalog.state_schema_version
        );
    }
    if catalog.mode != "production" {
        bail!("refusing non-production marketplace catalogue");
    }
    if catalog.generated_at.is_empty() || catalog.generated_at.len() > 80 {
        bail!("marketplace generatedAt is missing or invalid");
    }
    if catalog.plugins.is_empty() || catalog.plugins.len() > MAX_CATALOG_PLUGINS {
        bail!("marketplace plugin count is outside the supported boundary");
    }
    let mut ids = BTreeSet::new();
    for plugin in &catalog.plugins {
        validate_plugin_id(&plugin.id)?;
        validate_field(&plugin.name, "name", 256)?;
        validate_field(&plugin.description, "description", 4_096)?;
        validate_field(&plugin.author, "author", 256)?;
        validate_field(&plugin.version, "version", 128)?;
        validate_field(&plugin.category, "category", 128)?;
        validate_field(&plugin.kind, "kind", 128)?;
        validate_field(&plugin.status, "status", 128)?;
        validate_field(&plugin.source_type, "sourceType", 128)?;
        validate_field(&plugin.repo, "repo", 2_048)?;
        if plugin.tags.len() > 16 || plugin.tags.iter().any(|tag| tag.len() > 64) {
            bail!(
                "marketplace tags exceed the supported boundary for '{}'",
                plugin.id
            );
        }
        if !ids.insert(&plugin.id) {
            bail!("duplicate marketplace plugin id '{}'", plugin.id);
        }
        if plugin.install_available {
            validate_github_repo(&plugin.repo)?;
            validate_revision(&plugin.listing_validated_commit)?;
            if plugin.repository_layout != "root-plugin" {
                bail!("installable plugin '{}' is not a root plugin", plugin.id);
            }
        }
    }
    Ok(())
}

fn validate_field(value: &str, name: &str, maximum: usize) -> Result<()> {
    if value.is_empty() || value.len() > maximum || value.contains('\0') {
        bail!("marketplace {name} is missing or exceeds its boundary");
    }
    Ok(())
}

fn matches_filters(
    plugin: &CatalogPlugin,
    filters: &SearchFilters<'_>,
    query_tokens: &[String],
) -> bool {
    let searchable = format!(
        "{} {} {} {} {} {} {}",
        plugin.id,
        plugin.name,
        plugin.description,
        plugin.author,
        plugin.category,
        plugin.kind,
        plugin.tags.join(" ")
    )
    .to_ascii_lowercase();
    query_tokens.iter().all(|token| searchable.contains(token))
        && filters
            .category
            .is_none_or(|value| plugin.category.eq_ignore_ascii_case(value))
        && filters
            .kind
            .is_none_or(|value| plugin.kind.eq_ignore_ascii_case(value))
        && filters.tag.is_none_or(|value| {
            plugin
                .tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case(value))
        })
        && (!filters.built_in_only || plugin.built_in || plugin.source_type == "builtin")
        && (!filters.verified_only || plugin.verification_status == "verified")
        && (!filters.installable_only || is_installable(plugin))
}

fn to_result(paths: &AppPaths, plugin: &CatalogPlugin) -> MarketplacePlugin {
    let built_in = plugin.built_in || plugin.source_type == "builtin";
    let installed = built_in
        || paths.plugins_dir.join(&plugin.id).exists()
        || paths.plugins_dir.join(&plugin.id).is_symlink();
    MarketplacePlugin {
        id: plugin.id.clone(),
        name: plugin.name.clone(),
        description: plugin.description.clone(),
        author: plugin.author.clone(),
        version: plugin.version.clone(),
        category: plugin.category.clone(),
        tags: plugin.tags.clone(),
        kind: plugin.kind.clone(),
        status: plugin.status.clone(),
        repo: plugin.repo.clone(),
        source_type: plugin.source_type.clone(),
        built_in,
        installed,
        installable: is_installable(plugin) && !installed,
        verification_status: plugin.verification_status.clone(),
        verification_snapshot_status: plugin.verification_snapshot_status.clone(),
        verification_coverage: plugin.verification_coverage.clone(),
        reviewed_revision: nonempty(&plugin.listing_validated_commit),
        reviewed_at: nonempty(&plugin.listing_validated_at),
        repository_updated_at: nonempty(&plugin.repository_updated_at),
        stars: plugin.stars,
    }
}

fn is_installable(plugin: &CatalogPlugin) -> bool {
    !plugin.built_in
        && plugin.source_type != "builtin"
        && plugin.install_available
        && plugin.repository_layout == "root-plugin"
        && validate_revision(&plugin.listing_validated_commit).is_ok()
        && validate_github_repo(&plugin.repo).is_ok()
}

fn nonempty(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}

fn validate_plugin_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.contains("..")
        || !id.chars().enumerate().all(|(index, character)| {
            character.is_ascii_alphanumeric() || (index > 0 && matches!(character, '.' | '_' | '-'))
        })
    {
        bail!("invalid marketplace plugin id '{id}'");
    }
    Ok(())
}

fn validate_revision(revision: &str) -> Result<()> {
    if revision.len() != 40 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("reviewed revision must be a full 40-character Git object id");
    }
    Ok(())
}

fn validate_github_repo(repo: &str) -> Result<()> {
    let Some(path) = repo.strip_prefix("https://github.com/") else {
        bail!("installable marketplace repository must use GitHub HTTPS");
    };
    let path = path.strip_suffix(".git").unwrap_or(path);
    let mut parts = path.split('/');
    let owner = parts.next().unwrap_or_default();
    let name = parts.next().unwrap_or_default();
    if owner.is_empty()
        || name.is_empty()
        || parts.next().is_some()
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("invalid GitHub marketplace repository URL");
    }
    Ok(())
}

fn ensure_plugins_directory(path: &Path) -> Result<()> {
    if path.exists() {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("Omarchy plugins path is not a normal directory");
        }
    } else {
        fs::create_dir_all(path).context("create Omarchy plugins directory")?;
    }
    Ok(())
}

fn run_git(paths: &AppPaths, cwd: &Path, args: &[&str]) -> Result<CheckResult> {
    let mut argv = vec![
        "git".to_owned(),
        "-c".to_owned(),
        "credential.interactive=false".to_owned(),
        "-c".to_owned(),
        "core.hooksPath=/dev/null".to_owned(),
        "-c".to_owned(),
        "core.fsmonitor=false".to_owned(),
        "-c".to_owned(),
        "protocol.ext.allow=never".to_owned(),
    ];
    argv.extend(args.iter().map(|argument| (*argument).to_owned()));
    run_check(
        &CheckSpec {
            name: "marketplace-install-git".to_owned(),
            argv,
            timeout_seconds: INSTALL_TIMEOUT_SECONDS,
        },
        cwd,
        &paths.state_dir.join("command-output"),
    )
}

fn git_stdout(paths: &AppPaths, cwd: &Path, args: &[&str]) -> Result<String> {
    let result = run_git(paths, cwd, args)?;
    if !result.ok {
        bail!("Git command failed: {}", check_output(&result));
    }
    Ok(result.stdout.trim().to_owned())
}

fn validate_with_omarchy(paths: &AppPaths, plugin: &Path) -> Result<()> {
    let result = run_tool(
        paths,
        "validate-marketplace-plugin",
        vec![
            "omarchy".to_owned(),
            "plugin".to_owned(),
            "validate".to_owned(),
            plugin.to_string_lossy().into_owned(),
        ],
        INSTALL_TIMEOUT_SECONDS,
    )?;
    if !result.ok {
        bail!("Omarchy validation failed: {}", check_output(&result));
    }
    Ok(())
}

fn rescan_shell(paths: &AppPaths) -> Result<()> {
    let result = run_tool(
        paths,
        "rescan-marketplace-plugin",
        vec![
            "omarchy-shell".to_owned(),
            "shell".to_owned(),
            "rescanPlugins".to_owned(),
        ],
        60,
    )?;
    if !result.ok {
        bail!("{}", check_output(&result));
    }
    Ok(())
}

fn enable_plugin(paths: &AppPaths, id: &str) -> Result<()> {
    let result = run_tool(
        paths,
        "enable-marketplace-plugin",
        vec![
            "omarchy".to_owned(),
            "plugin".to_owned(),
            "enable".to_owned(),
            id.to_owned(),
        ],
        60,
    )?;
    if !result.ok {
        bail!("{}", check_output(&result));
    }
    Ok(())
}

fn run_tool(
    paths: &AppPaths,
    name: &str,
    argv: Vec<String>,
    timeout_seconds: u64,
) -> Result<CheckResult> {
    run_check(
        &CheckSpec {
            name: name.to_owned(),
            argv,
            timeout_seconds,
        },
        &paths.plugins_dir,
        &paths.state_dir.join("command-output"),
    )
}

fn remove_owned_stage(plugins_dir: &Path, stage: &Path) {
    if stage.parent() == Some(plugins_dir)
        && stage
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".workbench-marketplace."))
    {
        if let Ok(metadata) = fs::symlink_metadata(stage) {
            if metadata.file_type().is_symlink() || metadata.is_file() {
                let _ = fs::remove_file(stage);
            } else if metadata.is_dir() {
                let _ = fs::remove_dir_all(stage);
            }
        }
    }
}

fn check_output(result: &CheckResult) -> String {
    let output = [result.stdout.trim(), result.stderr.trim()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if result.timed_out {
        "command timed out".to_owned()
    } else if output.is_empty() {
        format!("command exited with {:?}", result.exit_code)
    } else {
        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_repository_boundary_is_exact() {
        assert!(validate_github_repo("https://github.com/acme/plugin").is_ok());
        assert!(validate_github_repo("https://github.com/acme/plugin.git").is_ok());
        assert!(validate_github_repo("https://evil.example/acme/plugin").is_err());
        assert!(validate_github_repo("https://github.com/acme/plugin/extra").is_err());
        assert!(validate_github_repo("ext::sh -c bad").is_err());
    }

    #[test]
    fn full_revision_is_required() {
        assert!(validate_revision("0123456789abcdef0123456789abcdef01234567").is_ok());
        assert!(validate_revision("main").is_err());
        assert!(validate_revision("0123456").is_err());
    }
}
