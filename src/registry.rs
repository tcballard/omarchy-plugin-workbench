use crate::model::{CONFIG_SCHEMA, PROJECT_SCHEMA, Project, ProjectDefinition, RegistryConfig};
use crate::paths::AppPaths;
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_PROJECT_DEFINITION_BYTES: u64 = 256 * 1024;

pub struct RegistryLock {
    file: File,
}

impl RegistryLock {
    pub fn acquire(paths: &AppPaths) -> Result<Self> {
        paths.ensure()?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(&paths.lock_file)
            .with_context(|| format!("open lock {}", paths.lock_file.display()))?;
        file.lock_exclusive().context("acquire workbench lock")?;
        Ok(Self { file })
    }
}

impl Drop for RegistryLock {
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

pub fn load(paths: &AppPaths) -> Result<RegistryConfig> {
    paths.ensure()?;
    if !paths.config_file.exists() {
        return Ok(RegistryConfig::default());
    }
    let meta = fs::symlink_metadata(&paths.config_file)
        .with_context(|| format!("inspect {}", paths.config_file.display()))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        bail!(
            "registry is not a regular file: {}",
            paths.config_file.display()
        );
    }
    if meta.len() > MAX_CONFIG_BYTES {
        bail!("registry exceeds {MAX_CONFIG_BYTES} bytes");
    }
    let bytes = fs::read(&paths.config_file).context("read project registry")?;
    let registry: RegistryConfig =
        serde_json::from_slice(&bytes).context("parse project registry")?;
    if registry.schema_version != CONFIG_SCHEMA {
        bail!(
            "unsupported workbench registry schema {}; expected {}",
            registry.schema_version,
            CONFIG_SCHEMA
        );
    }
    Ok(registry)
}

pub fn save(paths: &AppPaths, registry: &RegistryConfig) -> Result<()> {
    paths.ensure()?;
    let bytes = serde_json::to_vec_pretty(registry).context("serialize project registry")?;
    let temporary = paths
        .config_file
        .with_extension(format!("tmp.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temporary)
        .with_context(|| format!("create temporary registry {}", temporary.display()))?;
    let result = (|| -> Result<()> {
        file.write_all(&bytes).context("write project registry")?;
        file.write_all(b"\n").context("finish project registry")?;
        file.sync_all().context("sync project registry")?;
        fs::rename(&temporary, &paths.config_file).context("publish project registry")?;
        fs::set_permissions(&paths.config_file, fs::Permissions::from_mode(0o600))
            .context("set private registry permissions")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub fn find_project<'a>(registry: &'a RegistryConfig, id: &str) -> Result<&'a Project> {
    registry
        .projects
        .iter()
        .find(|project| project.id == id)
        .with_context(|| format!("project '{id}' is not registered"))
}

pub fn add_project(
    paths: &AppPaths,
    supplied_root: &Path,
    supplied_plugin_path: Option<&Path>,
    trust_project_checks: bool,
) -> Result<Project> {
    let _lock = RegistryLock::acquire(paths)?;
    let mut registry = load(paths)?;
    let project_root = supplied_root
        .canonicalize()
        .with_context(|| format!("resolve project root {}", supplied_root.display()))?;
    if !project_root.is_dir() {
        bail!(
            "project root is not a directory: {}",
            project_root.display()
        );
    }

    let definition = read_project_definition(&project_root)?;
    let relative_plugin = supplied_plugin_path
        .map(PathBuf::from)
        .or_else(|| {
            definition
                .as_ref()
                .and_then(|value| value.plugin_path.clone())
        })
        .or_else(|| {
            if project_root.join("manifest.json").is_file() {
                Some(PathBuf::from("."))
            } else if project_root.join("omarchy-plugin/manifest.json").is_file() {
                Some(PathBuf::from("omarchy-plugin"))
            } else {
                None
            }
        })
        .context("could not find manifest.json at the project root or omarchy-plugin/")?;
    validate_relative_plugin_path(&relative_plugin)?;
    let plugin_root = project_root
        .join(&relative_plugin)
        .canonicalize()
        .with_context(|| format!("resolve plugin path {}", relative_plugin.display()))?;
    if !plugin_root.starts_with(&project_root) {
        bail!("plugin path escapes the project root");
    }
    let manifest = crate::manifest::validate_plugin(&plugin_root)?;
    if registry
        .projects
        .iter()
        .any(|project| project.id == manifest.id)
    {
        bail!("project '{}' is already registered", manifest.id);
    }
    if registry
        .projects
        .iter()
        .any(|project| project.project_root == project_root)
    {
        bail!(
            "project root is already registered: {}",
            project_root.display()
        );
    }
    let checks = definition.map(|value| value.checks).unwrap_or_default();
    validate_checks(&checks)?;
    let project = Project {
        id: manifest.id,
        name: manifest.name,
        project_root,
        plugin_root,
        checks,
        project_checks_trusted: trust_project_checks,
        added_at_unix: now_unix(),
    };
    registry.projects.push(project.clone());
    registry
        .projects
        .sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)));
    save(paths, &registry)?;
    Ok(project)
}

pub fn remove_project(paths: &AppPaths, id: &str) -> Result<Project> {
    let _lock = RegistryLock::acquire(paths)?;
    let mut registry = load(paths)?;
    let index = registry
        .projects
        .iter()
        .position(|project| project.id == id)
        .with_context(|| format!("project '{id}' is not registered"))?;
    let project = registry.projects.remove(index);
    save(paths, &registry)?;
    Ok(project)
}

pub fn set_trust(paths: &AppPaths, id: &str, trusted: bool) -> Result<Project> {
    let _lock = RegistryLock::acquire(paths)?;
    let mut registry = load(paths)?;
    let project = registry
        .projects
        .iter_mut()
        .find(|project| project.id == id)
        .with_context(|| format!("project '{id}' is not registered"))?;
    project.project_checks_trusted = trusted;
    let updated = project.clone();
    save(paths, &registry)?;
    Ok(updated)
}

fn read_project_definition(root: &Path) -> Result<Option<ProjectDefinition>> {
    let path = root.join(".omarchy-workbench.json");
    if !path.exists() {
        return Ok(None);
    }
    let meta = fs::symlink_metadata(&path).context("inspect .omarchy-workbench.json")?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        bail!(".omarchy-workbench.json must be a regular file");
    }
    if meta.len() > MAX_PROJECT_DEFINITION_BYTES {
        bail!(".omarchy-workbench.json exceeds {MAX_PROJECT_DEFINITION_BYTES} bytes");
    }
    let definition: ProjectDefinition =
        serde_json::from_slice(&fs::read(&path)?).context("parse .omarchy-workbench.json")?;
    if definition.schema_version != PROJECT_SCHEMA {
        bail!(
            "unsupported project definition schema {}; expected {}",
            definition.schema_version,
            PROJECT_SCHEMA
        );
    }
    Ok(Some(definition))
}

fn validate_relative_plugin_path(path: &Path) -> Result<()> {
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        bail!("plugin path must be relative and remain inside the project");
    }
    Ok(())
}

fn validate_checks(checks: &[crate::model::CheckSpec]) -> Result<()> {
    if checks.len() > 32 {
        bail!("a project may define at most 32 checks");
    }
    for check in checks {
        if check.name.is_empty() || check.name.len() > 80 {
            bail!("check names must be 1-80 characters");
        }
        if check.argv.is_empty() || check.argv.len() > 64 {
            bail!("check '{}' must contain 1-64 argv entries", check.name);
        }
        if check
            .argv
            .iter()
            .any(|arg| arg.contains('\0') || arg.len() > 4096)
        {
            bail!("check '{}' contains an invalid argument", check.name);
        }
        if check.timeout_seconds == 0 || check.timeout_seconds > 1800 {
            bail!(
                "check '{}' timeout must be between 1 and 1800 seconds",
                check.name
            );
        }
    }
    Ok(())
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
