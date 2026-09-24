use rm_for_fool::cli::{CliOptions, PromptMode};
use rm_for_fool::fs_ops::{InodeMap, move_item};
#[cfg(unix)]
use rm_for_fool::fs_ops::{move_dir_fallback, move_file_fallback};
use rm_for_fool::safety::{is_dot_or_dotdot, is_inside_trash};
use rm_for_fool::trash::TrashSession;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

#[test]
fn test_cli_prompt_mode_precedence() {
    // -f then -i -> Always
    let opts = CliOptions::parse_from(["rm-for-fool", "-f", "-i", "foo.txt"]).unwrap();
    assert_eq!(opts.prompt_mode, PromptMode::Always);

    // -i then -f -> Never
    let opts = CliOptions::parse_from(["rm-for-fool", "-i", "-f", "foo.txt"]).unwrap();
    assert_eq!(opts.prompt_mode, PromptMode::Never);

    // -rf -> recursive = true, prompt_mode = Never
    let opts = CliOptions::parse_from(["rm-for-fool", "-rf", "foo"]).unwrap();
    assert!(opts.recursive);
    assert_eq!(opts.prompt_mode, PromptMode::Never);

    // default -> Normal
    let opts = CliOptions::parse_from(["rm-for-fool", "foo.txt"]).unwrap();
    assert_eq!(opts.prompt_mode, PromptMode::Normal);
    assert!(opts.preserve_root);

    // --no-preserve-root
    let opts = CliOptions::parse_from(["rm-for-fool", "--no-preserve-root", "foo"]).unwrap();
    assert!(!opts.preserve_root);
}

#[test]
fn test_safety_dot_and_dotdot() {
    assert!(is_dot_or_dotdot(&PathBuf::from(".")));
    assert!(is_dot_or_dotdot(&PathBuf::from("..")));
    assert!(is_dot_or_dotdot(&PathBuf::from("dir/.")));
    assert!(is_dot_or_dotdot(&PathBuf::from("dir/..")));
    assert!(!is_dot_or_dotdot(&PathBuf::from("foo.txt")));
    assert!(!is_dot_or_dotdot(&PathBuf::from("dir/sub")));
}

#[test]
fn test_safety_trash_protection() {
    let temp = TempDir::new().unwrap();
    let trash_root = temp.path().join(".Trash");
    fs::create_dir_all(&trash_root).unwrap();

    let inside_file = trash_root.join("some_file.txt");
    fs::write(&inside_file, "data").unwrap();

    let outside_file = temp.path().join("safe.txt");
    fs::write(&outside_file, "data").unwrap();

    assert!(is_inside_trash(&trash_root, &trash_root));
    assert!(is_inside_trash(&inside_file, &trash_root));
    // A directory that CONTAINS the trash root (e.g. $HOME itself) must also be rejected
    assert!(is_inside_trash(temp.path(), &trash_root));
    assert!(!is_inside_trash(&outside_file, &trash_root));
}

#[cfg(unix)]
#[test]
fn test_safety_root_directory() {
    use std::path::Path;
    assert!(rm_for_fool::safety::is_root_directory(Path::new("/")));
    assert!(!rm_for_fool::safety::is_root_directory(Path::new("/tmp")));
    // Non-existent paths are not the root
    assert!(!rm_for_fool::safety::is_root_directory(Path::new(
        "/definitely_not_existing_xyz"
    )));
}

#[test]
fn test_cli_interactive_long_flag() {
    // Bare --interactive must be accepted (and must not swallow the next positional)
    let opts = CliOptions::parse_from(["rm-for-fool", "--interactive", "foo.txt"]).unwrap();
    assert_eq!(opts.prompt_mode, PromptMode::Always);
    assert_eq!(opts.files.len(), 1);

    let opts = CliOptions::parse_from(["rm-for-fool", "--interactive=once", "foo.txt"]).unwrap();
    assert_eq!(opts.prompt_mode, PromptMode::Once);

    let opts = CliOptions::parse_from(["rm-for-fool", "--interactive=never", "foo.txt"]).unwrap();
    assert_eq!(opts.prompt_mode, PromptMode::Never);

    // Invalid WHEN value must be rejected by clap
    assert!(CliOptions::parse_from(["rm-for-fool", "--interactive=bogus", "foo.txt"]).is_err());
}

#[test]
fn test_binary_i_prompt_threshold() {
    let bin_path = env!("CARGO_BIN_EXE_rm-for-fool");
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    // Exactly 3 files: GNU -I does NOT prompt ("more than three")
    let work = home.join("work");
    fs::create_dir_all(&work).unwrap();
    for i in 0..3 {
        fs::write(work.join(format!("f{i}.txt")), "x").unwrap();
    }
    let out = std::process::Command::new(bin_path)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .arg("-I")
        .arg(work.join("f0.txt"))
        .arg(work.join("f1.txt"))
        .arg(work.join("f2.txt"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("arguments?"),
        "must not prompt for exactly 3 files: {stderr}"
    );
    // No prompt => nothing declined => all three were moved
    let sessions = fs::read_dir(home.join(".Trash")).unwrap().count();
    assert_eq!(sessions, 1);

    // 4 files: prompts once; EOF declines => nothing is moved
    let work2 = home.join("work2");
    fs::create_dir_all(&work2).unwrap();
    for i in 0..4 {
        fs::write(work2.join(format!("g{i}.txt")), "x").unwrap();
    }
    let out = std::process::Command::new(bin_path)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .arg("-I")
        .arg(work2.join("g0.txt"))
        .arg(work2.join("g1.txt"))
        .arg(work2.join("g2.txt"))
        .arg(work2.join("g3.txt"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("remove 4 arguments?"),
        "must prompt for 4 files: {stderr}"
    );
    assert!(
        work2.join("g0.txt").exists(),
        "declined move must leave file in place"
    );
}

#[cfg(unix)]
#[test]
fn test_binary_no_orphan_session_on_failed_move() {
    use std::os::unix::fs::PermissionsExt;

    // A move that fails after the session dir was created (rename EACCES because
    // the parent directory is not writable) must not leave an empty UUID dir behind.
    let bin_path = env!("CARGO_BIN_EXE_rm-for-fool");
    let temp = TempDir::new().unwrap();
    let home = temp.path().join("home");
    let rodir = home.join("rodir");
    fs::create_dir_all(&rodir).unwrap();
    let file = rodir.join("f.txt");
    fs::write(&file, "x").unwrap();

    // Skip when running as root: chmod cannot stop root from unlinking.
    if libc_getuid() == 0 {
        return;
    }
    fs::set_permissions(&rodir, fs::Permissions::from_mode(0o555)).unwrap();

    let out = std::process::Command::new(bin_path)
        .env("HOME", &home)
        .arg(&file)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));

    fs::set_permissions(&rodir, fs::Permissions::from_mode(0o755)).unwrap();

    let trash = home.join(".Trash");
    if trash.exists() {
        assert_eq!(
            fs::read_dir(&trash).unwrap().count(),
            0,
            "no orphan session dir allowed"
        );
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn getuid() -> u32;
}

#[cfg(unix)]
fn libc_getuid() -> u32 {
    unsafe { getuid() }
}

#[test]
fn test_trash_session_collision_resolution() {
    let temp = TempDir::new().unwrap();
    let trash_root = temp.path().join(".Trash");
    let session = TrashSession::new_with_root(trash_root).unwrap();
    session.ensure_session_dir().unwrap();

    let p1 = session.destination_path(&PathBuf::from("report.pdf"));
    fs::write(&p1, "first").unwrap();

    let p2 = session.destination_path(&PathBuf::from("sub/report.pdf"));
    assert_ne!(p1, p2);
    assert!(p2.to_string_lossy().contains("report (1).pdf"));
    fs::write(&p2, "second").unwrap();

    let p3 = session.destination_path(&PathBuf::from("other/report.pdf"));
    assert!(p3.to_string_lossy().contains("report (2).pdf"));
}

#[test]
fn test_move_item_file_and_directory() {
    let temp = TempDir::new().unwrap();
    let work_dir = temp.path().join("work");
    let trash_dir = temp.path().join("trash");
    fs::create_dir_all(&work_dir).unwrap();
    fs::create_dir_all(&trash_dir).unwrap();

    // 1. Move single file
    let file = work_dir.join("test.txt");
    fs::write(&file, "hello trash").unwrap();
    let meta = fs::symlink_metadata(&file).unwrap();
    let dst_file = trash_dir.join("test.txt");

    move_item(&file, &dst_file, &meta, false, &mut InodeMap::new()).unwrap();
    assert!(!file.exists());
    assert!(dst_file.exists());
    assert_eq!(fs::read_to_string(&dst_file).unwrap(), "hello trash");

    // 2. Move directory recursively
    let dir = work_dir.join("my_dir");
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("sub").join("nested.txt"), "nested content").unwrap();

    let dir_meta = fs::symlink_metadata(&dir).unwrap();
    let dst_dir = trash_dir.join("my_dir");

    move_item(&dir, &dst_dir, &dir_meta, false, &mut InodeMap::new()).unwrap();
    assert!(!dir.exists());
    assert!(dst_dir.exists());
    assert_eq!(
        fs::read_to_string(dst_dir.join("sub").join("nested.txt")).unwrap(),
        "nested content"
    );
}

#[cfg(unix)]
#[test]
fn test_fallback_preserves_hard_links_and_xattrs() {
    use std::os::unix::fs::MetadataExt;

    let temp = TempDir::new().unwrap();
    let src_root = temp.path().join("src");
    let dst_root = temp.path().join("dst");
    fs::create_dir_all(src_root.join("sub")).unwrap();

    // a.txt and sub/b.txt are hard-linked; c.txt is independent.
    let f1 = src_root.join("a.txt");
    fs::write(&f1, "linked content").unwrap();
    let f2 = src_root.join("sub").join("b.txt");
    fs::hard_link(&f1, &f2).unwrap();
    let f3 = src_root.join("c.txt");
    fs::write(&f3, "independent").unwrap();

    // A user xattr must survive the cross-device copy.
    xattr::set(&f1, "user.rm-for-fool-test", b"hello").unwrap();

    let meta = fs::symlink_metadata(&src_root).unwrap();
    move_dir_fallback(
        &src_root,
        &dst_root,
        &meta,
        false,
        None,
        &mut InodeMap::new(),
    )
    .unwrap();

    // Contents preserved
    assert_eq!(
        fs::read_to_string(dst_root.join("a.txt")).unwrap(),
        "linked content"
    );
    assert_eq!(
        fs::read_to_string(dst_root.join("sub").join("b.txt")).unwrap(),
        "linked content"
    );
    assert_eq!(
        fs::read_to_string(dst_root.join("c.txt")).unwrap(),
        "independent"
    );

    // Hard link preserved: same inode at the destination, nlink == 2.
    let m1 = fs::metadata(dst_root.join("a.txt")).unwrap();
    let m2 = fs::metadata(dst_root.join("sub").join("b.txt")).unwrap();
    assert_eq!(m1.ino(), m2.ino());
    assert_eq!(m1.nlink(), 2);

    // xattr preserved on both links.
    let got = xattr::get(dst_root.join("a.txt"), "user.rm-for-fool-test").unwrap();
    assert_eq!(got.as_deref(), Some(b"hello".as_slice()));
}

#[cfg(unix)]
#[test]
fn test_fallback_preserves_hard_links_across_top_level_moves() {
    use std::os::unix::fs::MetadataExt;

    let temp = TempDir::new().unwrap();
    let src1 = temp.path().join("x.txt");
    let src2 = temp.path().join("y.txt");
    fs::write(&src1, "shared").unwrap();
    fs::hard_link(&src1, &src2).unwrap();

    // Two separate top-level moves sharing one inode map (as main.rs does).
    let mut map = InodeMap::new();
    let dst1 = temp.path().join("trash_x.txt");
    move_item(
        &src1,
        &dst1,
        &fs::symlink_metadata(&src1).unwrap(),
        false,
        &mut map,
    )
    .unwrap();
    let dst2 = temp.path().join("trash_y.txt");
    move_item(
        &src2,
        &dst2,
        &fs::symlink_metadata(&src2).unwrap(),
        false,
        &mut map,
    )
    .unwrap();

    assert_eq!(
        fs::metadata(&dst1).unwrap().ino(),
        fs::metadata(&dst2).unwrap().ino()
    );
}

#[cfg(target_os = "linux")]
#[test]
fn test_fallback_preserves_sparse_file() {
    use std::os::unix::fs::MetadataExt;

    let temp = TempDir::new().unwrap();
    let src_root = temp.path().join("src");
    let dst_root = temp.path().join("dst");
    fs::create_dir_all(&src_root).unwrap();

    // 1 MiB sparse file: one byte at offset 0, one byte near the end.
    let src_file = src_root.join("sparse.bin");
    {
        use std::io::{Seek, SeekFrom, Write};
        let mut f = fs::File::create(&src_file).unwrap();
        f.write_all(b"A").unwrap();
        f.seek(SeekFrom::Start(1024 * 1024 - 1)).unwrap();
        f.write_all(b"Z").unwrap();
    }
    let src_blocks = fs::metadata(&src_file).unwrap().blocks(); // should be tiny
    assert!(
        src_blocks < 32,
        "test setup: source file is not sparse ({} blocks)",
        src_blocks
    );

    let meta = fs::symlink_metadata(&src_root).unwrap();
    move_dir_fallback(
        &src_root,
        &dst_root,
        &meta,
        false,
        None,
        &mut InodeMap::new(),
    )
    .unwrap();

    let dst_file = dst_root.join("sparse.bin");
    // Content preserved.
    {
        use std::io::{Read, Seek, SeekFrom};
        let mut f = fs::File::open(&dst_file).unwrap();
        let mut b = [0u8; 1];
        f.read_exact(&mut b).unwrap();
        assert_eq!(&b[..], b"A");
        f.seek(SeekFrom::Start(1024 * 1024 - 1)).unwrap();
        f.read_exact(&mut b).unwrap();
        assert_eq!(&b[..], b"Z");
    }
    // Apparent size preserved and the file is still sparse.
    let dm = fs::metadata(&dst_file).unwrap();
    assert_eq!(dm.len(), 1024 * 1024);
    assert!(
        dm.blocks() < 32,
        "sparse file was materialized: {} blocks",
        dm.blocks()
    );
}

#[cfg(unix)]
#[test]
fn test_fallback_recreates_fifo_and_socket() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let src_dir = temp.path().join("src");
    let dst_dir = temp.path().join("dst");
    // In production the session dir (parent of every dst) exists already.
    fs::create_dir_all(&dst_dir).unwrap();
    fs::create_dir_all(&src_dir).unwrap();

    // FIFO via libc (same call the implementation uses).
    let fifo_src = src_dir.join("pipe.fifo");
    {
        let mut bytes = fifo_src.as_os_str().as_bytes().to_vec();
        bytes.push(0);
        assert_eq!(
            unsafe { libc::mkfifo(bytes.as_ptr() as *const _, 0o644) },
            0
        );
    }
    // Socket via std (a bound listener is a valid unix socket file).
    let sock_src = src_dir.join("test.sock");
    std::os::unix::net::UnixListener::bind(&sock_src).unwrap();

    for name in ["pipe.fifo", "test.sock"] {
        let s = src_dir.join(name);
        let d = dst_dir.join(name);
        move_file_fallback(
            &s,
            &d,
            &fs::symlink_metadata(&s).unwrap(),
            &mut InodeMap::new(),
        )
        .unwrap();
        assert!(!s.exists());
        assert!(d.exists());
    }

    // File types preserved at the destination.
    let fifo_mode = fs::symlink_metadata(dst_dir.join("pipe.fifo"))
        .unwrap()
        .permissions()
        .mode()
        & 0o170_000;
    assert_eq!(fifo_mode, 0o010_000); // S_IFIFO
    let sock_mode = fs::symlink_metadata(dst_dir.join("test.sock"))
        .unwrap()
        .permissions()
        .mode()
        & 0o170_000;
    assert_eq!(sock_mode, 0o140_000); // S_IFSOCK
}

#[test]
fn test_binary_missing_operand() {
    let bin_path = env!("CARGO_BIN_EXE_rm-for-fool");
    let output = std::process::Command::new(bin_path).output().unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing operand"));
}

#[test]
fn test_binary_force_nonexistent() {
    let bin_path = env!("CARGO_BIN_EXE_rm-for-fool");
    let output = std::process::Command::new(bin_path)
        .args(["-f", "definitely_nonexistent_file_xyz.txt"])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn test_binary_dir_without_recursive() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("some_dir");
    fs::create_dir(&dir).unwrap();

    let bin_path = env!("CARGO_BIN_EXE_rm-for-fool");
    let output = std::process::Command::new(bin_path)
        .arg(&dir)
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Is a directory"));
    assert!(dir.exists());
}

#[test]
fn test_binary_dot_rejected() {
    let bin_path = env!("CARGO_BIN_EXE_rm-for-fool");
    let output = std::process::Command::new(bin_path)
        .arg(".")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("refusing to remove '.' or '..' directory"));
}
