# rm-for-fool 🗑️

[![CI](https://github.com/kjakx/rm-for-fool/actions/workflows/rust.yml/badge.svg)](https://github.com/kjakx/rm-for-fool/actions/workflows/rust.yml)
[![Rust Version](https://img.shields.io/badge/rust-1.85%2B-orange.svg)](https://www.rust-lang.org)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)

`rm-for-fool` is a safe, GNU `rm`-compatible command-line utility. Instead of irrevocably unlinking files and directories, it safely quarantines them into `~/.Trash/<UUID>/`.

It provides a transparent safety net for accidental file deletions while preserving metadata, directory hierarchies, hard links, extended attributes, and file structures.

---

## ✨ Key Features

- **GNU `rm` Interface Compatibility**: Supports standard CLI flags including `-r` / `-R`, `-f`, `-i`, `-I`, `-d`, `-v`, `--preserve-root`, `--no-preserve-root`, and `--one-file-system`.
- **Per-Run UUID Quarantine Session**: Each invocation creates an isolated session directory (`~/.Trash/<UUIDv4>/`), making it easy to inspect or recover exactly what was removed in a given execution.
- **Collision Resolution**: If multiple files with identical names are moved in the same session, they are automatically renamed (e.g., `file (1).txt`, `file (2).txt`) to avoid accidental overwrites.
- **Fast Path & Robust Cross-Device (`EXDEV`) Fallback**:
  - Uses atomic `std::fs::rename` when operating within the same filesystem.
  - When crossing filesystem boundaries (`EXDEV`), it falls back to recursive copy + metadata preservation + unlink, safely maintaining:
    - **Hard Links**: Recreates shared inodes across moved targets.
    - **Extended Attributes & POSIX ACLs** (`xattr` on Unix).
    - **Sparse File Holes**: Preserves hole allocations via `lseek(SEEK_DATA/SEEK_HOLE)` on Linux.
    - **Special Files**: Reconstructs FIFOs, Unix domain sockets, and device nodes on Unix.
    - **Timestamps & Permissions**: Preserves `mtime`, `atime`, and permission bits.
- **Multi-Layered Safety Protections**:
  - Rejects `.` and `..` target paths.
  - Protects root directory `/` (and Windows drive roots) by default with `--preserve-root`.
  - Prevents recursive moves into or out of the `.Trash` directory itself.
  - Pre-validates file system boundaries when using `--one-file-system` to prevent split directory states.
- **Cross-Platform**: Fully supported and tested on Linux, macOS, and Windows.

---

## 📥 Installation

### Prerequisites
- [Rust](https://www.rust-lang.org) 1.85+ (Edition 2024)

### Building and Installing from Source

```bash
git clone https://github.com/kjakx/rm-for-fool.git
cd rm-for-fool
cargo install --path .
```

Ensure `~/.cargo/bin` is in your system `PATH`.

---

## 🚀 Usage

Use `rm-for-fool` exactly as you would use standard `rm`:

```bash
# Move a file to the trash
rm-for-fool document.txt

# Verbose removal of multiple files
rm-for-fool -v file1.txt file2.png

# Recursively move a directory
rm-for-fool -r old_project/

# Force removal without prompts (ignores non-existent files)
rm-for-fool -rf build_artifacts/

# Interactive confirmation per item
rm-for-fool -i important.docx

# Prompt once when removing > 3 files or during recursion
rm-for-fool -I *.log

# Remove an empty directory
rm-for-fool -d empty_dir/
```

### Setting an Alias

To protect yourself from unintended data loss, set an alias in your shell configuration (`~/.bashrc`, `~/.zshrc`, or `config.fish`):

```bash
alias rm="rm-for-fool"
```

To invoke the original system `rm` when needed, prepend a backslash:
```bash
\rm temp_file.txt
```

---

## 🗂️ Trash Directory Structure

Quarantined files are placed under `~/.Trash/<UUID>/`:

```text
~/.Trash/
├── 550e8400-e29b-41d4-a716-446655440000/    # Session 1
│   ├── document.pdf
│   └── project/                             # Directory moved recursively
└── 7c9e6679-7425-40de-944b-e07fc1f90ae7/    # Session 2
    ├── report.pdf
    └── report (1).pdf                       # Duplicate name resolved cleanly
```

> **Tip**: The trash directory location can be customized using the `RM_TRASH_ROOT` or `HOME` environment variables.

---

## ⚙️ Supported Options

| Option | Long Option | Description |
|---|---|---|
| `-r`, `-R` | `--recursive` | Remove directories and their contents recursively |
| `-d` | `--dir` | Remove empty directories |
| `-f` | `--force` | Ignore nonexistent files and arguments, never prompt |
| `-i` | | Prompt before every removal |
| `-I` | | Prompt once before removing more than 3 files, or when removing recursively |
| | `--interactive[=WHEN]` | Prompt according to `WHEN` (`always`, `once`, `never`) |
| `-v` | `--verbose` | Explain what is being done |
| | `--preserve-root` | Do not treat `/` specially (default) |
| | `--no-preserve-root` | Do not treat `/` specially |
| | `--one-file-system` | When removing recursively, skip subdirectories on different file systems |
| | `--help` | Print help information |
| | `--version` | Print version information |
| `--` | | End of option parsing |

---

## 🏗️ Architecture & Codebase

The codebase is organized into focused, modular components:

```text
src/
├── lib.rs              # Library root and public module exports
├── main.rs             # CLI binary entrypoint (delegates to engine)
├── cli.rs              # CLI flag definition and GNU precedence scanning
├── engine.rs           # Orchestration engine & execution pipeline
├── prompt.rs           # Prompter trait, TerminalPrompter, and mockable IoPrompter
├── safety.rs           # Safety guards (. / .., root directory, trash containment)
├── trash.rs            # Thread-safe TrashSession & collision resolution
└── fs_ops/             # Filesystem operations module
    ├── mod.rs          # Re-exports and dispatcher
    ├── metadata.rs     # Timestamp sync & permissions
    ├── fallback.rs     # Cross-device move fallback & hard link preserving
    ├── sparse.rs       # Linux-specific sparse file hole copying
    ├── special.rs      # Unix special file (FIFO, socket, mknod) handling
    └── xattrs.rs       # Unix extended attributes & POSIX ACL copying
```

---

## 🧪 Development & Testing

Run all unit and integration tests:
```bash
cargo test
```

Run code formatting and linter checks:
```bash
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
```

---

## 📜 License

This project is dual-licensed under either:
- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
