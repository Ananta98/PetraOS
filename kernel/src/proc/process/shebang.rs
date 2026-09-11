//! Shebang (`#!`) Script Interpreter Support.
//!
//! Provides an OOP representation of the POSIX shebang line, encapsulating
//! interpreter path and optional argument parsing with bounded recursion
//! depth to prevent circular script references.

use alloc::string::String;
use alloc::vec::Vec;
use super::cmdline::CommandLine;

/// Maximum recursion depth for nested shebang scripts (e.g. script → interpreter → script).
/// Linux uses `BINPRM_BUF_SIZE` / 4 recursion passes; we cap at 4.
pub const MAX_SHEBANG_RECURSION: usize = 4;

/// Maximum length of the shebang line (bytes from `#!` to the first newline).
/// Linux caps at 256; we use the same limit.
const MAX_SHEBANG_LINE_LEN: usize = 256;

/// Errors that can occur during shebang parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShebangError {
    /// The shebang line contains invalid UTF-8.
    InvalidUtf8,
    /// The `#!` line is present but contains no interpreter path.
    EmptyInterpreter,
    /// Nested shebang recursion exceeded `MAX_SHEBANG_RECURSION`.
    RecursionLimitExceeded,
}

/// Parsed representation of a `#!interpreter [argument]` shebang line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shebang {
    /// Path to the script interpreter (e.g. `"/bin/sh"`).
    interpreter: String,
    /// Optional single argument passed to the interpreter (e.g. `"-e"`).
    argument: Option<String>,
}

impl Shebang {
    /// Construct a new `Shebang` with an explicit interpreter and optional argument.
    pub fn new(interpreter: String, argument: Option<String>) -> Self {
        Self {
            interpreter,
            argument,
        }
    }

    /// Returns `true` if `data` begins with the shebang magic bytes `#!`.
    pub fn is_shebang(data: &[u8]) -> bool {
        data.len() >= 2 && data[0] == b'#' && data[1] == b'!'
    }

    /// Parse the shebang line from binary data.
    ///
    /// Returns `Ok(Some(shebang))` when a valid `#!interpreter` line is found,
    /// `Ok(None)` when `data` does not start with `#!`, or an error on
    /// malformed content.
    pub fn parse(data: &[u8]) -> Result<Option<Self>, ShebangError> {
        if !Self::is_shebang(data) {
            return Ok(None);
        }

        // Find the end of the first line (LF or end of data), capped at MAX_SHEBANG_LINE_LEN
        let search_limit = core::cmp::min(data.len(), MAX_SHEBANG_LINE_LEN + 2);
        let first_line_end = data[..search_limit]
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(search_limit);

        // Extract the text after `#!` up to the first newline
        let line_bytes = &data[2..first_line_end];
        let line_str = core::str::from_utf8(line_bytes).map_err(|_| ShebangError::InvalidUtf8)?;

        // Strip trailing carriage return (Windows-style line endings)
        let trimmed = line_str.trim();
        if trimmed.is_empty() {
            return Err(ShebangError::EmptyInterpreter);
        }

        let mut parts = trimmed.split_whitespace();
        let interpreter = parts.next().ok_or(ShebangError::EmptyInterpreter)?;
        let argument = parts.next().map(String::from);

        Ok(Some(Self {
            interpreter: String::from(interpreter),
            argument,
        }))
    }

    /// Returns a reference to the interpreter path.
    pub fn interpreter(&self) -> &str {
        &self.interpreter
    }

    /// Returns a reference to the optional interpreter argument.
    pub fn argument(&self) -> Option<&str> {
        self.argument.as_deref()
    }

    /// Build a new `CommandLine` for executing the interpreter with the script.
    ///
    /// The resulting argv is:
    ///   `[interpreter, [argument], script_path, original_argv[1..]]`
    /// Environment variables are preserved from the original command line.
    pub fn build_command_line(
        &self,
        script_path: &str,
        original_cmdline: &CommandLine,
    ) -> CommandLine {
        let mut new_args = Vec::new();

        // argv[0] = interpreter path
        new_args.push(self.interpreter.clone());

        // argv[1] = optional interpreter argument (e.g. "-e")
        if let Some(ref arg) = self.argument {
            new_args.push(arg.clone());
        }

        // argv[next] = the script being executed
        new_args.push(String::from(script_path));

        // Forward remaining arguments from the original command line (skip argv[0])
        if original_cmdline.argc() > 1 {
            for arg in &original_cmdline.args[1..] {
                new_args.push(arg.clone());
            }
        }

        CommandLine::new(new_args, original_cmdline.env.clone())
    }
}
