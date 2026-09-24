#[cfg(unix)]
use anyhow::{Context, Result};
#[cfg(unix)]
use filetime::{FileTime, set_symlink_file_times};
#[cfg(unix)]
use std::fs::{self, Metadata};
#[cfg(unix)]
use std::io;
#[cfg(unix)]
use std::path::Path;

/// Recreates a special file (FIFO, socket, device node) at dst and preserves
/// its permissions/timestamps. Device nodes require root privileges.
#[cfg(unix)]
pub(crate) fn move_special_file(src: &Path, dst: &Path, meta: &Metadata) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    let mode = meta.permissions().mode();
    let kind = mode & 0o170_000;

    // NUL-terminated path for libc calls (handles non-UTF8 names).
    let mut c_path = dst.as_os_str().as_bytes().to_vec();
    c_path.push(0);

    match kind {
        0o010_000 => {
            /* S_IFIFO */
            let ret = unsafe {
                libc::mkfifo(c_path.as_ptr() as *const _, (mode & 0o7777) as libc::mode_t)
            };
            if ret != 0 {
                return Err(io::Error::last_os_error())
                    .with_context(|| format!("failed to create FIFO at '{}'", dst.display()));
            }
        }
        0o140_000 => {
            /* S_IFSOCK: bind an empty unix socket */
            std::os::unix::net::UnixListener::bind(dst)
                .with_context(|| format!("failed to create socket at '{}'", dst.display()))?;
        }
        0o020_000 | 0o060_000 => {
            /* S_IFCHR / S_IFBLK */
            #[cfg(target_os = "linux")]
            {
                // st_rdev is already in the kernel's dev_t encoding.
                let rdev = meta.rdev();
                let ret = unsafe { libc::mknod(c_path.as_ptr() as *const _, mode, rdev) };
                if ret != 0 {
                    return Err(io::Error::last_os_error()).with_context(|| {
                        format!(
                            "failed to create device node '{}' (requires root privileges)",
                            dst.display()
                        )
                    });
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                return Err(anyhow::anyhow!(
                    "device nodes are not supported on this platform ('{}')",
                    src.display()
                ));
            }
        }
        _ => {
            return Err(anyhow::anyhow!(
                "unsupported special file type at '{}'",
                src.display()
            ));
        }
    }

    // NOTE: do NOT use copy_metadata() here. filetime's set_file_times opens
    // the path, and opening a FIFO for reading blocks until a writer appears.
    // utimensat-by-path (set_symlink_file_times) and chmod(2)-by-path need no
    // open, so they are safe for special files.
    let atime = FileTime::from_last_access_time(meta);
    let mtime = FileTime::from_last_modification_time(meta);
    let _ = set_symlink_file_times(dst, atime, mtime);
    let _ = fs::set_permissions(dst, meta.permissions());

    Ok(())
}
