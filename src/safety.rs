use std::path::Path;

/// Checks whether the path represents '.' or '..'
pub fn is_dot_or_dotdot(path: &Path) -> bool {
    let s = path.to_string_lossy();
    let trimmed = s.trim_end_matches(['/', '\\']);
    trimmed == "."
        || trimmed == ".."
        || trimmed.ends_with("/.")
        || trimmed.ends_with("\\.")
        || trimmed.ends_with("/..")
        || trimmed.ends_with("\\..")
}

/// Checks whether the target is the root directory '/' (or Windows drive root)
pub fn is_root_directory(path: &Path) -> bool {
    let canonical = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => return false,
    };

    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match (canonical.metadata(), Path::new("/").metadata()) {
            (Ok(target_meta), Ok(root_meta)) => {
                target_meta.dev() == root_meta.dev() && target_meta.ino() == root_meta.ino()
            }
            _ => false,
        }
    }

    #[cfg(windows)]
    {
        use std::path::Component;
        // On Windows canonical paths look like `\\?\C:\`
        let mut components = canonical.components();
        matches!(
            (components.next(), components.next(), components.next()),
            (Some(Component::Prefix(_)), Some(Component::RootDir), None)
        )
    }

    #[cfg(not(any(unix, windows)))]
    {
        canonical == Path::new("/")
    }
}

/// Checks whether moving `path` into the trash would conflict with the trash
/// directory itself. This is true when:
///   * the target equals or is inside `trash_root`, OR
///   * `trash_root` is inside the target (e.g. moving `$HOME` itself, which
///     contains `~/.Trash`). Moving such a directory into its own descendant
///     must be refused explicitly rather than relying on the kernel's EINVAL.
pub fn is_inside_trash(path: &Path, trash_root: &Path) -> bool {
    let can_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let can_trash = trash_root
        .canonicalize()
        .unwrap_or_else(|_| trash_root.to_path_buf());

    can_path == can_trash || can_path.starts_with(&can_trash) || can_trash.starts_with(&can_path)
}

/// Gets the device ID of a path for --one-file-system check
#[cfg(unix)]
pub fn get_device_id(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    path.symlink_metadata().ok().map(|m| m.dev())
}

#[cfg(windows)]
pub fn get_device_id(path: &Path) -> Option<u64> {
    // On Windows, compare drive prefix from canonical path (e.g. 'C' -> hash/byte)
    let canonical = path.canonicalize().ok()?;
    for component in canonical.components() {
        if let std::path::Component::Prefix(prefix) = component {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            prefix.as_os_str().hash(&mut hasher);
            return Some(hasher.finish());
        }
    }
    None
}

#[cfg(not(any(unix, windows)))]
pub fn get_device_id(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_dot_or_dotdot() {
        assert!(is_dot_or_dotdot(Path::new(".")));
        assert!(is_dot_or_dotdot(Path::new("./")));
        assert!(is_dot_or_dotdot(Path::new(".\\")));
        assert!(is_dot_or_dotdot(Path::new("..")));
        assert!(is_dot_or_dotdot(Path::new("../")));
        assert!(is_dot_or_dotdot(Path::new("..\\")));
        assert!(is_dot_or_dotdot(Path::new("dir/.")));
        assert!(is_dot_or_dotdot(Path::new("dir/..")));
        assert!(is_dot_or_dotdot(Path::new("dir/./")));
        assert!(is_dot_or_dotdot(Path::new("dir/../")));
        assert!(is_dot_or_dotdot(Path::new("dir\\.")));
        assert!(is_dot_or_dotdot(Path::new("dir\\..")));
        assert!(is_dot_or_dotdot(Path::new("dir\\.\\")));
        assert!(is_dot_or_dotdot(Path::new("dir\\..\\")));
        assert!(!is_dot_or_dotdot(Path::new("foo.txt")));
        assert!(!is_dot_or_dotdot(Path::new("dir/sub")));
        assert!(!is_dot_or_dotdot(Path::new("...")));
        assert!(!is_dot_or_dotdot(Path::new("dir/...")));
    }
}
