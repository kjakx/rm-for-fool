use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use super::metadata::copy_metadata;
use crate::safety::get_device_id;

/// Map from source inode number to the destination path where that inode was
/// first placed during this invocation. Used to preserve hard links when a
/// tree is copied across devices (the fast rename path needs no map because
/// the kernel preserves links atomically).
pub type InodeMap = HashMap<u64, PathBuf>;

/// Checks if an I/O error indicates a cross-device move error
pub(crate) fn is_cross_device_error(err: &io::Error) -> bool {
    if err.kind() == io::ErrorKind::CrossesDevices {
        return true;
    }
    match err.raw_os_error() {
        Some(18) => true, // EXDEV on Linux/macOS
        Some(17) => true, // ERROR_NOT_SAME_DEVICE on Windows
        _ => false,
    }
}

/// Validates that a directory tree can be moved atomically under --one-file-system.
/// Fails if any subdirectory lives on a different device than `root_dev`, or if
/// any part of the tree cannot even be read. Running this *before* copying starts
/// guarantees we never leave the source and destination in a split state.
pub(crate) fn validate_one_file_system_tree(root: &Path, root_dev: u64) -> Result<()> {
    for entry in WalkDir::new(root).follow_links(false).into_iter() {
        let entry =
            entry.with_context(|| format!("failed to walk directory tree '{}'", root.display()))?;
        if !entry.file_type().is_dir() {
            continue;
        }
        if let Some(dev) = get_device_id(entry.path())
            && dev != root_dev
        {
            return Err(anyhow::anyhow!(
                "cannot move '{}' with --one-file-system: subdirectory '{}' is on a different file system",
                root.display(),
                entry.path().display()
            ));
        }
    }
    Ok(())
}

/// Copies a regular file's content plus permissions, timestamps and extended
/// attributes to dst. On Linux holes are preserved (sparse files stay sparse)
/// via SEEK_DATA/SEEK_HOLE; if that is unsupported or fails, it falls back to
/// a plain copy.
pub fn copy_regular_file(src: &Path, dst: &Path, meta: &Metadata) -> Result<()> {
    #[cfg(target_os = "linux")]
    match super::sparse::copy_sparse(src, dst) {
        Ok(()) => {}
        Err(_) => {
            fs::copy(src, dst).with_context(|| {
                format!("failed to copy '{}' to '{}'", src.display(), dst.display())
            })?;
        }
    }
    #[cfg(not(target_os = "linux"))]
    fs::copy(src, dst)
        .with_context(|| format!("failed to copy '{}' to '{}'", src.display(), dst.display()))?;

    copy_metadata(meta, dst, false)?;
    #[cfg(unix)]
    super::xattrs::copy_xattrs(src, dst)?;
    Ok(())
}

/// Moves a single non-directory entry (regular file, symlink, FIFO, socket or
/// device node) to dst. Regular files that share an inode with another file
/// already moved in this invocation are recreated as hard links instead of
/// being copied again.
pub fn move_file_fallback(
    src: &Path,
    dst: &Path,
    meta: &Metadata,
    #[cfg_attr(not(unix), allow(unused_variables))] inode_map: &mut InodeMap,
) -> Result<()> {
    if meta.file_type().is_symlink() {
        let target = fs::read_link(src)
            .with_context(|| format!("failed to read symlink '{}'", src.display()))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            symlink(&target, dst)
                .with_context(|| format!("failed to create symlink at '{}'", dst.display()))?;
        }
        #[cfg(windows)]
        {
            if meta.is_dir() {
                std::os::windows::fs::symlink_dir(&target, dst)?;
            } else {
                std::os::windows::fs::symlink_file(&target, dst)?;
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            return Err(anyhow::anyhow!("symlinks not supported on this platform"));
        }

        copy_metadata(meta, dst, true)?;
        fs::remove_file(src)
            .with_context(|| format!("failed to remove original symlink '{}'", src.display()))?;
        return Ok(());
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let ino = meta.ino();
        if meta.file_type().is_file() {
            match inode_map.get(&ino) {
                Some(prev_dst) => {
                    // Same inode already moved: recreate the hard link.
                    fs::hard_link(prev_dst, dst).with_context(|| {
                        format!(
                            "failed to create hard link '{}' -> '{}'",
                            prev_dst.display(),
                            dst.display()
                        )
                    })?;
                }
                None => {
                    copy_regular_file(src, dst, meta)?;
                    inode_map.insert(ino, dst.to_path_buf());
                }
            }
        } else {
            super::special::move_special_file(src, dst, meta)?;
        }
    }

    #[cfg(not(unix))]
    {
        if !meta.file_type().is_file() {
            return Err(anyhow::anyhow!(
                "cannot move special file '{}' across devices (only same-filesystem rename is supported)",
                src.display()
            ));
        }
        copy_regular_file(src, dst, meta)?;
    }

    fs::remove_file(src)
        .with_context(|| format!("failed to remove original file '{}'", src.display()))?;

    Ok(())
}

/// Recursively copies a directory tree and removes the original (for cross-device directory move)
pub fn move_dir_fallback(
    src: &Path,
    dst: &Path,
    meta: &Metadata,
    one_file_system: bool,
    root_dev: Option<u64>,
    inode_map: &mut InodeMap,
) -> Result<()> {
    if one_file_system
        && matches!((root_dev, get_device_id(src)), (Some(root), Some(current)) if root != current)
    {
        // Skip directories on different file systems
        return Ok(());
    }

    fs::create_dir_all(dst)
        .with_context(|| format!("failed to create directory '{}'", dst.display()))?;

    for entry in
        fs::read_dir(src).with_context(|| format!("failed to read dir '{}'", src.display()))?
    {
        let entry = entry?;
        let child_src = entry.path();
        let child_dst = dst.join(entry.file_name());
        let child_meta = fs::symlink_metadata(&child_src)?;

        if child_meta.file_type().is_dir() {
            move_dir_fallback(
                &child_src,
                &child_dst,
                &child_meta,
                one_file_system,
                root_dev,
                inode_map,
            )?;
        } else {
            move_file_fallback(&child_src, &child_dst, &child_meta, inode_map)?;
        }
    }

    copy_metadata(meta, dst, false)?;
    if let Err(err) = fs::remove_dir(src) {
        // EBUSY (16 on Unix): the directory is an active mount point and cannot be
        // unlinked while mounted. All *contents* have already been moved safely to
        // the trash; only the empty mount-point shell remains in place.
        if err.raw_os_error() == Some(16) {
            return Err(anyhow::anyhow!(
                "moved contents of '{}' to the trash, but could not remove the directory itself: it is an active mount point (unmount it first)",
                src.display()
            ));
        }
        return Err(err)
            .with_context(|| format!("failed to remove original dir '{}'", src.display()));
    }

    Ok(())
}

/// Moves a file, symlink, or directory to the destination path.
/// Tries `std::fs::rename` first (Fast Path). If cross-device error occurs,
/// falls back to recursive copy + metadata sync + remove (Fallback Path).
///
/// `inode_map` is shared across all targets of one invocation so that hard
/// links are preserved even between separately moved top-level entries.
pub fn move_item(
    src: &Path,
    dst: &Path,
    meta: &Metadata,
    one_file_system: bool,
    inode_map: &mut InodeMap,
) -> Result<()> {
    // 1. Fast Path: atomic rename
    match fs::rename(src, dst) {
        Ok(_) => return Ok(()),
        Err(err) if is_cross_device_error(&err) => {
            // Proceed to Fallback Path
        }
        Err(err) => {
            return Err(anyhow::Error::new(err).context(format!(
                "cannot move '{}' to '{}'",
                src.display(),
                dst.display()
            )));
        }
    }

    // 2. Fallback Path: Cross-device copy and remove
    let root_dev = if one_file_system {
        get_device_id(src)
    } else {
        None
    };

    if meta.file_type().is_dir() {
        // Pre-validate the whole tree before touching anything: with
        // --one-file-system a nested mount point would otherwise be skipped,
        // leaving the source partially emptied and a partial copy in the trash.
        if let Some(root) = root_dev {
            validate_one_file_system_tree(src, root)?;
        }
        move_dir_fallback(src, dst, meta, one_file_system, root_dev, inode_map)?;
    } else {
        move_file_fallback(src, dst, meta, inode_map)?;
    }

    Ok(())
}
