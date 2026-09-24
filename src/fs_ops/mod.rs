pub mod fallback;
pub mod metadata;
#[cfg(target_os = "linux")]
pub(crate) mod sparse;
#[cfg(unix)]
pub(crate) mod special;
#[cfg(unix)]
pub(crate) mod xattrs;

pub use fallback::{InodeMap, copy_regular_file, move_dir_fallback, move_file_fallback, move_item};
pub use metadata::is_write_protected;
