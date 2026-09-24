# AGENTS.md — Contributor & Agent Guide for `rm-for-fool`

This document provides technical guidance, architectural principles, invariants, and operational workflows for AI agents and human contributors working on the `rm-for-fool` codebase.

---

## 1. Project Overview & Philosophy

`rm-for-fool` is a safe, drop-in replacement for GNU coreutils `rm`. Rather than permanently deleting files or directory hierarchies with `unlink` / `rmdir`, it moves them to an isolated quarantine session directory under `~/.Trash/<UUID>/`.

### Core Goals
1. **Safety First**: Accidental deletions should never result in irrecoverable data loss.
2. **GNU `rm` Compatibility**: Follow GNU `rm` CLI flags, flag precedence ("last flag wins"), prompt behavior, and exit code conventions.
3. **Data & Metadata Fidelity**: Preserve directory trees, timestamps (`mtime`, `atime`), permissions, file holes (sparse files), hard links, extended attributes (`xattr`), and special files (FIFOs, sockets, device nodes) across filesystem boundaries.
4. **Cross-Platform**: Support Linux, macOS, and Windows with target-gated OS features.

---

## 2. Directory Layout & Module Responsibilities

```text
rm-for-fool/
├── .github/workflows/ci.yml # Multi-platform CI (Linux, macOS, Windows)
├── Cargo.toml               # Package dependencies & target-gated deps
├── tests/
│   └── integration_test.rs  # End-to-end integration test suite
└── src/
    ├── lib.rs               # Crate root exporting public modules
    ├── main.rs              # Binary entrypoint delegating to engine::run()
    ├── cli.rs               # Command-line options & GNU precedence scanner
    ├── engine.rs            # Core execution engine & orchestration pipeline
    ├── prompt.rs            # Prompter trait, TerminalPrompter, and IoPrompter
    ├── safety.rs            # Safety filters (. / .., root /, trash self-nesting)
    ├── trash.rs             # Thread-safe TrashSession & collision resolver
    └── fs_ops/              # Low-level filesystem operations
        ├── mod.rs           # fs_ops module entrypoint & public exports
        ├── metadata.rs      # Timestamps and permission sync
        ├── fallback.rs      # Cross-device move fallback & hard link tracking
        ├── sparse.rs        # Linux SEEK_DATA/SEEK_HOLE hole preservation
        ├── special.rs       # Unix FIFO, socket, device node recreation
        └── xattrs.rs        # Unix extended attributes & POSIX ACL copying
```

---

## 3. Architecture & Execution Pipeline

```text
[ CLI Input ] -> [ scan_precedence_flags ] -> [ clap::Command Parser ]
                                                     |
                                                     v
                                             [ CliOptions ]
                                                     |
                                                     v
                                           [ engine::run_with ]
                                                     |
               +-------------------------------------+-------------------------------------+
               |                                     |                                     |
               v                                     v                                     v
     [ Safety Verification ]               [ Confirmation Prompt ]                [ Move Execution ]
  - is_dot_or_dotdot                   - PromptMode::Always                  - Fast Path: fs::rename
  - is_root_directory                  - PromptMode::Once                    - Fallback Path:
  - is_inside_trash                    - Write-protected prompt                * Sparse copy (Linux)
  - --one-file-system check            (via Prompter trait)                    * Inode hard link map
                                                                               * xattr / ACL sync (Unix)
                                                                               * Special files (Unix)
```

### Module Breakdown

- **`cli.rs`**:
  - `scan_precedence_flags(&[OsString]) -> (PromptMode, bool)`: Scans arguments to enforce GNU "last flag wins" precedence for `-f`, `-i`, `-I`, `--interactive=WHEN`, and `--preserve-root` / `--no-preserve-root`.
  - `CliOptions::parse_from(...)`: Uses `clap` for standard argument validation, help formatting, and gathering positional target paths.
- **`engine.rs`**:
  - Encapsulates the execution loop.
  - Decoupled from `std::io::stdin`/`stderr` through `Prompter` dependency injection (`run_with`), allowing fast in-memory unit testing.
  - Automatically cleans up empty `~/.Trash/<UUID>` session folders if nothing was moved.
- **`safety.rs`**:
  - `is_dot_or_dotdot(&Path)`: Rejects removals targeting `.` or `..`.
  - `is_root_directory(&Path)`: Detects `/` on Unix and drive roots (e.g., `\\?\C:\`) on Windows.
  - `is_inside_trash(&Path, &Path)`: Prevents recursively moving trash into itself or moving parent dirs containing trash.
  - `get_device_id(&Path)`: Obtains filesystem device IDs for `--one-file-system` validation.
- **`trash.rs`**:
  - `TrashSession`: Manages the session directory `~/.Trash/<UUIDv4>/`. Thread-safe (`Send + Sync`).
  - Supports `RM_TRASH_ROOT` and `HOME` environment variable overrides for hermetic testing.
  - Resolves duplicate filename collisions within the same session using numbered suffixes (e.g. `name (1).ext`).
- **`prompt.rs`**:
  - `Prompter` trait for interactive prompts.
  - `TerminalPrompter` for interactive terminal execution.
  - `IoPrompter<R, W>` for testing prompt interactions without spawning processes.
- **`fs_ops/`**:
  - `move_item`: Dispatches to `std::fs::rename` first; on `EXDEV` (`CrossesDevices`), switches to fallback copying and source deletion.
  - Preserves hard links across targets using `InodeMap` (`HashMap<u64, PathBuf>`).

---

## 4. Key Invariants & Behavioral Rules

1. **Never Permanently Delete Target Files**:
   - Targets must be moved to `~/.Trash/<UUID>/`. Direct unlinking of user targets without moving to trash is strictly forbidden.
2. **GNU Flag Precedence**:
   - If multiple contradictory flags are passed (e.g., `-f` then `-i`, or `-rif`), the rightmost flag MUST take precedence.
3. **Preserve Root Directory by Default**:
   - Deletion of `/` (or Windows drive root) must be rejected unless `--no-preserve-root` is explicitly given.
4. **No Orphan Session Directories**:
   - If an invocation moves zero files (e.g. all targets declined or failed validation), the created session directory in `~/.Trash/<UUID>` must be cleanly removed.
5. **Cross-Platform Portability**:
   - Any dependency or syscall that is Unix-specific (like `xattr`, `libc`, `mkfifo`, `lseek`) must be target-gated (`#[cfg(unix)]` or `#[cfg(target_os = "linux")]`).
   - Windows must never fail to build or run.

---

## 5. Development & Testing Commands

### Build & Check
```bash
# Check across all targets
cargo check --all-targets

# Compile in release mode
cargo build --release
```

### Run Tests
```bash
# Run unit and integration tests
cargo test

# Run a specific test
cargo test -- test_cli_prompt_mode_precedence
```

### Lint & Format
```bash
# Verify formatting
cargo fmt -- --check

# Format code
cargo fmt

# Run Clippy with warnings denied
cargo clippy --all-targets -- -D warnings
```

---

## 6. Guidelines for Making Changes

- **Adding CLI Flags**: Update both `scan_precedence_flags` (if mutual override applies) and the `clap::Command` builder in `src/cli.rs`. Add unit tests in `src/cli.rs`.
- **Filesystem Changes**: Add new OS-specific filesystem routines under `src/fs_ops/` with proper `#[cfg(...)]` gating. Ensure fallback paths maintain metadata integrity.
- **Testing Safety**: Whenever adding safety checks or error conditions, write tests both as unit tests in `src/` and as integration tests in `tests/integration_test.rs`.
