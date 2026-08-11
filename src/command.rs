#[cfg(unix)]
use std::collections::HashSet;
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output};
#[cfg(unix)]
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::DtrError;

pub(crate) struct CommandPlan {
    pub(crate) kind: PlanKind,
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

pub(crate) enum PlanKind {
    Clone,
    GoInstall(GoInstallSource),
    OtherInstall,
}

pub(crate) enum GoInstallSource {
    Local,
    Remote { import_path: OsString },
}

pub(crate) struct SecretEnvironment {
    name: OsString,
    value: OsString,
}

pub(crate) struct InterruptState {
    interrupt_count: AtomicUsize,
    #[cfg(unix)]
    active_process_groups: Mutex<HashSet<u32>>,
}

impl InterruptState {
    pub(crate) fn new() -> Self {
        Self {
            interrupt_count: AtomicUsize::new(0),
            #[cfg(unix)]
            active_process_groups: Mutex::new(HashSet::new()),
        }
    }

    pub(crate) fn request_interrupt(&self) -> usize {
        self.interrupt_count.fetch_add(1, Ordering::SeqCst)
    }

    pub(crate) fn is_interrupted(&self) -> bool {
        self.interrupt_count.load(Ordering::Acquire) != 0
    }

    pub(crate) fn stop_active_children(&self) {
        #[cfg(unix)]
        for process_group in self
            .active_process_groups
            .lock()
            .expect("active child process groups should not be poisoned")
            .iter()
            .copied()
        {
            stop_process_group(process_group);
        }
    }

    fn configure_command(&self, _command: &mut Command) {
        #[cfg(unix)]
        _command.process_group(0);
    }

    fn register_child(&self, _child: &Child) {
        #[cfg(unix)]
        self.active_process_groups
            .lock()
            .expect("active child process groups should not be poisoned")
            .insert(_child.id());
    }

    fn unregister_child(&self, _child: &Child) {
        #[cfg(unix)]
        self.active_process_groups
            .lock()
            .expect("active child process groups should not be poisoned")
            .remove(&_child.id());
    }

    fn stop_child(&self, child: &mut Child) {
        #[cfg(unix)]
        stop_process_group(child.id());
        #[cfg(not(unix))]
        let _ = child.kill();
    }
}

#[cfg(unix)]
fn stop_process_group(process_group: u32) {
    if let Ok(process_group) = i32::try_from(process_group) {
        unsafe {
            libc::kill(-process_group, libc::SIGKILL);
        }
    }
}

pub(crate) enum DtrMessage {
    Stdout(String),
    Stderr(String),
}

impl DtrMessage {
    pub(crate) fn emit(&self) {
        match self {
            Self::Stdout(message) => println!("{message}"),
            Self::Stderr(message) => eprintln!("{message}"),
        }
    }
}

struct MessageSink<'a> {
    narration: bool,
    replay: Option<&'a mut Vec<DtrMessage>>,
}

impl MessageSink<'_> {
    fn narration_enabled(&self) -> bool {
        self.narration
    }

    fn emit_and_replay(&mut self, message: DtrMessage) {
        message.emit();
        if let Some(replay) = &mut self.replay {
            replay.push(message);
        }
    }

    fn narrate_stdout(&mut self, message: String) {
        if self.narration {
            self.emit_and_replay(DtrMessage::Stdout(message));
        }
    }

    fn narrate_stderr(&mut self, message: String) {
        if self.narration {
            self.emit_and_replay(DtrMessage::Stderr(message));
        }
    }

    fn warn_stderr(&mut self, message: String) {
        self.emit_and_replay(DtrMessage::Stderr(message));
    }
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

    pub(crate) fn execute(&self, narration: bool) -> Result<i32, DtrError> {
        let mut messages = MessageSink {
            narration,
            replay: None,
        };
        self.execute_with_messages(&mut messages, None)
    }

    pub(crate) fn execute_with_replay(
        &self,
        narration: bool,
        replay: &mut Vec<DtrMessage>,
        interrupted: &InterruptState,
    ) -> Result<i32, DtrError> {
        let mut messages = MessageSink {
            narration,
            replay: Some(replay),
        };
        self.execute_with_messages(&mut messages, Some(interrupted))
    }

    fn execute_with_messages(
        &self,
        messages: &mut MessageSink<'_>,
        interrupted: Option<&InterruptState>,
    ) -> Result<i32, DtrError> {
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
        if let Some(interrupted) = interrupted {
            interrupted.configure_command(&mut command);
        }
        if messages.narration_enabled() {
            if let Some(directory) = &self.current_dir {
                messages.narrate_stderr(format!(
                    "→ {} (in {})",
                    self.render_command(),
                    shell_quote(absolute_path(directory).as_os_str())
                ));
            } else {
                messages.narrate_stderr(format!("→ {}", self.render_command()));
            }
        }
        let mut child = command.spawn().map_err(|error| {
            DtrError::new(format!(
                "could not start {}: {error}",
                self.program.to_string_lossy()
            ))
        })?;
        if let Some(interrupted) = interrupted {
            interrupted.register_child(&child);
            if interrupted.is_interrupted() {
                interrupted.stop_child(&mut child);
            }
        }
        let status = child.wait();
        if let Some(interrupted) = interrupted {
            interrupted.unregister_child(&child);
        }
        let status = status.map_err(|error| {
            DtrError::new(format!(
                "could not wait for {}: {error}",
                self.program.to_string_lossy()
            ))
        })?;
        let code = status.code().unwrap_or(1);
        if code == 0 {
            self.report_success(messages);
        }
        Ok(code)
    }

    pub(crate) fn render_command(&self) -> String {
        std::iter::once(self.program.as_os_str())
            .chain(self.args.iter().map(OsString::as_os_str))
            .map(shell_quote)
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn report_success(&self, messages: &mut MessageSink<'_>) {
        match &self.kind {
            PlanKind::Clone => {
                if let Some(target) = &self.target_dir {
                    messages.narrate_stdout(absolute_path(target).display().to_string());
                }
            }
            PlanKind::GoInstall(source) => match self.go_installed_binaries(source) {
                Ok(binaries) => {
                    if messages.narration_enabled() {
                        narrate_installed_binaries(&binaries, messages);
                    }
                    warn_about_path(&binaries, messages);
                }
                Err(error) if messages.narration_enabled() => {
                    messages.narrate_stderr(format!("go install succeeded; {error}"));
                }
                Err(_) => {}
            },
            PlanKind::OtherInstall => {}
        }
    }

    fn go_installed_binaries(&self, source: &GoInstallSource) -> Result<Vec<PathBuf>, String> {
        match source {
            GoInstallSource::Local => {
                let output = self.run_go_query([
                    "list",
                    "-f",
                    "{{if eq .Name \"main\"}}{{.Target}}{{end}}",
                    "./...",
                ])?;
                let stdout =
                    successful_utf8_output(output, "go list could not report binary locations")?;
                Ok(stdout
                    .lines()
                    .filter(|line| !line.is_empty())
                    .map(PathBuf::from)
                    .collect())
            }
            GoInstallSource::Remote { import_path } => {
                let output = self.run_go_query(["env", "GOBIN", "GOPATH"])?;
                let stdout =
                    successful_utf8_output(output, "go env could not report the binary directory")?;
                let mut lines = stdout.lines();
                let gobin = lines.next().unwrap_or_default();
                let gopath = lines.next().unwrap_or_default();
                let directory = if gobin.is_empty() {
                    env::split_paths(OsStr::new(gopath))
                        .next()
                        .map(|path| path.join("bin"))
                } else {
                    Some(PathBuf::from(gobin))
                }
                .ok_or_else(|| "go env did not report GOBIN or GOPATH".to_owned())?;
                let name = remote_go_binary_name(import_path).ok_or_else(|| {
                    "the installed binary name could not be determined".to_owned()
                })?;
                Ok(vec![directory.join(name)])
            }
        }
    }

    fn run_go_query<const N: usize>(&self, args: [&str; N]) -> Result<Output, String> {
        let mut command = Command::new(&self.program);
        command.args(args);
        apply_environment(&mut command, &self.environment, &self.removed_environment);
        if let Some(directory) = &self.current_dir {
            command.current_dir(directory);
        }
        command
            .output()
            .map_err(|error| format!("binary locations could not be determined: {error}"))
    }
}

fn successful_utf8_output(output: Output, failure: &str) -> Result<String, String> {
    if !output.status.success() {
        return Err(failure.to_owned());
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{failure}: output was not valid UTF-8"))
}

fn remote_go_binary_name(import_path: &OsStr) -> Option<&str> {
    let import_path = import_path.to_str()?.split('@').next()?;
    let mut components = import_path.rsplit('/');
    let last = components.next()?;
    if last.strip_prefix('v').is_some_and(|version| {
        !version.is_empty() && version.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        components.next()
    } else {
        Some(last)
    }
}

fn narrate_installed_binaries(binaries: &[PathBuf], messages: &mut MessageSink<'_>) {
    if binaries.is_empty() {
        messages.narrate_stderr("go install succeeded; no binaries were reported".to_owned());
        return;
    }

    if let Some(directory) = binaries[0].parent()
        && binaries
            .iter()
            .all(|binary| binary.parent() == Some(directory))
    {
        let names = binaries
            .iter()
            .filter_map(|binary| binary.file_name())
            .map(|name| name.to_string_lossy())
            .collect::<Vec<_>>()
            .join(", ");
        messages.narrate_stderr(format!("installed: {names} → {}", directory.display()));
    } else {
        for binary in binaries {
            messages.narrate_stderr(format!("installed: {}", binary.display()));
        }
    }
}

fn warn_about_path(binaries: &[PathBuf], messages: &mut MessageSink<'_>) {
    let Some(path) = env::var_os("PATH") else {
        for directory in unique_binary_directories(binaries) {
            messages.warn_stderr(format!("warning: {} is not on PATH", directory.display()));
        }
        return;
    };
    let path_directories = env::split_paths(&path).collect::<Vec<_>>();

    for directory in unique_binary_directories(binaries) {
        let Some(position) = path_directories
            .iter()
            .position(|candidate| paths_equivalent(candidate, &directory))
        else {
            messages.warn_stderr(format!("warning: {} is not on PATH", directory.display()));
            continue;
        };

        for binary in binaries
            .iter()
            .filter(|binary| binary.parent() == Some(&directory))
        {
            let Some(name) = binary.file_name() else {
                continue;
            };
            let shadow = path_directories[..position]
                .iter()
                .map(|directory| directory.join(name))
                .find(|candidate| is_executable(candidate) && !paths_equivalent(candidate, binary));
            if let Some(shadow) = shadow {
                messages.warn_stderr(format!(
                    "warning: '{}' is shadowed by {} (earlier on PATH)",
                    name.to_string_lossy(),
                    shadow.display()
                ));
            }
        }
    }
}

fn unique_binary_directories(binaries: &[PathBuf]) -> Vec<PathBuf> {
    let mut directories: Vec<PathBuf> = Vec::new();
    for directory in binaries.iter().filter_map(|binary| binary.parent()) {
        if !directories
            .iter()
            .any(|known| paths_equivalent(known, directory))
        {
            directories.push(directory.to_path_buf());
        }
    }
    directories
}

fn paths_equivalent(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => absolute_path(left) == absolute_path(right),
    }
}

fn absolute_path(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()
            .map(|directory| directory.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
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

    #[test]
    fn remote_go_binary_names_skip_major_version_suffixes() {
        assert_eq!(
            remote_go_binary_name(OsStr::new("example.com/owner/tool@latest")),
            Some("tool")
        );
        assert_eq!(
            remote_go_binary_name(OsStr::new("example.com/owner/tool/v2@v2.3.4")),
            Some("tool")
        );
        assert_eq!(
            remote_go_binary_name(OsStr::new("example.com/owner/vault")),
            Some("vault")
        );
    }

    #[cfg(unix)]
    #[test]
    fn shell_quotes_non_utf8_unambiguously() {
        use std::os::unix::ffi::OsStrExt;

        assert_eq!(shell_quote(OsStr::from_bytes(b"a\xff")), "$'a\\xff'");
    }
}
