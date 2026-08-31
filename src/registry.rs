use crate::model::{
    CONFIG_SCHEMA, EnvironmentSpec, PROJECT_SCHEMA, Project, ProjectDefinition, RegistryConfig,
    WorkflowSpec,
};
use crate::paths::AppPaths;
use anyhow::{Context, Result, bail};
use fs2::FileExt;
use sha2::{Digest, Sha256};
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
                .and_then(|(value, _)| value.plugin_path.clone())
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
    let definition_digest = definition.as_ref().map(|(_, digest)| digest.clone());
    let (checks, workflows, environment) = definition
        .map(|(value, _)| (value.checks, value.workflows, value.environment))
        .unwrap_or_default();
    validate_checks(&checks)?;
    validate_workflows(&workflows)?;
    validate_environment(&environment)?;
    let project = Project {
        id: manifest.id,
        name: manifest.name,
        project_root,
        plugin_root,
        checks,
        workflows,
        environment,
        project_checks_trusted: trust_project_checks,
        trusted_definition_digest: trust_project_checks
            .then(|| definition_digest.clone())
            .flatten(),
        definition_digest,
        approved_capabilities: Vec::new(),
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

pub fn refresh_project(paths: &AppPaths, id: &str) -> Result<Project> {
    let _lock = RegistryLock::acquire(paths)?;
    let mut registry = load(paths)?;
    let project = registry
        .projects
        .iter_mut()
        .find(|project| project.id == id)
        .with_context(|| format!("project '{id}' is not registered"))?;
    let definition = read_project_definition(&project.project_root)?;
    let fallback = project
        .plugin_root
        .strip_prefix(&project.project_root)
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let relative_plugin = definition
        .as_ref()
        .and_then(|(value, _)| value.plugin_path.clone())
        .unwrap_or(fallback);
    validate_relative_plugin_path(&relative_plugin)?;
    let plugin_root = project
        .project_root
        .join(&relative_plugin)
        .canonicalize()
        .with_context(|| format!("resolve plugin path {}", relative_plugin.display()))?;
    if !plugin_root.starts_with(&project.project_root) {
        bail!("plugin path escapes the project root");
    }
    let manifest = crate::manifest::validate_plugin(&plugin_root)?;
    if manifest.id != project.id {
        bail!(
            "refreshed definition resolves to a different plugin id: {}",
            manifest.id
        );
    }
    let digest = definition.as_ref().map(|(_, value)| value.clone());
    let (checks, workflows, environment) = definition
        .map(|(value, _)| (value.checks, value.workflows, value.environment))
        .unwrap_or_default();
    validate_checks(&checks)?;
    validate_workflows(&workflows)?;
    validate_environment(&environment)?;
    project.name = manifest.name;
    project.plugin_root = plugin_root;
    project.checks = checks;
    project.workflows = workflows;
    project.environment = environment;
    project.definition_digest = digest;
    project.project_checks_trusted = false;
    project.trusted_definition_digest = None;
    project.approved_capabilities.clear();
    let updated = project.clone();
    save(paths, &registry)?;
    Ok(updated)
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
    let current = current_definition_digest(&project.project_root)?;
    if current != project.definition_digest {
        bail!("project definition changed; run refresh before trusting it");
    }
    project.trusted_definition_digest = trusted.then(|| current.clone()).flatten();
    let updated = project.clone();
    save(paths, &registry)?;
    Ok(updated)
}

fn read_project_definition(root: &Path) -> Result<Option<(ProjectDefinition, String)>> {
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
    let bytes = fs::read(&path)?;
    let definition: ProjectDefinition =
        serde_json::from_slice(&bytes).context("parse .omarchy-workbench.json")?;
    if definition.schema_version != PROJECT_SCHEMA {
        bail!(
            "unsupported project definition schema {}; expected {}",
            definition.schema_version,
            PROJECT_SCHEMA
        );
    }
    Ok(Some((definition, format!("{:x}", Sha256::digest(&bytes)))))
}

pub fn current_definition_digest(root: &Path) -> Result<Option<String>> {
    Ok(read_project_definition(root)?.map(|(_, digest)| digest))
}

pub fn definition_is_trusted(project: &Project) -> Result<bool> {
    let current = current_definition_digest(&project.project_root)?;
    Ok(project.project_checks_trusted
        && current == project.definition_digest
        && project.trusted_definition_digest == current)
}

pub fn set_capability_approval(
    paths: &AppPaths,
    id: &str,
    capability: &str,
    approved: bool,
) -> Result<Project> {
    validate_capability(capability)?;
    let _lock = RegistryLock::acquire(paths)?;
    let mut registry = load(paths)?;
    let project = registry
        .projects
        .iter_mut()
        .find(|project| project.id == id)
        .with_context(|| format!("project '{id}' is not registered"))?;
    project
        .approved_capabilities
        .retain(|item| item != capability);
    if approved {
        project.approved_capabilities.push(capability.to_owned());
        project.approved_capabilities.sort();
    }
    let updated = project.clone();
    save(paths, &registry)?;
    Ok(updated)
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
            .any(|arg| arg.is_empty() || arg.contains('\0') || arg.len() > 4096)
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

fn validate_workflows(workflows: &[WorkflowSpec]) -> Result<()> {
    if workflows.len() > 32 {
        bail!("a project may define at most 32 workflows");
    }
    let mut names = std::collections::BTreeSet::new();
    for workflow in workflows {
        validate_command(&workflow.name, &workflow.argv, workflow.timeout_seconds)?;
        validate_capability(&workflow.capability)?;
        if !names.insert(&workflow.name) {
            bail!("workflow names must be unique: {}", workflow.name);
        }
        if workflow.requires.len() > 8 {
            bail!(
                "workflow '{}' may require at most 8 capabilities",
                workflow.name
            );
        }
        let mut requirements = std::collections::BTreeSet::new();
        for permission in &workflow.requires {
            validate_capability(permission)?;
            if !requirements.insert(permission) {
                bail!(
                    "workflow '{}' repeats capability '{permission}'",
                    workflow.name
                );
            }
        }
    }
    Ok(())
}

fn validate_environment(environment: &[EnvironmentSpec]) -> Result<()> {
    if environment.len() > 32 {
        bail!("a project may define at most 32 environment requirements");
    }
    for requirement in environment {
        validate_command(&requirement.name, &requirement.argv, 300)?;
    }
    Ok(())
}

fn validate_command(name: &str, argv: &[String], timeout: u64) -> Result<()> {
    if name.is_empty() || name.len() > 80 {
        bail!("command names must be 1-80 characters");
    }
    if argv.is_empty() || argv.len() > 64 {
        bail!("command '{name}' must contain 1-64 argv entries");
    }
    if argv
        .iter()
        .any(|arg| arg.is_empty() || arg.contains('\0') || arg.len() > 4096)
    {
        bail!("command '{name}' contains an invalid argument");
    }
    if timeout == 0 || timeout > 1800 {
        bail!("command '{name}' timeout must be between 1 and 1800 seconds");
    }
    Ok(())
}

fn validate_capability(capability: &str) -> Result<()> {
    const ALLOWED: &[&str] = &[
        "validate",
        "preview",
        "package",
        "release-check",
        "network",
        "publish",
        "deploy",
        "write-outside-project",
    ];
    if !ALLOWED.contains(&capability) {
        bail!("unsupported capability '{capability}'");
    }
    Ok(())
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
