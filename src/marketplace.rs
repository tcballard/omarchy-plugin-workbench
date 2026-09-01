use crate::manifest;
use crate::model::{CheckResult, CheckSpec};
use crate::paths::{AppPaths, secure_dir};
use crate::process::{command_exists, run_check};
use crate::registry::{RegistryLock, now_unix};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

const CATALOG_URL: &str = "https://omarchyplugins.com/catalog.json";
const CATALOG_SCHEMA: u32 = 2;
const MAX_CATALOG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_CATALOG_PLUGINS: usize = 5_000;
const MAX_SEARCH_LIMIT: usize = 100;
const NETWORK_TIMEOUT_SECONDS: u64 = 90;
const INSTALL_TIMEOUT_SECONDS: u64 = 300;
const RECEIPT_SCHEMA: u32 = 1;
const MAX_RECEIPT_BYTES: u64 = 64 * 1024;

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
    pub managed: bool,
    pub installable: bool,
    pub managed_revision: Option<String>,
    pub update_available: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MarketplaceReceipt {
    pub schema_version: u32,
    pub plugin_id: String,
    pub repo: String,
    pub installed_revision: String,
    pub installed_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedPlugin {
    pub id: String,
    pub repo: String,
    pub installed_revision: String,
    pub catalogue_revision: Option<String>,
    pub state: String,
    pub update_available: bool,
    pub installed_at_unix: u64,
    pub updated_at_unix: u64,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedReport {
    pub ok: bool,
    pub managed: usize,
    pub updates_available: usize,
    pub plugins: Vec<ManagedPlugin>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleReport {
    pub ok: bool,
    pub action: String,
    pub plugin_id: String,
    pub revision: Option<String>,
    pub retained_backup: Option<String>,
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
        let installed_at_unix = now_unix();
        let receipt = MarketplaceReceipt {
            schema_version: RECEIPT_SCHEMA,
            plugin_id: id.to_owned(),
            repo: reviewed_repo.to_owned(),
            installed_revision: reviewed_revision.to_owned(),
            installed_at_unix,
            updated_at_unix: installed_at_unix,
        };
        if let Err(error) = save_receipt(paths, &receipt) {
            let _ = fs::remove_dir_all(&target);
            return Err(error).context("record managed marketplace installation");
        }
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

pub fn managed(paths: &AppPaths) -> Result<ManagedReport> {
    let catalog = read_catalog(paths).ok();
    let mut plugins = load_receipts(paths)?
        .into_iter()
        .map(|receipt| inspect_managed(paths, catalog.as_ref(), receipt))
        .collect::<Vec<_>>();
    plugins.sort_by(|left, right| left.id.cmp(&right.id));
    let updates_available = plugins
        .iter()
        .filter(|plugin| plugin.update_available)
        .count();
    Ok(ManagedReport {
        ok: plugins.iter().all(|plugin| plugin.error.is_none()),
        managed: plugins.len(),
        updates_available,
        plugins,
    })
}

pub fn is_managed(paths: &AppPaths, id: &str) -> bool {
    receipt_path(paths, id).is_file()
}

pub fn submission_collision(paths: &AppPaths, id: &str, repo: &str) -> Result<Option<String>> {
    let catalog = read_catalog(paths)?;
    Ok(catalog.plugins.iter().find_map(|plugin| {
        if plugin.id == id {
            Some(format!(
                "plugin id '{id}' is already listed from {}",
                plugin.repo
            ))
        } else if plugin.repo.trim_end_matches(".git") == repo.trim_end_matches(".git") {
            Some(format!(
                "repository is already listed as plugin '{}'",
                plugin.id
            ))
        } else {
            None
        }
    }))
}

pub fn update_managed(
    paths: &AppPaths,
    id: &str,
    reviewed_revision: &str,
    confirmed: bool,
) -> Result<LifecycleReport> {
    require_lifecycle_confirmation(confirmed, "update")?;
    validate_plugin_id(id)?;
    validate_revision(reviewed_revision)?;
    for command in ["git", "omarchy", "omarchy-shell"] {
        if !command_exists(command) {
            bail!("{command} is required to update marketplace plugins");
        }
    }
    let catalog = read_catalog(paths)?;
    let listing = catalog
        .plugins
        .iter()
        .find(|plugin| plugin.id == id)
        .with_context(|| format!("plugin '{id}' is not in the cached marketplace catalogue"))?;
    if listing.listing_validated_commit != reviewed_revision {
        bail!("marketplace entry changed since review; refresh and review again");
    }
    let _lock = RegistryLock::acquire(paths)?;
    let mut receipt = load_receipt(paths, id)?
        .with_context(|| format!("plugin '{id}' is not managed by Workbench"))?;
    if receipt.repo != listing.repo {
        bail!("managed repository differs from the current catalogue listing");
    }
    let directory = verified_managed_target(paths, &receipt)?;
    let current = git_stdout(paths, &directory, &["rev-parse", "HEAD"])?;
    if current != receipt.installed_revision {
        bail!("installed revision drifted from the Workbench receipt");
    }
    if !git_stdout(
        paths,
        &directory,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?
    .is_empty()
    {
        bail!("managed plugin has local changes; repair or preserve them manually");
    }
    if current == reviewed_revision {
        return Ok(LifecycleReport {
            ok: true,
            action: "marketplace-update".to_owned(),
            plugin_id: id.to_owned(),
            revision: Some(current),
            retained_backup: None,
            message: format!("{id} is already at the reviewed marketplace revision"),
            warnings: Vec::new(),
        });
    }
    let fetch = run_git(
        paths,
        &directory,
        &["fetch", "--quiet", "origin", reviewed_revision],
    )?;
    if !fetch.ok {
        bail!(
            "could not fetch reviewed revision: {}",
            check_output(&fetch)
        );
    }
    let ancestry = run_git(
        paths,
        &directory,
        &["merge-base", "--is-ancestor", &current, reviewed_revision],
    )?;
    if !ancestry.ok {
        bail!("reviewed marketplace revision is not a fast-forward from the installed snapshot");
    }
    let merge = run_git(
        paths,
        &directory,
        &["merge", "--ff-only", reviewed_revision],
    )?;
    if !merge.ok {
        bail!(
            "could not apply reviewed revision: {}",
            check_output(&merge)
        );
    }
    if let Err(error) = validate_managed_checkout(paths, id, &directory, reviewed_revision) {
        let _ = run_git(paths, &directory, &["reset", "--hard", &current]);
        bail!("marketplace update failed validation and was rolled back: {error:#}");
    }
    receipt.installed_revision = reviewed_revision.to_owned();
    receipt.updated_at_unix = now_unix();
    if let Err(error) = save_receipt(paths, &receipt) {
        let _ = run_git(paths, &directory, &["reset", "--hard", &current]);
        return Err(error).context("update marketplace ownership receipt; checkout rolled back");
    }
    rescan_shell(paths)?;
    Ok(LifecycleReport {
        ok: true,
        action: "marketplace-update".to_owned(),
        plugin_id: id.to_owned(),
        revision: Some(reviewed_revision.to_owned()),
        retained_backup: None,
        message: format!("Updated {id} to reviewed marketplace revision {reviewed_revision}"),
        warnings: Vec::new(),
    })
}

pub fn repair(paths: &AppPaths, id: &str, confirmed: bool) -> Result<LifecycleReport> {
    require_lifecycle_confirmation(confirmed, "repair")?;
    validate_plugin_id(id)?;
    for command in ["git", "omarchy", "omarchy-shell"] {
        if !command_exists(command) {
            bail!("{command} is required to repair marketplace plugins");
        }
    }
    let catalog = read_catalog(paths)?;
    let listing = catalog
        .plugins
        .iter()
        .find(|plugin| plugin.id == id)
        .with_context(|| format!("plugin '{id}' is not in the cached marketplace catalogue"))?;
    if !is_installable(listing) {
        bail!("plugin '{id}' is no longer an installable marketplace root plugin");
    }
    let _lock = RegistryLock::acquire(paths)?;
    let receipt = load_receipt(paths, id)?
        .with_context(|| format!("plugin '{id}' is not managed by Workbench"))?;
    if receipt.repo != listing.repo {
        bail!("managed repository differs from the current catalogue listing");
    }
    ensure_plugins_directory(&paths.plugins_dir)?;
    let target = paths.plugins_dir.join(id);
    let backup = if target.exists() || target.is_symlink() {
        let metadata = fs::symlink_metadata(&target)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("managed target is not a normal directory; refusing repair");
        }
        let backup = paths
            .marketplace_trash_dir
            .join(format!("{id}.repair.{}", now_unix()));
        fs::rename(&target, &backup).context("retain pre-repair marketplace checkout")?;
        Some(backup)
    } else {
        None
    };
    let revision = listing.listing_validated_commit.clone();
    if let Err(error) = clone_reviewed(paths, id, &listing.repo, &revision, &target) {
        if let Some(backup) = &backup {
            let _ = fs::rename(backup, &target);
        }
        return Err(error).context("repair marketplace plugin; previous checkout restored");
    }
    let repaired = MarketplaceReceipt {
        schema_version: RECEIPT_SCHEMA,
        plugin_id: id.to_owned(),
        repo: listing.repo.clone(),
        installed_revision: revision.clone(),
        installed_at_unix: receipt.installed_at_unix,
        updated_at_unix: now_unix(),
    };
    save_receipt(paths, &repaired)?;
    rescan_shell(paths)?;
    Ok(LifecycleReport {
        ok: true,
        action: "marketplace-repair".to_owned(),
        plugin_id: id.to_owned(),
        revision: Some(revision),
        retained_backup: backup.map(|path| path.display().to_string()),
        message: format!("Repaired {id} from the latest reviewed marketplace snapshot"),
        warnings: Vec::new(),
    })
}

pub fn uninstall(paths: &AppPaths, id: &str, confirmed: bool) -> Result<LifecycleReport> {
    require_lifecycle_confirmation(confirmed, "uninstall")?;
    validate_plugin_id(id)?;
    let _lock = RegistryLock::acquire(paths)?;
    let receipt = load_receipt(paths, id)?
        .with_context(|| format!("plugin '{id}' is not managed by Workbench"))?;
    let target = paths.plugins_dir.join(id);
    let backup = if target.exists() || target.is_symlink() {
        let metadata = fs::symlink_metadata(&target)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("managed target is not a normal directory; refusing uninstall");
        }
        let backup = paths
            .marketplace_trash_dir
            .join(format!("{id}.uninstall.{}", now_unix()));
        fs::rename(&target, &backup).context("retain uninstalled marketplace checkout")?;
        Some(backup)
    } else {
        None
    };
    let mut warnings = Vec::new();
    if command_exists("omarchy") {
        let disable = run_tool(
            paths,
            "disable-marketplace-plugin",
            vec![
                "omarchy".to_owned(),
                "plugin".to_owned(),
                "disable".to_owned(),
                id.to_owned(),
            ],
            60,
        );
        if let Ok(result) = disable
            && !result.ok
        {
            warnings.push(format!(
                "plugin was removed but disable failed: {}",
                check_output(&result)
            ));
        }
    }
    remove_receipt(paths, id)?;
    if command_exists("omarchy-shell")
        && let Err(error) = rescan_shell(paths)
    {
        warnings.push(format!(
            "plugin was removed but shell rescan failed: {error:#}"
        ));
    }
    Ok(LifecycleReport {
        ok: true,
        action: "marketplace-uninstall".to_owned(),
        plugin_id: id.to_owned(),
        revision: Some(receipt.installed_revision),
        retained_backup: backup.map(|path| path.display().to_string()),
        message: format!("Uninstalled {id}; the previous checkout was retained for recovery"),
        warnings,
    })
}

fn inspect_managed(
    paths: &AppPaths,
    catalog: Option<&Catalog>,
    receipt: MarketplaceReceipt,
) -> ManagedPlugin {
    let catalogue_revision = catalog.and_then(|catalog| {
        catalog
            .plugins
            .iter()
            .find(|plugin| plugin.id == receipt.plugin_id && plugin.repo == receipt.repo)
            .and_then(|plugin| nonempty(&plugin.listing_validated_commit))
    });
    let target = paths.plugins_dir.join(&receipt.plugin_id);
    let inspected = (|| -> Result<(String, bool)> {
        let metadata = fs::symlink_metadata(&target).context("managed target is missing")?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("managed target is not a normal directory");
        }
        let manifest = manifest::validate_plugin(&target)?;
        if manifest.id != receipt.plugin_id {
            bail!("installed manifest id differs from its receipt");
        }
        if !command_exists("git") {
            bail!("git is unavailable");
        }
        let revision = git_stdout(paths, &target, &["rev-parse", "HEAD"])?;
        let dirty = !git_stdout(
            paths,
            &target,
            &["status", "--porcelain", "--untracked-files=normal"],
        )?
        .is_empty();
        Ok((revision, dirty))
    })();
    let (state, update_available, error) = match inspected {
        Ok((revision, _)) if revision != receipt.installed_revision => (
            "drifted".to_owned(),
            false,
            Some("installed revision differs from its ownership receipt".to_owned()),
        ),
        Ok((_, true)) => (
            "local-changes".to_owned(),
            false,
            Some("installed checkout has local changes".to_owned()),
        ),
        Ok((_, false)) if catalogue_revision.as_deref() == Some(&receipt.installed_revision) => {
            ("current".to_owned(), false, None)
        }
        Ok((_, false)) if catalogue_revision.is_some() => {
            ("update-available".to_owned(), true, None)
        }
        Ok((_, false)) => ("catalogue-missing".to_owned(), false, None),
        Err(error) => ("drifted".to_owned(), false, Some(format!("{error:#}"))),
    };
    ManagedPlugin {
        id: receipt.plugin_id,
        repo: receipt.repo,
        installed_revision: receipt.installed_revision,
        catalogue_revision,
        state,
        update_available,
        installed_at_unix: receipt.installed_at_unix,
        updated_at_unix: receipt.updated_at_unix,
        error,
    }
}

fn clone_reviewed(
    paths: &AppPaths,
    id: &str,
    repo: &str,
    revision: &str,
    target: &Path,
) -> Result<()> {
    if target.exists() || target.is_symlink() {
        bail!("plugin target already exists");
    }
    let stage = paths.plugins_dir.join(format!(
        ".workbench-marketplace.{}.{}",
        std::process::id(),
        now_unix()
    ));
    if stage.exists() || stage.is_symlink() {
        bail!("marketplace staging path already exists");
    }
    let result = (|| -> Result<()> {
        let clone = run_git(
            paths,
            &paths.plugins_dir,
            &[
                "clone",
                "--no-checkout",
                "--",
                repo,
                &stage.to_string_lossy(),
            ],
        )?;
        if !clone.ok {
            bail!(
                "could not clone reviewed repository: {}",
                check_output(&clone)
            );
        }
        let checkout = run_git(paths, &stage, &["checkout", "--detach", revision])?;
        if !checkout.ok {
            bail!(
                "reviewed revision is unavailable: {}",
                check_output(&checkout)
            );
        }
        validate_managed_checkout(paths, id, &stage, revision)?;
        if target.exists() || target.is_symlink() {
            bail!("plugin target appeared during installation");
        }
        fs::rename(&stage, target).context("publish repaired marketplace checkout")?;
        Ok(())
    })();
    if result.is_err() {
        remove_owned_stage(&paths.plugins_dir, &stage);
    }
    result
}

fn validate_managed_checkout(
    paths: &AppPaths,
    id: &str,
    directory: &Path,
    revision: &str,
) -> Result<()> {
    if git_stdout(paths, directory, &["rev-parse", "HEAD"])? != revision {
        bail!("Git did not check out the reviewed revision");
    }
    let validated = manifest::validate_plugin(directory)?;
    if validated.id != id {
        bail!(
            "reviewed manifest id '{}' does not match '{id}'",
            validated.id
        );
    }
    validate_with_omarchy(paths, directory)
}

fn require_lifecycle_confirmation(confirmed: bool, action: &str) -> Result<()> {
    if !confirmed {
        bail!("refusing to {action} without explicit confirmation; pass --yes after review");
    }
    Ok(())
}

fn receipt_path(paths: &AppPaths, id: &str) -> PathBuf {
    paths.marketplace_receipts_dir.join(format!("{id}.json"))
}

fn load_receipts(paths: &AppPaths) -> Result<Vec<MarketplaceReceipt>> {
    let mut receipts = Vec::new();
    let mut entries = 0usize;
    for entry in fs::read_dir(&paths.marketplace_receipts_dir)
        .context("read marketplace ownership receipts")?
    {
        let entry = entry?;
        entries += 1;
        if entries > 128 {
            bail!("marketplace ownership receipt count exceeds 128");
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("marketplace ownership receipt is not a normal file");
        }
        if entry.path().extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        receipts.push(read_receipt_path(&entry.path())?);
    }
    Ok(receipts)
}

fn load_receipt(paths: &AppPaths, id: &str) -> Result<Option<MarketplaceReceipt>> {
    let path = receipt_path(paths, id);
    if !path.exists() && !path.is_symlink() {
        return Ok(None);
    }
    read_receipt_path(&path).map(Some)
}

fn read_receipt_path(path: &Path) -> Result<MarketplaceReceipt> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_RECEIPT_BYTES
    {
        bail!("marketplace ownership receipt is not a bounded normal file");
    }
    let receipt: MarketplaceReceipt =
        serde_json::from_slice(&fs::read(path)?).context("parse marketplace ownership receipt")?;
    if receipt.schema_version != RECEIPT_SCHEMA {
        bail!("unsupported marketplace ownership receipt schema");
    }
    validate_plugin_id(&receipt.plugin_id)?;
    validate_github_repo(&receipt.repo)?;
    validate_revision(&receipt.installed_revision)?;
    Ok(receipt)
}

fn save_receipt(paths: &AppPaths, receipt: &MarketplaceReceipt) -> Result<()> {
    secure_dir(&paths.marketplace_receipts_dir)?;
    let path = receipt_path(paths, &receipt.plugin_id);
    if path.is_symlink() {
        bail!("marketplace ownership receipt path is a symlink");
    }
    let temporary = paths.marketplace_receipts_dir.join(format!(
        ".{}.tmp.{}.{}",
        receipt.plugin_id,
        std::process::id(),
        now_unix()
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        serde_json::to_writer_pretty(&mut file, receipt)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn remove_receipt(paths: &AppPaths, id: &str) -> Result<()> {
    let path = receipt_path(paths, id);
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("marketplace ownership receipt is not a normal file");
    }
    fs::remove_file(path).context("remove marketplace ownership receipt")
}

fn verified_managed_target(paths: &AppPaths, receipt: &MarketplaceReceipt) -> Result<PathBuf> {
    let target = paths.plugins_dir.join(&receipt.plugin_id);
    let metadata = fs::symlink_metadata(&target)
        .with_context(|| format!("managed plugin '{}' is missing", receipt.plugin_id))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("managed marketplace target is not a normal directory");
    }
    let manifest = manifest::validate_plugin(&target)?;
    if manifest.id != receipt.plugin_id {
        bail!("managed marketplace target manifest id changed");
    }
    Ok(target)
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
    let receipt = load_receipt(paths, &plugin.id).ok().flatten();
    let managed_revision = receipt
        .as_ref()
        .map(|receipt| receipt.installed_revision.clone());
    let managed = receipt.is_some();
    let update_available = managed_revision
        .as_deref()
        .is_some_and(|revision| revision != plugin.listing_validated_commit);
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
        managed,
        installable: is_installable(plugin) && !installed,
        managed_revision,
        update_available,
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
    let owned = stage.parent() == Some(plugins_dir)
        && stage
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(".workbench-marketplace."));
    if !owned {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(stage) else {
        return;
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        let _ = fs::remove_file(stage);
    } else if metadata.is_dir() {
        let _ = fs::remove_dir_all(stage);
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
