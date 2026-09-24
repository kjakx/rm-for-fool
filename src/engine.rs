use std::fs;
use std::path::Path;
use std::process::ExitCode;

use crate::cli::{CliOptions, PromptMode};
use crate::fs_ops::{InodeMap, is_write_protected, move_item};
use crate::prompt::{Prompter, TerminalPrompter};
use crate::safety::{is_dot_or_dotdot, is_inside_trash, is_root_directory};
use crate::trash::TrashSession;

/// Checks if a directory is empty (contains no entries)
pub fn is_empty_dir(path: &Path) -> bool {
    fs::read_dir(path)
        .map(|mut entries| entries.next().is_none())
        .unwrap_or(false)
}

/// Runs the standard CLI application using `std::env::args_os()`, the default user
/// trash session, and terminal prompts.
pub fn run() -> ExitCode {
    let opts = match CliOptions::parse_from(std::env::args_os()) {
        Ok(opts) => opts,
        Err(e) => {
            e.exit();
        }
    };

    let session = match TrashSession::new() {
        Ok(s) => s,
        Err(err) => {
            eprintln!("rm-for-fool: failed to initialize trash session: {:#}", err);
            return ExitCode::from(1);
        }
    };

    run_with(&opts, &session, &mut TerminalPrompter)
}

/// Core execution engine for `rm-for-fool`. Accepts custom options, trash session,
/// and prompter for maximum testability.
pub fn run_with<P: Prompter>(
    opts: &CliOptions,
    session: &TrashSession,
    prompter: &mut P,
) -> ExitCode {
    if opts.files.is_empty() {
        eprintln!("rm-for-fool: missing operand");
        eprintln!("Try 'rm-for-fool --help' for more information.");
        return ExitCode::from(1);
    }

    // -I (prompt once) handling.
    // GNU rm: prompt before removing *more than* three arguments, or when
    // removing recursively. Exactly three files must NOT trigger a prompt.
    if opts.prompt_mode == PromptMode::Once {
        let count = opts.files.len();
        if (count > 3 || opts.recursive) && !prompter.prompt_once(count, opts.recursive) {
            return ExitCode::SUCCESS;
        }
    }

    let mut has_error = false;
    let mut moved_any = false;
    // Shared across all targets so hard links survive cross-device moves.
    let mut inode_map: InodeMap = std::collections::HashMap::new();

    for target in &opts.files {
        // 1. Check dot or dotdot
        if is_dot_or_dotdot(target) {
            eprintln!(
                "rm-for-fool: refusing to remove '.' or '..' directory: skipping '{}'",
                target.display()
            );
            has_error = true;
            continue;
        }

        // 2. Check root directory
        if opts.preserve_root && is_root_directory(target) {
            eprintln!(
                "rm-for-fool: it is dangerous to operate recursively on '/'\nrm-for-fool: use --no-preserve-root to override this failsafe"
            );
            has_error = true;
            continue;
        }

        // 3. Check inside trash (also catches targets that CONTAIN the trash root)
        if is_inside_trash(target, &session.trash_root) {
            eprintln!(
                "rm-for-fool: cannot remove '{}': refusing to move a directory into or out of the trash directory",
                target.display()
            );
            has_error = true;
            continue;
        }

        // 4. Retrieve metadata (symlink metadata to avoid dereferencing)
        let meta = match fs::symlink_metadata(target) {
            Ok(m) => m,
            Err(err) => {
                if opts.prompt_mode == PromptMode::Never {
                    // -f ignores non-existent files
                    continue;
                }
                eprintln!("rm-for-fool: cannot remove '{}': {}", target.display(), err);
                has_error = true;
                continue;
            }
        };

        let is_dir = meta.file_type().is_dir();

        // 5. Check directory handling options
        if is_dir {
            if !opts.recursive && !opts.dir {
                eprintln!(
                    "rm-for-fool: cannot remove '{}': Is a directory",
                    target.display()
                );
                has_error = true;
                continue;
            }
            if !opts.recursive && opts.dir && !is_empty_dir(target) {
                eprintln!(
                    "rm-for-fool: cannot remove '{}': Directory not empty",
                    target.display()
                );
                has_error = true;
                continue;
            }
        }

        // 6. Interactive / Write-protected prompts
        if opts.prompt_mode == PromptMode::Always {
            if !prompter.prompt_interactive(target, is_dir) {
                continue;
            }
        } else if opts.prompt_mode == PromptMode::Normal
            && is_write_protected(&meta)
            && !prompter.prompt_write_protected(target, is_dir)
        {
            continue;
        }

        // 7. Ensure session dir and generate destination path
        if let Err(err) = session.ensure_session_dir() {
            eprintln!("rm-for-fool: failed to prepare trash directory: {:#}", err);
            has_error = true;
            continue;
        }

        let dst = session.destination_path(target);

        // 8. Execute move
        match move_item(target, &dst, &meta, opts.one_file_system, &mut inode_map) {
            Ok(_) => {
                moved_any = true;
                if opts.verbose {
                    println!("removed '{}' -> '{}'", target.display(), dst.display());
                }
            }
            Err(err) => {
                eprintln!("rm-for-fool: {:#}", err);
                has_error = true;
            }
        }
    }

    // If nothing was actually moved, don't leave an empty session directory
    // behind in ~/.Trash (remove_dir only succeeds on existing *empty* dirs,
    // so any partially copied content is left untouched).
    if !moved_any {
        let _ = fs::remove_dir(&session.session_dir);
    }

    if has_error {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompt::IoPrompter;
    use tempfile::TempDir;

    #[test]
    fn test_engine_missing_operand() {
        let temp = TempDir::new().unwrap();
        let session = TrashSession::new_with_root(temp.path().to_path_buf()).unwrap();
        let opts = CliOptions::parse_from(["rm-for-fool"]).unwrap();
        let mut prompter = IoPrompter::new("".as_bytes(), Vec::new());

        let code = run_with(&opts, &session, &mut prompter);
        assert_eq!(code, ExitCode::from(1));
    }

    #[test]
    fn test_engine_prompt_interactive_decline() {
        let temp = TempDir::new().unwrap();
        let session = TrashSession::new_with_root(temp.path().join("trash")).unwrap();
        let file = temp.path().join("file.txt");
        fs::write(&file, "test").unwrap();

        let opts = CliOptions::parse_from(["rm-for-fool", "-i", file.to_str().unwrap()]).unwrap();
        // User answers 'n' to decline
        let mut prompter = IoPrompter::new("n\n".as_bytes(), Vec::new());

        let code = run_with(&opts, &session, &mut prompter);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(file.exists(), "File should still exist after declining");
    }

    #[test]
    fn test_engine_prompt_interactive_accept() {
        let temp = TempDir::new().unwrap();
        let session = TrashSession::new_with_root(temp.path().join("trash")).unwrap();
        let file = temp.path().join("file.txt");
        fs::write(&file, "test").unwrap();

        let opts = CliOptions::parse_from(["rm-for-fool", "-i", file.to_str().unwrap()]).unwrap();
        // User answers 'y' to accept
        let mut prompter = IoPrompter::new("y\n".as_bytes(), Vec::new());

        let code = run_with(&opts, &session, &mut prompter);
        assert_eq!(code, ExitCode::SUCCESS);
        assert!(!file.exists(), "File should have been moved to trash");
    }
}
