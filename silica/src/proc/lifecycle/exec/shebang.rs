// SPDX-License-Identifier: GPL-2.0

//! Object-Oriented Shebang (`#!`) script parser and interpreter resolution.
//!
//! Follows standard Linux `fs/binfmt_script.c` behavior:
//! - Recognizes `#!` magic header bytes.
//! - Extracts interpreter path and single optional argument.
//! - Strips whitespace and handles CRLF line endings.
//! - Reconstructs `argv` vector: `[interpreter, optional_arg?, script_path, original_argv[1..]]`.
//! - Enforces Linux recursion limits to prevent script loops (`ELOOP`).

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use core::fmt;

use crate::uapi::errno::{Errno, Result};

/// Maximum length in bytes for a shebang header line (matching Linux `BINPRM_BUF_SIZE`).
pub const MAX_SHEBANG_LINE_LEN: usize = 256;

/// Maximum nested shebang recursion depth to prevent infinite loops (matching Linux `BINPRM_MAX_RECURSION`).
pub const MAX_SHEBANG_RECURSION: usize = 4;

/// Shebang magic byte marker (`#!`).
pub const SHEBANG_MAGIC: [u8; 2] = [b'#', b'!'];

/// Encapsulates a parsed Shebang script header with interpreter path and arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shebang {
    /// Path to the binary or script interpreter (e.g., `/bin/sh`).
    interpreter: String,
    /// Single optional argument passed to the interpreter (e.g., `-e` or `-x`).
    arg: Option<String>,
}

impl Shebang {
    /// Creates a new `Shebang` instance with validation.
    pub fn new(interpreter: String, arg: Option<String>) -> Result<Self> {
        let shebang = Self { interpreter, arg };
        shebang.validate()?;
        Ok(shebang)
    }

    /// Checks if a buffer begins with the `#!` magic bytes.
    #[inline]
    pub fn is_shebang(data: &[u8]) -> bool {
        data.len() >= 2 && data[0..2] == SHEBANG_MAGIC
    }

    /// Attempts to parse shebang header from file bytes.
    pub fn parse(data: &[u8]) -> Result<Option<Self>> {
        if !Self::is_shebang(data) {
            return Ok(None);
        }

        // 1. Locate end of the shebang line (newline or EOF within limit)
        let line_limit = data.len().min(MAX_SHEBANG_LINE_LEN);
        let line_bytes = &data[2..line_limit];

        let line_end = line_bytes
            .iter()
            .position(|&b| b == b'\n')
            .unwrap_or(line_bytes.len());

        let raw_line = &line_bytes[..line_end];

        // 2. Convert to UTF-8 str, trimming trailing carriage return or spaces
        let line_str = core::str::from_utf8(raw_line)
            .map_err(|_| Errno::ENOEXEC)?
            .trim_end_matches(|c| c == '\r' || c == ' ' || c == '\t');

        // 3. Skip whitespace between `#!` and interpreter
        let trimmed_start = line_str.trim_start_matches(|c| c == ' ' || c == '\t');
        if trimmed_start.is_empty() {
            crate::return_errno!(ENOEXEC, "empty shebang interpreter path");
        }

        // 4. Extract interpreter and optional argument
        let (interp_str, opt_arg) = match trimmed_start.find(|c| c == ' ' || c == '\t') {
            Some(idx) => {
                let interp = &trimmed_start[..idx];
                let remainder = trimmed_start[idx..].trim_start_matches(|c| c == ' ' || c == '\t');
                let arg = if !remainder.is_empty() {
                    Some(remainder.to_string())
                } else {
                    None
                };
                (interp, arg)
            }
            None => (trimmed_start, None),
        };

        if interp_str.is_empty() {
            crate::return_errno!(ENOEXEC, "missing interpreter in shebang header");
        }

        let instance = Self {
            interpreter: interp_str.to_string(),
            arg: opt_arg,
        };
        instance.validate()?;

        Ok(Some(instance))
    }

    /// Validates the shebang's contents.
    pub fn validate(&self) -> Result<()> {
        if self.interpreter.is_empty() {
            crate::return_errno!(ENOEXEC, "interpreter path cannot be empty");
        }
        if self.interpreter.len() > MAX_SHEBANG_LINE_LEN {
            crate::return_errno!(ENAMETOOLONG, "interpreter path exceeds maximum length");
        }
        if let Some(arg) = &self.arg {
            if arg.len() > MAX_SHEBANG_LINE_LEN {
                crate::return_errno!(E2BIG, "shebang argument exceeds maximum length");
            }
        }
        Ok(())
    }

    /// Returns the interpreter binary path.
    #[inline]
    pub fn interpreter(&self) -> &str {
        &self.interpreter
    }

    /// Returns the optional argument, if any.
    #[inline]
    pub fn arg(&self) -> Option<&str> {
        self.arg.as_deref()
    }

    /// Returns whether the shebang contains an argument.
    #[inline]
    pub fn has_arg(&self) -> bool {
        self.arg.is_some()
    }

    /// Reconstructs the argument vector for executing the interpreter.
    pub fn build_argv<'a>(
        &'a self,
        script_path: &'a str,
        original_argv: &[&'a str],
    ) -> Vec<&'a str> {
        let mut new_argv = Vec::new();
        new_argv.push(self.interpreter());
        if let Some(arg) = self.arg() {
            new_argv.push(arg);
        }
        new_argv.push(script_path);
        if original_argv.len() > 1 {
            new_argv.extend_from_slice(&original_argv[1..]);
        }
        new_argv
    }
}

impl fmt::Display for Shebang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#!{}", self.interpreter)?;
        if let Some(arg) = &self.arg {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}
