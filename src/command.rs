use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::DtrError;

pub(crate) struct CommandPlan {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
    pub(crate) current_dir: Option<PathBuf>,
    pub(crate) target_dir: Option<PathBuf>,
    pub(crate) preparations: Vec<PathBuf>,
    pub(crate) repospec: String,
    pub(crate) backend: &'static str,
    pub(crate) auth: Option<String>,
    pub(crate) environment: Vec<SecretEnvironment>,
    pub(crate) removed_environment: Vec<OsString>,
}

pub(crate) struct SecretEnvironment {
    name: OsString,
    value: OsString,
}

impl SecretEnvironment {
    pub(crate) fn new(name: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

impl CommandPlan {
    pub(crate) fn explain(&self) {
        println!("repospec: {}", self.repospec);
        println!("backend:  {}", self.backend);
        if let Some(auth) = &self.auth {
            println!("auth:     {auth}");
        }
        if let Some(directory) = &self.current_dir {
            println!("directory: {}", shell_quote(directory.as_os_str()));
        }
        if let Some(target) = &self.target_dir {
            println!("target:   {}", shell_quote(target.as_os_str()));
        }
        for directory in &self.preparations {
            println!("prepare:  mkdir -p {}", shell_quote(directory.as_os_str()));
        }
        println!("command:  {}", self.render_command());
    }

    pub(crate) fn execute(&self) -> Result<i32, DtrError> {
        for directory in &self.preparations {
            fs::create_dir_all(directory).map_err(|error| {
                DtrError::new(format!(
                    "could not create directory {}: {error}",
                    directory.display()
                ))
            })?;
        }

        let mut command = Command::new(&self.program);
        command.args(&self.args);
        apply_environment(&mut command, &self.environment, &self.removed_environment);
        if let Some(directory) = &self.current_dir {
            command.current_dir(directory);
        }
        let status = command.status().map_err(|error| {
            DtrError::new(format!(
                "could not start {}: {error}",
                self.program.to_string_lossy()
            ))
        })?;
        Ok(status.code().unwrap_or(1))
    }

    fn render_command(&self) -> String {
        std::iter::once(self.program.as_os_str())
            .chain(self.args.iter().map(OsString::as_os_str))
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

pub(crate) fn apply_environment(
    command: &mut Command,
    environment: &[SecretEnvironment],
    removed_environment: &[OsString],
) {
    for variable in environment {
        command.env(&variable.name, &variable.value);
    }
    for name in removed_environment {
        command.env_remove(name);
    }
}

pub(crate) fn command_exists(program: &str) -> bool {
    let Some(path) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|directory| command_exists_in(&directory, program))
}

#[cfg(not(windows))]
fn command_exists_in(directory: &Path, program: &str) -> bool {
    is_executable(&directory.join(program))
}

#[cfg(windows)]
fn command_exists_in(directory: &Path, program: &str) -> bool {
    if is_executable(&directory.join(program)) {
        return true;
    }
    if Path::new(program).extension().is_some() {
        return false;
    }

    let mut executable = OsString::from(program);
    executable.push(".exe");
    is_executable(&directory.join(executable))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

pub(crate) fn shell_quote(value: &OsStr) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;

        quote_bytes(value.as_bytes())
    }
    #[cfg(not(unix))]
    {
        quote_bytes(value.to_string_lossy().as_bytes())
    }
}

fn quote_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "''".to_owned();
    }
    if bytes.iter().all(|byte| {
        byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'_' | b'@' | b'%' | b'+' | b'=' | b':' | b',' | b'.' | b'/' | b'-'
            )
    }) {
        return String::from_utf8(bytes.to_vec()).expect("safe bytes are ASCII");
    }

    if let Ok(text) = std::str::from_utf8(bytes)
        && !text.chars().any(char::is_control)
    {
        return format!("'{}'", text.replace('\'', "'\\''"));
    }

    let mut quoted = String::from("$'");
    for byte in bytes {
        match byte {
            b'\\' => quoted.push_str("\\\\"),
            b'\'' => quoted.push_str("\\'"),
            b'\n' => quoted.push_str("\\n"),
            b'\r' => quoted.push_str("\\r"),
            b'\t' => quoted.push_str("\\t"),
            0x20..=0x7e => quoted.push(char::from(*byte)),
            _ => quoted.push_str(&format!("\\x{byte:02x}")),
        }
    }
    quoted.push('\'');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn command_lookup_recognizes_windows_exe_suffix() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("cargo.exe"), []).unwrap();

        assert!(command_exists_in(directory.path(), "cargo"));
        assert!(command_exists_in(directory.path(), "cargo.exe"));
        assert!(!command_exists_in(directory.path(), "missing"));
    }

    #[test]
    fn shell_quotes_simple_empty_and_apostrophe_values() {
        assert_eq!(shell_quote(OsStr::new("owner/repo")), "owner/repo");
        assert_eq!(shell_quote(OsStr::new("")), "''");
        assert_eq!(shell_quote(OsStr::new("two words")), "'two words'");
        assert_eq!(shell_quote(OsStr::new("it's")), "'it'\\''s'");
    }

    #[cfg(unix)]
    #[test]
    fn shell_quotes_non_utf8_unambiguously() {
        use std::os::unix::ffi::OsStrExt;

        assert_eq!(shell_quote(OsStr::from_bytes(b"a\xff")), "$'a\\xff'");
    }
}
