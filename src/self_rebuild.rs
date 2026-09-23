use std::env;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::InstallTool;
use crate::kit::InstallEntry;
use crate::repospec::RepoSpec;

pub(crate) const DTR_SELF_REEXEC: &str = "DTR_SELF_REEXEC";
pub(crate) const DTR_SELF_LOCKS: &str = "DTR_SELF_LOCKS";

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

mod runtime;

pub(crate) use runtime::{adopt_locks, execute, sweep_stale_old_binary};

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
