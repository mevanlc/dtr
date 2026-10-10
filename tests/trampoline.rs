use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::json;
use tempfile::TempDir;

struct Binaries {
    _temporary: TempDir,
    cargo: PathBuf,
    example: PathBuf,
}

fn binaries() -> &'static Binaries {
    static BINARIES: OnceLock<Binaries> = OnceLock::new();
    BINARIES.get_or_init(|| {
        let temporary = tempfile::tempdir().unwrap();
        let cargo = temporary
            .path()
            .join(format!("cargo{}", env::consts::EXE_SUFFIX));
        let status = Command::new("rustc")
            .args(["--edition=2024", "--crate-name", "trampoline_cargo"])
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/trampoline_cargo.rs"))
            .arg("-o")
            .arg(&cargo)
            .status()
            .unwrap();
        assert!(status.success());
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "--locked",
                "--example",
                "dtr-trampoline",
                "--message-format=json",
            ])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let example = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|value| {
                value["reason"] == "compiler-artifact"
                    && value["target"]["name"] == "dtr-trampoline"
            })
            .and_then(|value| value["executable"].as_str().map(PathBuf::from))
            .unwrap();
        Binaries {
            _temporary: temporary,
            cargo,
            example,
        }
    })
}

struct Harness {
    directory: TempDir,
    repo: PathBuf,
    binary: PathBuf,
    example: PathBuf,
    target: PathBuf,
    bin_dir: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let fixtures = binaries();
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("checkout space λ");
        let bin_dir = directory.path().join("tools");
        let target = directory.path().join("custom artifacts λ");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&bin_dir).unwrap();
        fs::create_dir_all(&target).unwrap();
        fs::write(
            repo.join("Cargo.toml"),
            "[package]\nname = 'dtr'\nversion = '0.0.0'\n",
        )
        .unwrap();
        let binary = directory
            .path()
            .join(format!("dtr{}", env::consts::EXE_SUFFIX));
        let example = target.join(format!("dtr-trampoline{}", env::consts::EXE_SUFFIX));
        fs::copy(env!("CARGO_BIN_EXE_dtr"), &binary).unwrap();
        fs::copy(&fixtures.example, &example).unwrap();
        fs::copy(
            &fixtures.cargo,
            bin_dir.join(format!("cargo{}", env::consts::EXE_SUFFIX)),
        )
        .unwrap();
        let this = Self {
            directory,
            repo,
            binary,
            example,
            target,
            bin_dir,
        };
        this.write_metadata();
        this.write_artifact(Some(&this.example));
        this
    }

    fn write_metadata(&self) {
        fs::write(self.directory.path().join("metadata.json"), json!({
            "target_directory": self.target.canonicalize().unwrap(),
            "packages": [{"id": "fixture-dtr", "manifest_path": self.repo.join("Cargo.toml").canonicalize().unwrap()}]
        }).to_string()).unwrap();
    }

    fn write_artifact(&self, executable: Option<&Path>) {
        fs::write(self.directory.path().join("artifact.json"), format!("{}\n", json!({
            "reason": "compiler-artifact", "package_id": "fixture-dtr",
            "target": {"name": "dtr-trampoline", "kind": ["example"], "crate_types": ["bin"]},
            "profile": {"test": false}, "fresh": true, "executable": executable,
        }))).unwrap();
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(&self.binary);
        let mut paths = vec![self.bin_dir.clone()];
        paths.extend(env::split_paths(&env::var_os("PATH").unwrap()));
        command
            .args(args)
            .current_dir(&self.repo)
            .env("PATH", env::join_paths(paths).unwrap())
            .env("DTR_CONFIG_DIR", self.directory.path().join("config"))
            .env("DTR_FIXTURE", self.directory.path())
            .env("DTR_FIXTURE_ORIGINAL", &self.binary)
            .env("DTR_FIXTURE_REPLACEMENT", env!("CARGO_BIN_EXE_dtr"))
            .env_remove("DTR_SELF_REEXEC")
            .env_remove("DTR_SELF_LOCKS");
        command
    }

    fn install(&self) -> Command {
        self.command(&["install", "--tool", "cargo", "."])
    }

    fn log(&self, name: &str) -> String {
        fs::read_to_string(self.directory.path().join(name)).unwrap()
    }

    fn assert_intact(&self) {
        assert_eq!(
            fs::read(&self.binary).unwrap(),
            fs::read(env!("CARGO_BIN_EXE_dtr")).unwrap()
        );
        assert!(!self.binary.with_extension("exe.old").exists());
    }

    fn wait_ready(&self) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.directory.path().join("ready").exists() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for subprocess"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "status: {:?}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn example_reexec_preserves_arguments_directory_environment_and_tracking() {
    for prefix in [
        vec!["install"],
        vec!["install", "--add"],
        vec!["kit", "add"],
    ] {
        let harness = Harness::new();
        let mut args = prefix.clone();
        args.extend([
            "--tool",
            "cargo",
            ".",
            "--",
            "--locked",
            "--features",
            "a b",
        ]);
        let output = success(harness.command(&args).output().unwrap());
        assert!(String::from_utf8_lossy(&output.stderr).contains("fixture build diagnostic"));
        let build = harness.log("build.args");
        assert!(
            build.contains("\"--example\", \"dtr-trampoline\""),
            "{build}"
        );
        assert!(build.contains("\"--target\", \"host-tuple\""), "{build}");
        assert!(
            harness
                .log("install.args")
                .contains("\"--locked\", \"--features\", \"a b\"")
        );
        assert_eq!(harness.log("install.reexec"), "1");
        assert_eq!(harness.log("install.locks"), "");
        assert_eq!(
            PathBuf::from(harness.log("install.cwd"))
                .canonicalize()
                .unwrap(),
            harness.repo.canonicalize().unwrap()
        );
        if prefix.len() > 1 {
            assert!(harness.directory.path().join("config/kit.toml").is_file());
        }
        // Completion is a normal non-dry-run command and triggers backup cleanup.
        success(harness.command(&["completion", "bash"]).output().unwrap());
        harness.assert_intact();
    }
}

#[test]
fn kit_self_install_hands_off_once_and_returns_job_failures() {
    let harness = Harness::new();
    let config = harness.directory.path().join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("kit.toml"),
        "[[install]]\nrepospec = '.'\ntool = 'cargo'\n",
    )
    .unwrap();
    let output = harness
        .command(&["kit", "install"])
        .env("DTR_FIXTURE_INSTALL_EXIT", "23")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(harness.log("install.reexec"), "1");
    harness.assert_intact();
}

#[test]
fn kit_failure_after_successful_self_install_preserves_the_replacement() {
    let harness = Harness::new();
    let config = harness.directory.path().join("config");
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(harness.repo.join("other-tool")).unwrap();
    fs::write(config.join("kit.toml"), "[[install]]\nrepospec = '.'\ntool = 'cargo'\n\n[[install]]\nrepospec = './other-tool'\ntool = 'cargo'\n").unwrap();
    let output = harness
        .command(&["kit", "install", "--jobs", "1"])
        .env("DTR_FIXTURE_FAIL_OTHER", "1")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(harness.binary.is_file());
    #[cfg(windows)]
    assert!(harness.binary.with_extension("exe.old").is_file());
    success(harness.command(&["completion", "bash"]).output().unwrap());
    harness.assert_intact();
}

#[test]
fn dry_run_does_not_build_or_clean_up_backups() {
    let harness = Harness::new();
    let backup = harness.binary.with_extension("exe.old");
    fs::write(&backup, "leave untouched").unwrap();
    success(
        harness
            .command(&["--dry-run", "install", "--tool", "cargo", "."])
            .output()
            .unwrap(),
    );
    assert!(!harness.directory.path().join("metadata.args").exists());
    assert_eq!(fs::read_to_string(backup).unwrap(), "leave untouched");
}

#[test]
fn build_artifact_and_launch_failures_leave_the_original_usable() {
    let harness = Harness::new();
    let output = harness
        .install()
        .env("DTR_FIXTURE_BUILD_EXIT", "17")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!harness.directory.path().join("install.args").exists());
    harness.assert_intact();
    harness.write_artifact(None);
    let output = harness.install().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("did not report"));
    harness.assert_intact();
    let missing = harness.target.join("missing.exe");
    harness.write_artifact(Some(&missing));
    assert_eq!(harness.install().output().unwrap().status.code(), Some(2));
    harness.assert_intact();
    // An existing non-executable artifact passes existence validation but fails
    // at process creation, after the Windows rename.
    let invalid = harness.target.join("invalid.exe");
    fs::write(&invalid, "not an executable").unwrap();
    harness.write_artifact(Some(&invalid));
    assert_eq!(harness.install().output().unwrap().status.code(), Some(2));
    harness.assert_intact();
}

#[test]
fn installer_failure_and_alternate_install_root_restore_the_original() {
    let harness = Harness::new();
    let output = harness
        .install()
        .env("DTR_FIXTURE_INSTALL_EXIT", "23")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(23),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    harness.assert_intact();
    success(
        harness
            .install()
            .env("DTR_FIXTURE_NO_REPLACEMENT", "1")
            .output()
            .unwrap(),
    );
    harness.assert_intact();
}

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn concurrent_self_installs_fail_fast_during_build_and_execution() {
    for stage in ["build", "install", "published"] {
        let first = Harness::new();
        let mut owner = Running(
            first
                .install()
                .env("DTR_FIXTURE_WAIT", stage)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        first.wait_ready();
        // Different installed executable, shared output directory: validates
        // the target lock independently of the executable lock.
        let mut second = Harness::new();
        second.target = first.target.clone();
        second.write_metadata();
        let output = second.install().output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("self-install already in progress")
        );
        assert!(!second.directory.path().join("build.args").exists());
        // The original name exists before the rename and after publishing.
        if stage != "install" {
            let output = first.install().output().unwrap();
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("self-install already in progress")
            );
        }
        if stage == "published" {
            success(first.command(&["completion", "bash"]).output().unwrap());
            #[cfg(windows)]
            assert!(first.binary.with_extension("exe.old").is_file());
        }
        fs::write(first.directory.path().join("release"), "").unwrap();
        assert!(owner.0.wait().unwrap().success());
        // Completion proves the helper releases its inherited locks on Unix.
        success(second.install().output().unwrap());
    }
}

#[cfg(unix)]
#[test]
fn malformed_inherited_lock_protocol_is_rejected_without_panicking() {
    let harness = Harness::new();
    let output = harness
        .command(&["completion", "bash"])
        .env("DTR_SELF_REEXEC", "1")
        .env(
            "DTR_SELF_LOCKS",
            r#"[{"fd":0,"device":0,"inode":0},{"fd":1,"device":0,"inode":0}]"#,
        )
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid inherited self-install locks")
    );
}

#[cfg(unix)]
#[test]
fn descendants_do_not_adopt_their_ancestors_lock_descriptors() {
    let harness = Harness::new();
    success(
        harness
            .command(&["completion", "bash"])
            .env("DTR_SELF_REEXEC", "1")
            .env(
                "DTR_SELF_LOCKS",
                json!({"pid": std::process::id(), "locks": [
                    {"fd": 0, "device": 0, "inode": 0},
                    {"fd": 1, "device": 0, "inode": 0}
                ]})
                .to_string(),
            )
            .output()
            .unwrap(),
    );
}

#[test]
fn console_interrupt_during_build_and_install_returns_130_and_recovers() {
    for stage in ["build", "install"] {
        let harness = Harness::new();
        let mut command = harness.install();
        command
            .env("DTR_FIXTURE_WAIT", stage)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            unsafe {
                AllocConsole();
            }
            command.creation_flags(0x0000_0200); // CREATE_NEW_PROCESS_GROUP
        }
        let mut running = Running(command.spawn().unwrap());
        harness.wait_ready();
        #[cfg(unix)]
        assert_eq!(
            unsafe { libc::kill(-(running.0.id() as i32), libc::SIGINT) },
            0
        );
        #[cfg(windows)]
        assert_ne!(unsafe { GenerateConsoleCtrlEvent(1, running.0.id()) }, 0); // CTRL_BREAK_EVENT
        let deadline = Instant::now() + Duration::from_secs(10);
        let status = loop {
            if let Some(status) = running.0.try_wait().unwrap() {
                break status;
            }
            assert!(
                Instant::now() < deadline,
                "interrupted supervisor did not exit"
            );
            thread::sleep(Duration::from_millis(10));
        };
        #[cfg(windows)]
        assert_eq!(status.code(), Some(130));
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert!(
                status.code() == Some(130)
                    || (stage == "install" && status.signal() == Some(libc::SIGINT)),
                "{status:?}"
            );
        }
        harness.assert_intact();
        success(harness.install().output().unwrap());
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn AllocConsole() -> i32;
    fn GenerateConsoleCtrlEvent(event: u32, group: u32) -> i32;
}

#[test]
#[ignore = "builds the release helper and performs real isolated Cargo installs"]
fn real_cargo_repeated_self_install_keeps_only_dtr_installed() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("install root λ");
    fs::create_dir_all(root.join("bin")).unwrap();
    let executable = root
        .join("bin")
        .join(format!("dtr{}", env::consts::EXE_SUFFIX));
    fs::copy(env!("CARGO_BIN_EXE_dtr"), &executable).unwrap();
    let project = Path::new(env!("CARGO_MANIFEST_DIR"))
        .canonicalize()
        .unwrap();
    let metadata = success(
        Command::new(env!("CARGO"))
            .args(["metadata", "--no-deps", "--format-version=1"])
            .current_dir(&project)
            .output()
            .unwrap(),
    );
    let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
    let target = PathBuf::from(metadata["target_directory"].as_str().unwrap());
    // Exercise a relative override when the build directory is inside the
    // project, while retaining external cache directories used by CI/containers.
    let target_override = target.strip_prefix(&project).unwrap_or(&target);
    for _ in 0..2 {
        let output = Command::new(&executable)
            .args([
                "install",
                "--tool",
                "cargo",
                env!("CARGO_MANIFEST_DIR"),
                "--",
                "--locked",
                "--debug",
                "--force",
                "--target",
                "host-tuple",
                "--root",
            ])
            .arg(&root)
            .current_dir(&project)
            .env("CARGO_TARGET_DIR", target_override)
            // Both helper and installer explicitly choose a runnable host
            // target even when the environment defaults to cross-compilation.
            .env("CARGO_BUILD_TARGET", "unusable-default-target")
            .env("DTR_CONFIG_DIR", directory.path().join("config"))
            .env_remove("DTR_SELF_REEXEC")
            .env_remove("DTR_SELF_LOCKS")
            .output()
            .unwrap();
        success(output);
        #[cfg(windows)]
        assert!(executable.with_extension("exe.old").is_file());
        success(
            Command::new(&executable)
                .args(["completion", "bash"])
                .output()
                .unwrap(),
        );
        assert!(!executable.with_extension("exe.old").exists());
        assert_eq!(fs::read_dir(root.join("bin")).unwrap().count(), 1);
    }
}
