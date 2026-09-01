use anyhow::{Context, Result, bail};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub home_dir: PathBuf,
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub state_dir: PathBuf,
    pub plugins_dir: PathBuf,
    pub snapshots_dir: PathBuf,
    pub receipts_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub sessions_file: PathBuf,
    pub test_sessions_dir: PathBuf,
    pub handoffs_dir: PathBuf,
    pub evidence_dir: PathBuf,
    pub marketplace_dir: PathBuf,
    pub marketplace_catalog_file: PathBuf,
    pub marketplace_receipts_dir: PathBuf,
    pub marketplace_trash_dir: PathBuf,
    pub publishing_dir: PathBuf,
    pub lock_file: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME is not set")?;
        let config_base = env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config"));
        let state_base = env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/state"));
        Ok(Self::from_bases(home, config_base, state_base))
    }

    pub fn from_bases(home: PathBuf, config_base: PathBuf, state_base: PathBuf) -> Self {
        let config_dir = config_base.join("omarchy/plugin-workbench");
        let state_dir = state_base.join("omarchy/plugin-workbench");
        let sessions_dir = state_dir.join("sessions");
        let marketplace_dir = state_dir.join("marketplace");
        Self {
            home_dir: home.clone(),
            config_file: config_dir.join("projects.json"),
            lock_file: state_dir.join("workbench.lock"),
            plugins_dir: home.join(".config/omarchy/plugins"),
            snapshots_dir: state_dir.join("snapshots"),
            receipts_dir: state_dir.join("deployments"),
            sessions_file: state_dir.join("sessions.json"),
            test_sessions_dir: state_dir.join("test-sessions"),
            handoffs_dir: state_dir.join("handoffs"),
            evidence_dir: state_dir.join("evidence"),
            marketplace_catalog_file: marketplace_dir.join("catalog.json"),
            marketplace_receipts_dir: marketplace_dir.join("receipts"),
            marketplace_trash_dir: marketplace_dir.join("trash"),
            publishing_dir: state_dir.join("publishing"),
            marketplace_dir,
            sessions_dir,
            config_dir,
            state_dir,
        }
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [
            &self.config_dir,
            &self.state_dir,
            &self.snapshots_dir,
            &self.receipts_dir,
            &self.sessions_dir,
            &self.test_sessions_dir,
            &self.handoffs_dir,
            &self.evidence_dir,
            &self.marketplace_dir,
            &self.marketplace_receipts_dir,
            &self.marketplace_trash_dir,
            &self.publishing_dir,
        ] {
            secure_dir(dir)?;
        }
        Ok(())
    }

    pub fn receipt_path(&self, id: &str) -> PathBuf {
        self.receipts_dir.join(format!("{id}.json"))
    }
}

pub fn secure_dir(path: &Path) -> Result<()> {
    if path.exists() {
        let meta = fs::symlink_metadata(path)
            .with_context(|| format!("inspect directory {}", path.display()))?;
        if meta.file_type().is_symlink() {
            bail!("security boundary is a symlink: {}", path.display());
        }
        if !meta.is_dir() {
            bail!("expected a directory: {}", path.display());
        }
    } else {
        fs::create_dir_all(path).with_context(|| format!("create {}", path.display()))?;
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .with_context(|| format!("set private permissions on {}", path.display()))?;
    Ok(())
}
