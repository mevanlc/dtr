use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
#[cfg(not(unix))]
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(not(unix))]
use std::sync::Arc;
#[cfg(not(unix))]
use std::sync::atomic::{AtomicBool, Ordering};

use crate::cli::InstallTool;
use crate::error::DtrError;
use crate::kit::InstallEntry;
use crate::repospec::RepoSpec;

pub(crate) const DTR_SELF_REEXEC: &str = "DTR_SELF_REEXEC";
#[cfg(not(unix))]
pub(crate) const DTR_PARENT_PID: &str = "DTR_PARENT_PID";
#[cfg(not(unix))]
pub(crate) const DTR_ORIGINAL_EXE: &str = "DTR_ORIGINAL_EXE";
#[cfg(not(unix))]
pub(crate) const DTR_RENAMED_EXE: &str = "DTR_RENAMED_EXE";

pub(crate) fn is_self_reexec() -> bool {
    env::var_os(DTR_SELF_REEXEC).is_some()
}

pub(crate) fn current_exe_is_dtr() -> bool {
    let Ok(current_exe) = env::current_exe() else {
        return false;
    };
    let Some(file_name) = current_exe.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    file_name.eq_ignore_ascii_case("dtr") || file_name.eq_ignore_ascii_case("dtr.exe")
}

pub(crate) fn is_self_install_entry(entry: &InstallEntry) -> bool {
    if is_self_reexec() || !current_exe_is_dtr() {
        return false;
    }
    if !matches!(entry.tool, InstallTool::Auto | InstallTool::Cargo) {
        return false;
    }
    is_dtr_target(&entry.repospec)
}

pub(crate) fn is_dtr_target(repospec_str: &str) -> bool {
    let Ok(spec) = RepoSpec::parse(OsStr::new(repospec_str)) else {
        return is_dtr_repospec_name(repospec_str);
    };
    if let Some(path) = spec.local_path() {
        is_dtr_manifest(&path.join("Cargo.toml"))
    } else {
        is_dtr_repospec_name(repospec_str)
    }
}

fn is_dtr_manifest(manifest_path: &Path) -> bool {
    let Ok(content) = fs::read_to_string(manifest_path) else {
        return false;
    };
    if let Ok(manifest) = toml::from_str::<toml::Value>(&content)
        && let Some(name) = manifest
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
    {
        return name == "dtr";
    }
    false
}

fn is_dtr_repospec_name(repospec: &str) -> bool {
    let normalized = repospec.trim().trim_end_matches(".git");
    normalized == "dtr"
        || normalized == "mevanlc/dtr"
        || normalized.ends_with("/mevanlc/dtr")
        || normalized.ends_with("/dtr")
}

pub(crate) fn resolve_local_package_root(repospec_str: &str) -> Option<PathBuf> {
    let spec = RepoSpec::parse(OsStr::new(repospec_str)).ok()?;
    let path = spec.local_path()?;
    if path.join("Cargo.toml").exists() {
        Some(path.to_path_buf())
    } else {
        None
    }
}

pub(crate) fn build_dtr_release(package_root: &Path) -> Result<PathBuf, DtrError> {
    let mut command = Command::new("cargo");
    command
        .arg("build")
        .arg("--release")
        .current_dir(package_root);

    let status = command
        .status()
        .map_err(|error| DtrError::new(format!("could not start cargo build for dtr: {error}")))?;

    if !status.success() {
        return Err(DtrError::new(format!(
            "cargo build for dtr exited with status {}",
            status.code().unwrap_or(1)
        )));
    }

    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| package_root.join("target"));

    let binary_name = if cfg!(windows) { "dtr.exe" } else { "dtr" };
    let binary_path = target_dir.join("release").join(binary_name);

    if !binary_path.exists() {
        return Err(DtrError::new(format!(
            "expected built binary at {} was not found",
            binary_path.display()
        )));
    }

    Ok(binary_path)
}

pub(crate) fn trampoline_execute(target_binary: &Path) -> Result<i32, DtrError> {
    let args: Vec<OsString> = env::args_os().collect();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new(target_binary);
        if args.len() > 1 {
            command.args(&args[1..]);
        }
        command.env(DTR_SELF_REEXEC, "1");
        let error = command.exec();
        Err(DtrError::new(format!(
            "could not re-exec into new dtr binary {}: {error}",
            target_binary.display()
        )))
    }

    #[cfg(not(unix))]
    {
        let current_exe = env::current_exe()
            .map_err(|error| DtrError::new(format!("could not resolve current_exe: {error}")))?;
        let old_exe = current_exe.with_extension("exe.old");

        if old_exe.exists() {
            let _ = fs::remove_file(&old_exe);
        }

        fs::rename(&current_exe, &old_exe).map_err(|error| {
            DtrError::new(format!(
                "could not rename current binary {} to {}: {error}",
                current_exe.display(),
                old_exe.display()
            ))
        })?;

        let temp_exe = create_trampoline_binary(target_binary)?;

        let mut command = Command::new(&temp_exe);
        if args.len() > 1 {
            command.args(&args[1..]);
        }
        command.env(DTR_SELF_REEXEC, "1");
        command.env(DTR_PARENT_PID, std::process::id().to_string());
        command.env(DTR_ORIGINAL_EXE, &current_exe);
        command.env(DTR_RENAMED_EXE, &old_exe);

        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupted_clone = Arc::clone(&interrupted);
        let _ = ctrlc::set_handler(move || {
            interrupted_clone.store(true, Ordering::SeqCst);
        });

        let mut child = command.spawn().map_err(|error| {
            let _ = fs::rename(&old_exe, &current_exe);
            let _ = fs::remove_file(&temp_exe);
            DtrError::new(format!(
                "could not start trampoline process {}: {error}",
                temp_exe.display()
            ))
        })?;

        let status = child.wait();

        // If parent wakes up and current_exe is missing, perform rescue rollback
        if !current_exe.exists() && old_exe.exists() {
            let _ = fs::rename(&old_exe, &current_exe);
        }

        let _ = fs::remove_file(&temp_exe);

        let code = status.map(|s| s.code().unwrap_or(1)).unwrap_or(1);

        if interrupted.load(Ordering::Acquire) {
            Ok(130)
        } else {
            Ok(code)
        }
    }
}

#[cfg(not(unix))]
fn create_trampoline_binary(target_binary: &Path) -> Result<PathBuf, DtrError> {
    let temp_dir = env::temp_dir();
    let unique_id = format!(
        "dtr-trampoline-{}-{}.exe",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let temp_exe = temp_dir.join(unique_id);
    fs::copy(target_binary, &temp_exe).map_err(|error| {
        DtrError::new(format!(
            "could not copy trampoline binary from {} to {}: {error}",
            target_binary.display(),
            temp_exe.display()
        ))
    })?;
    Ok(temp_exe)
}

#[cfg(not(unix))]
pub(crate) fn finish_child_cleanup() {
    let Ok(parent_pid_str) = env::var(DTR_PARENT_PID) else {
        return;
    };
    let Ok(parent_pid) = parent_pid_str.parse::<u32>() else {
        return;
    };
    let Ok(renamed_exe) = env::var(DTR_RENAMED_EXE) else {
        return;
    };
    let renamed_path = PathBuf::from(renamed_exe);

    let _ = io::stdout().flush();
    let _ = io::stderr().flush();

    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::{CloseHandle, FALSE};
        use windows_sys::Win32::System::Threading::{
            INFINITE, OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, TerminateProcess,
            WaitForSingleObject,
        };

        unsafe {
            let handle = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, FALSE, parent_pid);
            if !handle.is_null() {
                let _ = TerminateProcess(handle, 0);
                let _ = WaitForSingleObject(handle, INFINITE);
                let _ = CloseHandle(handle);
            }
        }
        if renamed_path.exists() {
            let _ = fs::remove_file(renamed_path);
        }
    }
    #[cfg(not(windows))]
    {
        let _ = parent_pid;
        if renamed_path.exists() {
            let _ = fs::remove_file(renamed_path);
        }
    }
}

#[cfg(unix)]
pub(crate) fn finish_child_cleanup() {}

pub(crate) fn sweep_stale_old_binaries() {
    if let Ok(current_exe) = env::current_exe() {
        let old_exe = current_exe.with_extension("exe.old");
        if old_exe.exists() {
            let _ = fs::remove_file(old_exe);
        }
    }
    let temp_dir = env::temp_dir();
    if let Ok(entries) = fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str())
                && file_name.starts_with("dtr-trampoline-")
                && file_name.ends_with(".exe")
            {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_dtr_repospec_names() {
        assert!(is_dtr_repospec_name("dtr"));
        assert!(is_dtr_repospec_name("mevanlc/dtr"));
        assert!(is_dtr_repospec_name("https://github.com/mevanlc/dtr"));
        assert!(is_dtr_repospec_name("https://github.com/mevanlc/dtr.git"));
        assert!(!is_dtr_repospec_name("ripgrep"));
        assert!(!is_dtr_repospec_name("BurntSushi/ripgrep"));
    }

    #[test]
    fn detects_dtr_manifest_content() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            "[package]\nname = \"dtr\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert!(is_dtr_manifest(&cargo_toml));

        let other_toml = dir.path().join("Other.toml");
        fs::write(
            &other_toml,
            "[package]\nname = \"bat\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        assert!(!is_dtr_manifest(&other_toml));
    }

    #[test]
    fn resolves_local_package_root_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            "[package]\nname = \"dtr\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        let root = resolve_local_package_root(dir.path().to_str().unwrap());
        assert_eq!(root, Some(dir.path().to_path_buf()));

        let invalid = resolve_local_package_root("https://github.com/mevanlc/dtr");
        assert_eq!(invalid, None);
    }

    #[test]
    fn sweeps_stale_old_binaries_in_temp_dir() {
        let dir = tempfile::tempdir().unwrap();
        let old_file = dir.path().join("fake_dtr.exe.old");
        fs::write(&old_file, "stale binary").unwrap();
        assert!(old_file.exists());

        if old_file.exists() {
            let _ = fs::remove_file(&old_file);
        }
        assert!(!old_file.exists());

        let trampoline_file = dir.path().join("dtr-trampoline-1234-5678.exe");
        fs::write(&trampoline_file, "stale trampoline").unwrap();
        assert!(trampoline_file.exists());
        if trampoline_file.exists() {
            let _ = fs::remove_file(&trampoline_file);
        }
        assert!(!trampoline_file.exists());
    }

    #[test]
    fn is_dtr_target_identifies_local_and_remote() {
        let dir = tempfile::tempdir().unwrap();
        let cargo_toml = dir.path().join("Cargo.toml");
        fs::write(
            &cargo_toml,
            "[package]\nname = \"dtr\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();

        assert!(is_dtr_target(dir.path().to_str().unwrap()));
        assert!(is_dtr_target("mevanlc/dtr"));
        assert!(!is_dtr_target("user/other-tool"));
    }
}
