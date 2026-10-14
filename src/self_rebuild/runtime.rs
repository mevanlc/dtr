use std::env;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;

use serde::Deserialize;
use sha2::{Digest, Sha256};

#[cfg(unix)]
use super::DTR_SELF_LOCKS;
use super::DTR_SELF_REEXEC;
use crate::command::InterruptState;
use crate::error::DtrError;
use crate::output::{CapturedOutput, Verbosity};

const EXAMPLE: &str = "dtr-trampoline";

pub(crate) struct Locks {
    files: Vec<File>,
}

fn lock(kind: &str, path: &Path) -> Result<File, DtrError> {
    let directory = env::temp_dir().join("dtr-self-rebuild-locks");
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(&directory)?;
    // Canonicalization resolves aliases before this function is called. Windows
    // also permits different casing for the same path.
    #[cfg(windows)]
    let key = path.as_os_str().to_string_lossy().to_lowercase();
    #[cfg(windows)]
    let bytes = key.as_bytes();
    #[cfg(not(windows))]
    let bytes = path.as_os_str().as_encoded_bytes();
    let digest = Sha256::digest(bytes);
    let lock_path = directory.join(format!("{kind}-{digest:x}.lock"));
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(&lock_path)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(DtrError::new(format!(
            "self-install already in progress for {}",
            path.display()
        ))),
        Err(TryLockError::Error(error)) => Err(DtrError::new(format!(
            "could not lock {}: {error}",
            lock_path.display()
        ))),
    }
    // Never unlink the lock file: a concurrent opener must see the same inode.
}

#[cfg(unix)]
#[derive(serde::Serialize, Deserialize)]
struct InheritedLock {
    fd: i32,
    device: u64,
    inode: u64,
}

#[cfg(unix)]
#[derive(serde::Serialize, Deserialize)]
struct Handoff {
    pid: u32,
    locks: Vec<InheritedLock>,
}

#[cfg(unix)]
fn set_inheritable(fd: i32, inheritable: bool) -> io::Result<()> {
    // SAFETY: fcntl validates the descriptor; these commands use an integer arg.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    let flags = if inheritable {
        flags & !libc::FD_CLOEXEC
    } else {
        flags | libc::FD_CLOEXEC
    };
    if unsafe { libc::fcntl(fd, libc::F_SETFD, flags) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Called only at executable startup, before any threads or signal handlers.
pub(crate) fn adopt_locks() -> Result<Locks, DtrError> {
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        let Some(value) = env::var_os(DTR_SELF_LOCKS) else {
            return Ok(Locks { files: Vec::new() });
        };
        let invalid = || DtrError::new("invalid inherited self-install locks");
        if !super::is_self_reexec() {
            return Err(invalid());
        }
        let handoff: Handoff =
            serde_json::from_str(value.to_str().ok_or_else(invalid)?).map_err(|_| invalid())?;
        // exec preserves PID. Descendants must not adopt descriptors which
        // CLOEXEC closed, even if a discovery command inherited the environment.
        if handoff.pid != std::process::id() {
            return Ok(Locks { files: Vec::new() });
        }
        let records = handoff.locks;
        if records.len() != 2 || records[0].fd == records[1].fd {
            return Err(invalid());
        }
        // Validate everything before taking ownership of any descriptor.
        for record in &records {
            let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
            if record.fd < 3
                // SAFETY: fstat validates fd and initializes stat on success.
                || unsafe { libc::fstat(record.fd, stat.as_mut_ptr()) } != 0
            {
                return Err(invalid());
            }
            let stat = unsafe { stat.assume_init() };
            // libc's dev_t/ino_t widths and signedness vary across Unix targets.
            #[allow(clippy::unnecessary_cast)]
            let identity = (stat.st_dev as u64, stat.st_ino as u64);
            if stat.st_mode & libc::S_IFMT != libc::S_IFREG
                || identity != (record.device, record.inode)
            {
                return Err(invalid());
            }
        }
        let mut locks = Locks { files: Vec::new() };
        for record in records {
            // SAFETY: validated above; each inherited fd is unique and has no
            // Rust owner in the newly exec'd process.
            let file = unsafe { File::from_raw_fd(record.fd) };
            set_inheritable(record.fd, false)?;
            locks.files.push(file);
        }
        Ok(locks)
    }
    #[cfg(not(unix))]
    Ok(Locks { files: Vec::new() })
}

#[derive(Deserialize)]
struct Metadata {
    target_directory: PathBuf,
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    manifest_path: PathBuf,
}

enum CargoOutcome<T> {
    Completed(T),
    Interrupted,
}

fn was_interrupted(status: ExitStatus) -> bool {
    if status.code() == Some(130) {
        return true;
    }
    // STATUS_CONTROL_C_EXIT: Windows console termination by Ctrl-C/Ctrl-Break.
    #[cfg(windows)]
    if status.code() == Some(0xC000_013Au32 as i32) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal() == Some(libc::SIGINT)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

fn metadata(package_root: &Path, verbosity: Verbosity) -> Result<CargoOutcome<Metadata>, DtrError> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version=1"])
        .arg("--manifest-path")
        .arg(package_root.join("Cargo.toml"))
        .current_dir(package_root)
        .stderr(if verbosity.progress() {
            Stdio::inherit()
        } else {
            Stdio::piped()
        })
        .output()
        .map_err(|error| DtrError::new(format!("could not start cargo metadata: {error}")))?;
    if !output.status.success() {
        if !verbosity.progress() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }
        if was_interrupted(output.status) {
            return Ok(CargoOutcome::Interrupted);
        }
        return Err(DtrError::new(format!(
            "cargo metadata for dtr exited with status {}",
            output.status.code().unwrap_or(1)
        )));
    }
    serde_json::from_slice(&output.stdout)
        .map(CargoOutcome::Completed)
        .map_err(|error| DtrError::new(format!("invalid cargo metadata for dtr: {error}")))
}

fn artifact_path(message: &serde_json::Value, package_id: &str) -> Option<PathBuf> {
    if message["reason"] != "compiler-artifact"
        || message["package_id"] != package_id
        || message["target"]["name"] != EXAMPLE
        || message["profile"]["test"] != false
        || !message["target"]["kind"]
            .as_array()?
            .iter()
            .any(|kind| kind == "example")
        || !message["target"]["crate_types"]
            .as_array()?
            .iter()
            .any(|kind| kind == "bin")
    {
        return None;
    }
    message["executable"].as_str().map(PathBuf::from)
}

fn build_example(
    package_root: &Path,
    package_id: &str,
    verbosity: Verbosity,
) -> Result<CargoOutcome<PathBuf>, DtrError> {
    let mut captured = if verbosity.progress() {
        None
    } else {
        Some(CapturedOutput::new()?)
    };
    verbosity.debug(format_args!(
        "building the local dtr trampoline for self-install"
    ));
    let mut child = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--example",
            EXAMPLE,
            "--target",
            "host-tuple",
            "--message-format=json,json-render-diagnostics",
        ])
        .arg("--manifest-path")
        .arg(package_root.join("Cargo.toml"))
        .arg("--package")
        .arg(package_id)
        .current_dir(package_root)
        .stdout(Stdio::piped())
        .stderr(match &captured {
            Some(output) => output.stdio()?,
            None => Stdio::inherit(),
        })
        .spawn()
        .map_err(|error| DtrError::new(format!("could not start cargo build for dtr: {error}")))?;
    let mut executable = None;
    let mut read_error = None;
    for line in BufReader::new(child.stdout.take().expect("piped cargo stdout")).lines() {
        let line = match line {
            Ok(line) => line,
            Err(error) => {
                read_error = Some(error);
                break;
            }
        };
        match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(message) => {
                if let Some(path) = artifact_path(&message, package_id) {
                    executable = Some(path);
                }
            }
            // Cargo cannot control stdout from every compiler/build tool.
            Err(_) => match &mut captured {
                Some(output) => {
                    if let Err(error) = output
                        .append(line.as_bytes())
                        .and_then(|()| output.append(b"\n"))
                    {
                        read_error = Some(error);
                        break;
                    }
                }
                None => eprintln!("{line}"),
            },
        }
    }
    let status = child.wait()?;
    if (!status.success() || read_error.is_some())
        && let Some(output) = &mut captured
    {
        output.replay()?;
    }
    // The signal callback runs on another thread and may not have updated its
    // flag yet. Cargo's own termination status is sufficient to cancel.
    if was_interrupted(status) {
        return Ok(CargoOutcome::Interrupted);
    }
    if let Some(error) = read_error {
        return Err(DtrError::new(format!(
            "could not process cargo build output: {error}"
        )));
    }
    if !status.success() {
        return Err(DtrError::new(format!(
            "cargo build for dtr trampoline exited with status {}",
            status.code().unwrap_or(1)
        )));
    }
    executable
        .filter(|path| path.is_absolute() && path.is_file())
        .map(CargoOutcome::Completed)
        .ok_or_else(|| DtrError::new("cargo did not report an existing dtr-trampoline executable"))
}

pub(crate) fn execute(
    package_root: &Path,
    interrupted: Option<Arc<InterruptState>>,
    verbosity: Verbosity,
) -> Result<i32, DtrError> {
    let interrupted = match interrupted {
        Some(state) => state,
        None => InterruptState::install_handler()?,
    };
    interrupted.supervise();
    execute_inner(package_root, &interrupted, verbosity)
}

fn execute_inner(
    package_root: &Path,
    interrupted: &InterruptState,
    verbosity: Verbosity,
) -> Result<i32, DtrError> {
    let current_exe = env::current_exe()?.canonicalize()?;
    let mut locks = Locks {
        files: vec![lock("executable", &current_exe)?],
    };
    if interrupted.is_interrupted() {
        return Ok(130);
    }
    let package_root = package_root.canonicalize()?;
    let manifest = package_root.join("Cargo.toml").canonicalize()?;
    let metadata = metadata(&package_root, verbosity);
    if interrupted.is_interrupted() {
        return Ok(130);
    }
    let CargoOutcome::Completed(metadata) = metadata? else {
        interrupted.request_interrupt();
        return Ok(130);
    };
    let package = metadata
        .packages
        .iter()
        .find(|package| package.manifest_path.canonicalize().ok().as_ref() == Some(&manifest))
        .ok_or_else(|| DtrError::new("cargo metadata did not identify the dtr package"))?;
    fs::create_dir_all(&metadata.target_directory)?;
    locks
        .files
        .push(lock("target", &metadata.target_directory.canonicalize()?)?);
    if interrupted.is_interrupted() {
        return Ok(130);
    }
    let executable = build_example(&package_root, &package.id, verbosity);
    if interrupted.is_interrupted() {
        return Ok(130);
    }
    let CargoOutcome::Completed(executable) = executable? else {
        interrupted.request_interrupt();
        return Ok(130);
    };
    let mut command = Command::new(&executable);
    command
        .args(env::args_os().skip(1))
        .env(DTR_SELF_REEXEC, "1");

    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        use std::os::unix::fs::MetadataExt;
        use std::os::unix::process::CommandExt;
        let mut records = Vec::new();
        for file in &locks.files {
            let metadata = file.metadata()?;
            records.push(InheritedLock {
                fd: file.as_raw_fd(),
                device: metadata.dev(),
                inode: metadata.ino(),
            });
        }
        command.env(
            DTR_SELF_LOCKS,
            serde_json::to_string(&Handoff {
                pid: std::process::id(),
                locks: records,
            })
            .expect("lock records"),
        );
        // No further child processes are started between clearing CLOEXEC and
        // exec. On failure the local owners close the descriptors on return.
        for file in &locks.files {
            set_inheritable(file.as_raw_fd(), true)?;
        }
        Err(DtrError::new(format!(
            "could not re-exec dtr trampoline {}: {}",
            executable.display(),
            command.exec()
        )))
    }
    #[cfg(not(unix))]
    {
        let backup = current_exe.with_extension("exe.old");
        remove_backup(&backup)?;
        fs::rename(&current_exe, &backup).map_err(|error| {
            DtrError::new(format!(
                "could not rename {} to {}: {error}",
                current_exe.display(),
                backup.display()
            ))
        })?;
        let result = command.status().map_err(|error| {
            DtrError::new(format!(
                "could not execute dtr trampoline {}: {error}",
                executable.display()
            ))
        });
        // A failed kit can still have installed dtr successfully. Conversely a
        // successful install into a different --root leaves this path missing.
        restore_if_missing(&current_exe, &backup)?;
        let status = result?;
        if interrupted.is_interrupted() {
            Ok(130)
        } else {
            Ok(status.code().unwrap_or(1))
        }
    }
}

#[cfg(any(not(unix), test))]
fn remove_backup(backup: &Path) -> Result<(), DtrError> {
    match fs::remove_file(backup) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DtrError::new(format!(
            "could not remove old executable {}: {error}",
            backup.display()
        ))),
    }
}

#[cfg(any(not(unix), test))]
fn restore_if_missing(original: &Path, backup: &Path) -> Result<(), DtrError> {
    if !original.try_exists()? {
        fs::rename(backup, original).map_err(|error| {
            DtrError::new(format!(
                "could not restore {}; recover the original executable from {}: {error}",
                original.display(),
                backup.display()
            ))
        })?;
    }
    Ok(())
}

pub(crate) fn sweep_stale_old_binary() {
    #[cfg(not(unix))]
    if !super::is_self_reexec()
        && super::current_exe_is_dtr()
        && let Ok(executable) = env::current_exe().and_then(fs::canonicalize)
        && let Ok(_lock) = lock("executable", &executable)
        && executable.is_file()
    {
        let _ = remove_backup(&executable.with_extension("exe.old"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selects_only_the_requested_executable_example_including_cached_builds() {
        let mut message = json!({
            "reason": "compiler-artifact", "package_id": "dtr-id",
            "target": {"name": EXAMPLE, "kind": ["example"], "crate_types": ["bin"]},
            "profile": {"test": false}, "executable": "/custom target/dtr-trampoline",
            "fresh": true,
        });
        assert_eq!(
            artifact_path(&message, "dtr-id"),
            Some(PathBuf::from("/custom target/dtr-trampoline"))
        );
        assert_eq!(artifact_path(&message, "another-package"), None);
        message["fresh"] = json!(false);
        assert!(artifact_path(&message, "dtr-id").is_some());
        message["target"]["kind"] = json!(["bin"]);
        assert_eq!(artifact_path(&message, "dtr-id"), None);
        message["target"]["kind"] = json!(["example"]);
        message["profile"]["test"] = json!(true);
        assert_eq!(artifact_path(&message, "dtr-id"), None);
        message["profile"]["test"] = json!(false);
        message["executable"] = serde_json::Value::Null;
        assert_eq!(artifact_path(&message, "dtr-id"), None);
    }

    #[test]
    fn lock_contention_fails_and_dropping_the_owner_releases_it() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().canonicalize().unwrap();
        let owner = lock("test", &path).unwrap();
        assert!(
            lock("test", &path)
                .unwrap_err()
                .to_string()
                .contains("self-install already in progress")
        );
        drop(owner);
        drop(lock("test", &path).unwrap());
    }

    #[test]
    fn rollback_restores_a_missing_original_and_preserves_a_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let original = directory.path().join("dtr.exe");
        let backup = original.with_extension("exe.old");
        fs::write(&backup, b"old").unwrap();
        restore_if_missing(&original, &backup).unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"old");
        fs::write(&backup, b"backup").unwrap();
        fs::write(&original, b"new").unwrap();
        restore_if_missing(&original, &backup).unwrap();
        assert_eq!(fs::read(&original).unwrap(), b"new");
        assert_eq!(fs::read(&backup).unwrap(), b"backup");
        remove_backup(&backup).unwrap();
        assert!(!backup.exists());
    }

    #[test]
    fn failed_rollback_reports_the_backup_location() {
        let directory = tempfile::tempdir().unwrap();
        let backup = directory.path().join("dtr.exe.old");
        fs::write(&backup, b"old").unwrap();
        let original = directory.path().join("missing-parent/dtr.exe");
        let error = restore_if_missing(&original, &backup).unwrap_err();
        assert!(error.to_string().contains(&backup.display().to_string()));
        assert_eq!(fs::read(&backup).unwrap(), b"old");
    }
}
