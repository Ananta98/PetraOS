//! Process Command-Line Arguments & Environment Management.

use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::CStr;

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
        if !argv.is_null() {
            let mut i = 0;
            loop {
                if argc > 0 && i >= argc {
                    break;
                }
                let arg_ptr = unsafe { *argv.add(i) };
                if arg_ptr.is_null() {
                    break;
                }
                let c_str = unsafe { CStr::from_ptr(arg_ptr as *const i8) };
                let str_slice = c_str.to_str().map_err(|_| "Invalid UTF-8 in argv")?;
                args.push(String::from(str_slice));
                i += 1;
            }
        }

        let mut env = Vec::new();
        if !envp.is_null() {
            let mut i = 0;
            loop {
                let env_ptr = unsafe { *envp.add(i) };
                if env_ptr.is_null() {
                    break;
                }
                let c_str = unsafe { CStr::from_ptr(env_ptr as *const i8) };
                if let Ok(str_slice) = c_str.to_str() {
                    env.push(String::from(str_slice));
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
