use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptMode {
    Never,  // -f: confirm nothing, ignore non-existent
    Always, // -i: prompt before every removal
    Once,   // -I: prompt once before removing more than three files, or when removing recursively
    Normal, // default: prompt only for write-protected files
}

#[derive(Debug, Clone)]
pub struct CliOptions {
    pub recursive: bool,
    pub dir: bool,
    pub verbose: bool,
    pub preserve_root: bool,
    pub one_file_system: bool,
    pub prompt_mode: PromptMode,
    pub files: Vec<PathBuf>,
}

impl CliOptions {
    pub fn parse_from<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let raw_args: Vec<OsString> = args.into_iter().map(|a| a.into()).collect();

        // 1. Scan for the last prompt flag (-f, -i, -I) and root protection flag
        // following GNU rm's "last specified flag wins" rule.
        let (prompt_mode, preserve_root) = scan_precedence_flags(&raw_args);

        // 2. Parse general options using clap
        use clap::{Arg, ArgAction, Command};

        let cmd = Command::new("rm-for-fool")
            .version(env!("CARGO_PKG_VERSION"))
            .about("A safe rm replacement that moves files to ~/.Trash/<UUID>/")
            .arg(
                Arg::new("recursive")
                    .short('r')
                    .short_alias('R')
                    .long("recursive")
                    .action(ArgAction::SetTrue)
                    .help("remove directories and their contents recursively"),
            )
            .arg(
                Arg::new("dir")
                    .short('d')
                    .long("dir")
                    .action(ArgAction::SetTrue)
                    .help("remove empty directories"),
            )
            .arg(
                Arg::new("force")
                    .short('f')
                    .long("force")
                    .action(ArgAction::SetTrue)
                    .help("ignore nonexistent files and arguments, never prompt"),
            )
            .arg(
                Arg::new("interactive_always")
                    .short('i')
                    .action(ArgAction::SetTrue)
                    .help("prompt before every removal"),
            )
            .arg(
                Arg::new("interactive_once")
                    .short('I')
                    .action(ArgAction::SetTrue)
                    .help("prompt once before removing more than three files, or when removing recursively"),
            )
            // NOTE: prompt_mode itself is decided by the manual pre-scan above
            // (GNU "last flag wins" rule across -f/-i/-I). This clap arg exists so
            // that `--interactive[=WHEN]` is validated instead of rejected, and so
            // it never swallows a following positional argument.
            .arg(
                Arg::new("interactive_long")
                    .long("interactive")
                    .num_args(0..=1)
                    .require_equals(true)
                    .default_missing_value("always")
                    .value_parser(["always", "once", "never"])
                    .help("prompt according to WHEN (always, never, or once)"),
            )
            .arg(
                Arg::new("verbose")
                    .short('v')
                    .long("verbose")
                    .action(ArgAction::SetTrue)
                    .help("explain what is being done"),
            )
            .arg(
                Arg::new("preserve_root")
                    .long("preserve-root")
                    .action(ArgAction::SetTrue)
                    .help("do not remove '/' (default)"),
            )
            .arg(
                Arg::new("no_preserve_root")
                    .long("no-preserve-root")
                    .action(ArgAction::SetTrue)
                    .help("do not treat '/' specially"),
            )
            .arg(
                Arg::new("one_file_system")
                    .long("one-file-system")
                    .action(ArgAction::SetTrue)
                    .help("when removing a hierarchy recursively, skip any directory that is on a different file system"),
            )
            .arg(
                Arg::new("files")
                    .action(ArgAction::Append)
                    .num_args(0..)
                    .value_name("FILE")
                    .value_parser(clap::value_parser!(PathBuf))
                    .help("files or directories to remove"),
            );

        let matches = cmd.try_get_matches_from(raw_args)?;

        let recursive = matches.get_flag("recursive");
        let dir = matches.get_flag("dir");
        let verbose = matches.get_flag("verbose");
        let one_file_system = matches.get_flag("one_file_system");
        let files = matches
            .get_many::<PathBuf>("files")
            .unwrap_or_default()
            .cloned()
            .collect();

        Ok(Self {
            recursive,
            dir,
            verbose,
            preserve_root,
            one_file_system,
            prompt_mode,
            files,
        })
    }
}

/// Scans raw arguments to determine the final `PromptMode` and `preserve_root` status.
/// Following GNU `rm`, the last specified flag wins.
pub fn scan_precedence_flags(args: &[OsString]) -> (PromptMode, bool) {
    let mut prompt_mode = PromptMode::Normal;
    let mut preserve_root = true; // default enabled
    let mut in_options = true;

    for arg in args.iter().skip(1) {
        let s = match arg.to_str() {
            Some(s) => s,
            None => continue,
        };

        if s == "--" {
            in_options = false;
            continue;
        }

        if in_options && s.starts_with('-') && s != "-" {
            if s == "--preserve-root" {
                preserve_root = true;
            } else if s == "--no-preserve-root" {
                preserve_root = false;
            } else if s.starts_with("--interactive") {
                if s == "--interactive" || s == "--interactive=always" {
                    prompt_mode = PromptMode::Always;
                } else if s == "--interactive=once" {
                    prompt_mode = PromptMode::Once;
                } else if s == "--interactive=never" {
                    prompt_mode = PromptMode::Never;
                }
            } else if s == "--force" {
                prompt_mode = PromptMode::Never;
            } else if !s.starts_with("--") {
                // Short options cluster, e.g. -rf, -ir, -vI
                for c in s[1..].chars() {
                    match c {
                        'f' => prompt_mode = PromptMode::Never,
                        'i' => prompt_mode = PromptMode::Always,
                        'I' => prompt_mode = PromptMode::Once,
                        _ => {}
                    }
                }
            }
        }
    }

    (prompt_mode, preserve_root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_precedence_last_flag_wins() {
        let args = vec![
            OsString::from("rm-for-fool"),
            OsString::from("-f"),
            OsString::from("-i"),
        ];
        let (mode, root) = scan_precedence_flags(&args);
        assert_eq!(mode, PromptMode::Always);
        assert!(root);

        let args2 = vec![
            OsString::from("rm-for-fool"),
            OsString::from("-i"),
            OsString::from("-f"),
        ];
        let (mode2, _) = scan_precedence_flags(&args2);
        assert_eq!(mode2, PromptMode::Never);
    }

    #[test]
    fn test_precedence_cluster_ordering() {
        let args = vec![OsString::from("rm-for-fool"), OsString::from("-rif")];
        let (mode, _) = scan_precedence_flags(&args);
        assert_eq!(mode, PromptMode::Never);

        let args2 = vec![OsString::from("rm-for-fool"), OsString::from("-rfi")];
        let (mode2, _) = scan_precedence_flags(&args2);
        assert_eq!(mode2, PromptMode::Always);
    }

    #[test]
    fn test_precedence_preserve_root() {
        let args = vec![
            OsString::from("rm-for-fool"),
            OsString::from("--no-preserve-root"),
        ];
        let (_, root) = scan_precedence_flags(&args);
        assert!(!root);

        let args2 = vec![
            OsString::from("rm-for-fool"),
            OsString::from("--no-preserve-root"),
            OsString::from("--preserve-root"),
        ];
        let (_, root2) = scan_precedence_flags(&args2);
        assert!(root2);
    }

    #[test]
    fn test_precedence_double_dash_stops_parsing() {
        let args = vec![
            OsString::from("rm-for-fool"),
            OsString::from("--"),
            OsString::from("-f"),
        ];
        let (mode, _) = scan_precedence_flags(&args);
        assert_eq!(mode, PromptMode::Normal);
    }
}
