//! Kernel Boot Command-Line Parser & Manager.
//!
//! Provides an object-oriented representation of the bootloader command line,
//! separating environment variables (`KEY=VALUE`) from boot argument tokens.

use alloc::string::String;
use alloc::vec::Vec;

/// Object-oriented representation of the kernel boot command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BootCommandLine {
    raw: String,
    envs: Vec<String>,
    args: Vec<String>,
}

impl BootCommandLine {
    /// Creates a new `BootCommandLine` by reading and parsing the command line
    /// provided by the Limine bootloader via `KERNEL_FILE_REQUEST`.
    pub fn new() -> Self {
        let raw_str = crate::limine::KERNEL_FILE_REQUEST
            .get_response()
            .and_then(|resp| resp.file().string().to_str().ok());

        match raw_str {
            Some(raw) => {
                log::info!("[BootCmdLine] Parsed from Limine: \"{}\"", raw);
                Self::parse(raw)
            }
            None => {
                log::warn!("[BootCmdLine] No boot command line provided by Limine bootloader");
                Self::empty()
            }
        }
    }

    /// Creates a new empty `BootCommandLine`.
    pub const fn empty() -> Self {
        Self {
            raw: String::new(),
            envs: Vec::new(),
            args: Vec::new(),
        }
    }

    /// Parse a raw command-line string into `BootCommandLine`.
    ///
    /// Correctly handles quoted arguments (`"..."` and `'...'`), extracts `KEY=VALUE`
    /// tokens into environment variables, and standalone words into arguments.
    pub fn parse(raw: &str) -> Self {
        let mut envs = Vec::new();
        let mut args = Vec::new();
        let mut chars = raw.chars().peekable();

        while let Some(&c) = chars.peek() {
            if c.is_whitespace() {
                chars.next();
                continue;
            }

            let mut token = String::new();
            let mut in_quote: Option<char> = None;

            while let Some(&ch) = chars.peek() {
                if let Some(q) = in_quote {
                    if ch == q {
                        in_quote = None;
                        chars.next();
                    } else {
                        token.push(ch);
                        chars.next();
                    }
                } else if ch == '"' || ch == '\'' {
                    in_quote = Some(ch);
                    chars.next();
                } else if ch.is_whitespace() {
                    break;
                } else {
                    token.push(ch);
                    chars.next();
                }
            }

            if token.contains('=') {
                envs.push(token);
            } else if !token.is_empty() {
                args.push(token);
            }
        }

        Self {
            raw: String::from(raw),
            envs,
            args,
        }
    }

    /// Returns the raw command line string.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Returns a slice of parsed environment variables (`"KEY=VALUE"`).
    pub fn envs(&self) -> &[String] {
        &self.envs
    }

    /// Clones and returns the environment variables vector.
    pub fn to_envs_vec(&self) -> Vec<String> {
        self.envs.clone()
    }

    /// Returns a slice of boot argument tokens (non-environment flags/parameters).
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Clones and returns the boot arguments vector.
    pub fn to_args_vec(&self) -> Vec<String> {
        self.args.clone()
    }

    /// Searches for an environment variable value by key (e.g., `"PATH"` -> `"/bin:/usr/bin"`).
    pub fn get_env(&self, key: &str) -> Option<&str> {
        for item in &self.envs {
            if let Some((k, v)) = item.split_once('=') {
                if k == key {
                    return Some(v);
                }
            }
        }
        None
    }

    /// Checks if a specific argument flag exists in the boot arguments.
    pub fn has_arg(&self, flag: &str) -> bool {
        self.args.iter().any(|arg| arg == flag)
    }
}
