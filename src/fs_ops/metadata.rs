use anyhow::Result;
use filetime::{FileTime, set_file_times, set_symlink_file_times};
use std::fs::{self, Metadata};
use std::path::Path;

/// Determines if a file is write-protected
pub fn is_write_protected(meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Check if any write bit (user, group, other) is set
        (meta.permissions().mode() & 0o222) == 0
    }
    #[cfg(not(unix))]
    {
        meta.permissions().readonly()
    }
}

/// Preserves timestamps (mtime and atime) and permissions from src to dst
pub(crate) fn copy_metadata(src_meta: &Metadata, dst: &Path, is_symlink: bool) -> Result<()> {
    let atime = FileTime::from_last_access_time(src_meta);
    let mtime = FileTime::from_last_modification_time(src_meta);

    if is_symlink {
        let _ = set_symlink_file_times(dst, atime, mtime);
    } else {
        let _ = set_file_times(dst, atime, mtime);
        let _ = fs::set_permissions(dst, src_meta.permissions());
    }

    Ok(())
}
