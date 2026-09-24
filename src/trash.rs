use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct TrashSession {
    pub trash_root: PathBuf,
    pub session_dir: PathBuf,
    pub session_id: Uuid,
    created: Arc<AtomicBool>,
}

impl TrashSession {
    /// Initialize a new trash session in ~/.Trash/<UUID>
    pub fn new() -> Result<Self> {
        let trash_root = if let Some(custom) = std::env::var_os("RM_TRASH_ROOT") {
            PathBuf::from(custom)
        } else if let Some(home_env) = std::env::var_os("HOME") {
            PathBuf::from(home_env).join(".Trash")
        } else {
            let home = dirs::home_dir().context("failed to determine user home directory")?;
            home.join(".Trash")
        };
        Self::new_with_root(trash_root)
    }

    /// Initialize a new trash session with custom root (useful for testing)
    pub fn new_with_root(trash_root: PathBuf) -> Result<Self> {
        let session_id = Uuid::new_v4();
        let session_dir = trash_root.join(session_id.to_string());
        Ok(Self {
            trash_root,
            session_dir,
            session_id,
            created: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Ensure the session directory ~/.Trash/<UUID> exists.
    /// On Unix it is created with mode 0700 so that other local users cannot
    /// list what has been "deleted" (freedesktop trash convention).
    pub fn ensure_session_dir(&self) -> Result<()> {
        if !self.created.load(Ordering::Acquire) {
            std::fs::create_dir_all(&self.session_dir).with_context(|| {
                format!(
                    "failed to create trash session directory '{}'",
                    self.session_dir.display()
                )
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&self.session_dir, std::fs::Permissions::from_mode(0o700))
                    .with_context(|| {
                        format!(
                            "failed to set permissions on '{}'",
                            self.session_dir.display()
                        )
                    })?;
            }
            self.created.store(true, Ordering::Release);
        }
        Ok(())
    }

    /// Generate unique destination path inside ~/.Trash/<UUID>/
    /// If a file or directory with the same name already exists in this session,
    /// a suffix ` (1)`, ` (2)`, etc. is appended to prevent collision.
    pub fn destination_path(&self, source_path: &Path) -> PathBuf {
        let file_name = source_path
            .file_name()
            .unwrap_or_else(|| std::ffi::OsStr::new("unknown"));

        let base_dest = self.session_dir.join(file_name);
        if !base_dest.exists() {
            return base_dest;
        }

        // Collision resolution
        let file_stem = source_path
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let extension = source_path.extension().and_then(|s| s.to_str());

        let mut counter = 1;
        loop {
            let new_name = match extension {
                Some(ext) => format!("{} ({}).{}", file_stem, counter, ext),
                None => format!("{} ({})", file_stem, counter),
            };
            let candidate = self.session_dir.join(new_name);
            if !candidate.exists() {
                return candidate;
            }
            counter += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn test_trash_session_is_send_and_sync() {
        assert_send_sync::<TrashSession>();
    }

    #[test]
    fn test_destination_path_no_extension() {
        let temp = TempDir::new().unwrap();
        let session = TrashSession::new_with_root(temp.path().to_path_buf()).unwrap();
        session.ensure_session_dir().unwrap();

        let p1 = session.destination_path(Path::new("Makefile"));
        assert_eq!(p1, session.session_dir.join("Makefile"));
        std::fs::write(&p1, "content").unwrap();

        let p2 = session.destination_path(Path::new("Makefile"));
        assert_eq!(p2, session.session_dir.join("Makefile (1)"));
    }
}
