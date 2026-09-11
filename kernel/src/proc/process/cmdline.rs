//! Process Command-Line Arguments & Environment Management.
//!
//! Unified representation for boot command-line parsing (previously in `bootcmd.rs`)
//! and process argv/envp handling. `CommandLine` is the single source of truth for
//! all argument and environment variable management in PetraOS.

use crate::mm::{UserCStr, UserPtr};
use alloc::string::String;
use alloc::vec::Vec;

/// Represents parsed command line arguments and environment variables for a process.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandLine {
    /// Program name and arguments (argv[0], argv[1], ...)
    pub args: Vec<String>,
    /// Environment variables (envp[0], envp[1], ...) formatted as "KEY=VALUE"
    pub env: Vec<String>,
}

/// Backwards-compatible alias for code that previously used `BootCommandLine`.
pub type BootCommandLine = CommandLine;

impl CommandLine {
    /// Create a new `CommandLine` with explicit arguments and environment variables.
    pub fn new(args: Vec<String>, env: Vec<String>) -> Self {
        Self { args, env }
    }

    // ── Boot command-line support (consolidated from bootcmd.rs) ─────────

    /// Read and parse the kernel boot command line from the Limine bootloader.
    ///
    /// Environment-style tokens (`KEY=VALUE`) are placed into `env`;
    /// all other tokens become `args`. Quoted strings (`"..."` and `'...'`)
    /// are handled correctly.
    pub fn from_boot() -> Self {
        let raw_str = crate::limine::KERNEL_FILE_REQUEST
            .get_response()
            .and_then(|resp| resp.file().string().to_str().ok());

        match raw_str {
            Some(raw) => {
                log::info!("[CommandLine] Boot command line: \"{}\"", raw);
                Self::parse_boot(raw)
            }
            None => {
                log::warn!("[CommandLine] No boot command line provided by Limine bootloader");
                Self::default()
            }
        }
    }

    /// Parse a raw boot command-line string with quote-aware tokenization.
    ///
    /// Tokens containing `=` are treated as environment variables;
    /// everything else becomes an argument.
    pub fn parse_boot(raw: &str) -> Self {
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

        Self { args, env: envs }
    }

    /// Safely construct a `CommandLine` from raw C pointers (`argc`, `argv`, `envp`).
    ///
    /// # Safety
    /// `argv` must point to an array of `argc` valid null-terminated C string pointers.
    /// `envp` must point to a null-terminated array of null-terminated C string pointers (or null).
    pub unsafe fn from_raw(
        argc: usize,
        argv: *const *const u8,
        envp: *const *const u8,
    ) -> Result<Self, &'static str> {
        let mut args = Vec::new();
        let argv_user = UserPtr::<UserPtr<u8>>::from_raw(argv as *const UserPtr<u8>);
        if !argv_user.is_null() && argv_user.is_valid() {
            let mut i = 0;
            loop {
                if argc > 0 && i >= argc {
                    break;
                }
                let ptr_slot = argv_user.offset(i);
                let arg_ptr = match ptr_slot.read() {
                    Some(p) => p,
                    None => break,
                };
                if arg_ptr.is_null() {
                    break;
                }
                let c_str = UserCStr::new(arg_ptr.addr());
                match c_str.as_string(4096) {
                    Some(s) => args.push(s),
                    None => return Err("Invalid UTF-8 in argv"),
                }
                i += 1;
            }
        }

        let mut env = Vec::new();
        let envp_user = UserPtr::<UserPtr<u8>>::from_raw(envp as *const UserPtr<u8>);
        if !envp_user.is_null() && envp_user.is_valid() {
            let mut i = 0;
            loop {
                let ptr_slot = envp_user.offset(i);
                let env_ptr = match ptr_slot.read() {
                    Some(p) => p,
                    None => break,
                };
                if env_ptr.is_null() {
                    break;
                }
                let c_str = UserCStr::new(env_ptr.addr());
                if let Some(s) = c_str.as_string(4096) {
                    env.push(s);
                }
                i += 1;
            }
        }

        Ok(Self { args, env })
    }

    // ── Accessors ───────────────────────────────────────────────────────

    /// Returns `argc` (number of arguments).
    pub fn argc(&self) -> usize {
        self.args.len()
    }

    /// Returns cloned vector of argument strings.
    pub fn argv(&self) -> Vec<String> {
        self.args.clone()
    }

    /// Returns cloned vector of environment strings.
    pub fn envp(&self) -> Vec<String> {
        self.env.clone()
    }

    /// Returns the executable/program name (`argv[0]`), if present.
    pub fn program_name(&self) -> Option<&str> {
        self.args.first().map(|s| s.as_str())
    }

    /// Finds an environment variable value by key (e.g., `"PATH"` -> `"/bin:/usr/bin"`).
    pub fn get_env(&self, key: &str) -> Option<&str> {
        for item in &self.env {
            if let Some((k, v)) = item.split_once('=') {
                if k == key {
                    return Some(v);
                }
            }
        }
        None
    }

    /// Checks if a specific argument flag exists in the arguments list.
    pub fn has_arg(&self, flag: &str) -> bool {
        self.args.iter().any(|arg| arg == flag)
    }
}
