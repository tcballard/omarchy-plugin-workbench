use anyhow::{Context, Result, bail};
use std::env;
use std::ffi::CString;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Component, Path, PathBuf};

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
    pub security_reviews_dir: PathBuf,
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
            security_reviews_dir: state_dir.join("security-reviews"),
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
            &self.security_reviews_dir,
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
    let directory = open_directory(path, true)?;
    // chmod the opened inode, not a path that could have changed after inspection.
    if unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        return Err(std::io::Error::last_os_error()).context("protect private directory");
    }
    Ok(())
}

pub fn open_directory(path: &Path, create: bool) -> Result<OwnedFd> {
    if !path.is_absolute() {
        bail!("security boundary must be absolute: {}", path.display());
    }
    let root = CString::new("/")?;
    let mut fd = open_at(libc::AT_FDCWD, &root)?;
    for component in path.components() {
        let Component::Normal(part) = component else {
            if matches!(component, Component::RootDir) {
                continue;
            }
            bail!("unsafe path component in {}", path.display());
        };
        let name = CString::new(part.as_encoded_bytes())?;
        let next = open_at(fd.as_raw_fd(), &name);
        fd = match next {
            Ok(next) => next,
            Err(error)
                if create
                    && error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                if unsafe { libc::mkdirat(fd.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
                    let error = std::io::Error::last_os_error();
                    if error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(error.into());
                    }
                }
                open_at(fd.as_raw_fd(), &name)?
            }
            Err(error) => {
                return Err(error).with_context(|| format!("unsafe directory {}", path.display()));
            }
        };
    }
    Ok(fd)
}

fn open_at(parent: i32, name: &CString) -> Result<OwnedFd> {
    let raw = unsafe {
        libc::openat(
            parent,
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(raw) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn rejects_linked_ancestor_without_touching_destination() {
        let root = tempfile::tempdir().unwrap();
        let outside = root.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        symlink(&outside, root.path().join("redirect")).unwrap();
        assert!(secure_dir(&root.path().join("redirect/private")).is_err());
        assert!(!outside.join("private").exists());
    }
}
