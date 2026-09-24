#[cfg(unix)]
use anyhow::{Context, Result};
#[cfg(unix)]
use std::path::Path;

/// Copies extended attributes from src to dst. On Linux this also preserves
/// POSIX ACLs (stored as `system.posix_acl_*` xattrs). Attributes the current
/// user may not read or set (`security.*`, `trusted.*`) are skipped
/// best-effort instead of failing the whole move.
#[cfg(unix)]
pub(crate) fn copy_xattrs(src: &Path, dst: &Path) -> Result<()> {
    let names = xattr::list(src)
        .with_context(|| format!("failed to list xattrs of '{}'", src.display()))?;

    for name in names {
        let label = name.to_string_lossy().into_owned();
        let privileged = label.starts_with("security.") || label.starts_with("trusted.");

        let value = match xattr::get(src, &name) {
            Ok(Some(v)) => v,
            Ok(None) => continue, // vanished between list and get; ignore
            Err(err) if privileged && err.raw_os_error() == Some(13) => continue, // EPERM/EACCES
            Err(err) => {
                return Err(anyhow::Error::new(err)).with_context(|| {
                    format!("failed to read xattr '{}' of '{}'", label, src.display())
                });
            }
        };

        if let Err(err) = xattr::set(dst, &name, &value) {
            // e.g. security.selinux / capability labels when running as non-root
            if privileged && err.raw_os_error() == Some(13) {
                continue;
            }
            return Err(anyhow::Error::new(err)).with_context(|| {
                format!("failed to set xattr '{}' on '{}'", label, dst.display())
            });
        }
    }

    Ok(())
}
