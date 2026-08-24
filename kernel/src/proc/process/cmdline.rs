//! Process Command-Line Arguments & Environment Management.

use alloc::string::String;
use alloc::vec::Vec;
use crate::mm::{UserCStr, UserPtr};

/// Represents parsed command line arguments and environment variables for a process.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandLine {
    /// Program name and arguments (argv[0], argv[1], ...)
    pub args: Vec<String>,
    /// Environment variables (envp[0], envp[1], ...) formatted as "KEY=VALUE"
    pub env: Vec<String>,
}

impl CommandLine {
    /// Create a new `CommandLine` with explicit arguments and environment variables.
    pub fn new(args: Vec<String>, env: Vec<String>) -> Self {
        Self { args, env }
    }

    /// Parse a single command string (e.g., `"ls -l /bin"`) into arguments.
    pub fn from_cmd_str(cmd: &str) -> Self {
        let args: Vec<String> = cmd
            .split_whitespace()
            .map(String::from)
            .collect();
        Self {
            args,
            env: Vec::new(),
        }
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

    /// Returns `argc` (number of arguments).
    pub fn argc(&self) -> usize {
        self.args.len()
    }

    /// Returns slice of argument strings.
    pub fn argv(&self) -> &[String] {
        &self.args
    }

    /// Returns slice of environment strings.
    pub fn envp(&self) -> &[String] {
        &self.env
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
}
