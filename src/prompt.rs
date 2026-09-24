use std::io::{self, BufRead, Write};
use std::path::Path;

/// Trait for user confirmation prompts
pub trait Prompter {
    fn ask(&mut self, prompt_msg: &str) -> bool;

    fn prompt_interactive(&mut self, path: &Path, is_dir: bool) -> bool {
        let target_type = if is_dir { "directory" } else { "file" };
        let msg = format!("rm-for-fool: remove {} '{}'? ", target_type, path.display());
        self.ask(&msg)
    }

    fn prompt_once(&mut self, count: usize, is_recursive: bool) -> bool {
        let msg = if is_recursive {
            "rm-for-fool: remove all arguments recursively? ".to_string()
        } else {
            format!("rm-for-fool: remove {} arguments? ", count)
        };
        self.ask(&msg)
    }

    fn prompt_write_protected(&mut self, path: &Path, is_dir: bool) -> bool {
        let target_type = if is_dir {
            "write-protected directory"
        } else {
            "write-protected regular file"
        };
        let msg = format!("rm-for-fool: remove {} '{}'? ", target_type, path.display());
        self.ask(&msg)
    }
}

/// Standard terminal prompter using stdin and stderr
#[derive(Debug, Default, Clone, Copy)]
pub struct TerminalPrompter;

impl Prompter for TerminalPrompter {
    fn ask(&mut self, prompt_msg: &str) -> bool {
        let stderr = io::stderr();
        let mut handle = stderr.lock();
        let _ = write!(handle, "{}", prompt_msg);
        let _ = handle.flush();

        let stdin = io::stdin();
        let mut line = String::new();
        let mut reader = stdin.lock();
        if reader.read_line(&mut line).is_err() {
            return false;
        }

        is_yes(&line)
    }
}

/// IO-based prompter allowing injected reader and writer (ideal for testing)
pub struct IoPrompter<R, W> {
    reader: R,
    writer: W,
}

impl<R: BufRead, W: Write> IoPrompter<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self { reader, writer }
    }
}

impl<R: BufRead, W: Write> Prompter for IoPrompter<R, W> {
    fn ask(&mut self, prompt_msg: &str) -> bool {
        let _ = write!(self.writer, "{}", prompt_msg);
        let _ = self.writer.flush();

        let mut line = String::new();
        if self.reader.read_line(&mut line).is_err() {
            return false;
        }

        is_yes(&line)
    }
}

/// Returns true if user enters 'y' or 'yes' (case-insensitive).
fn is_yes(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.eq_ignore_ascii_case("y") || trimmed.eq_ignore_ascii_case("yes")
}

// Backwards-compatible standalone functions

/// Prompt for -i (interactive always)
pub fn prompt_interactive(path: &Path, is_dir: bool) -> bool {
    TerminalPrompter.prompt_interactive(path, is_dir)
}

/// Prompt for -I (interactive once)
pub fn prompt_once(count: usize, is_recursive: bool) -> bool {
    TerminalPrompter.prompt_once(count, is_recursive)
}

/// Prompt for write-protected file when not in -f mode
pub fn prompt_write_protected(path: &Path, is_dir: bool) -> bool {
    TerminalPrompter.prompt_write_protected(path, is_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_prompter_yes_variations() {
        for yes_input in ["y\n", "Y\n", "yes\n", "YES\n", "  Yes  \n"] {
            let mut output = Vec::new();
            let mut prompter = IoPrompter::new(yes_input.as_bytes(), &mut output);
            assert!(
                prompter.ask("remove? "),
                "Failed for input: {:?}",
                yes_input
            );
            assert_eq!(String::from_utf8(output).unwrap(), "remove? ");
        }
    }

    #[test]
    fn test_io_prompter_no_and_eof() {
        for no_input in ["n\n", "no\n", "anything\n", "\n", ""] {
            let mut output = Vec::new();
            let mut prompter = IoPrompter::new(no_input.as_bytes(), &mut output);
            assert!(
                !prompter.ask("remove? "),
                "Should be false for input: {:?}",
                no_input
            );
        }
    }

    #[test]
    fn test_io_prompter_messages() {
        let mut output = Vec::new();
        let mut prompter = IoPrompter::new("y\n".as_bytes(), &mut output);
        prompter.prompt_interactive(Path::new("test.txt"), false);
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "rm-for-fool: remove file 'test.txt'? "
        );
    }
}
