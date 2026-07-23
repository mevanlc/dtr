#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

struct Harness {
    _temp: TempDir,
    bin: PathBuf,
    log: PathBuf,
    work: PathBuf,
}

impl Harness {
    fn new(programs: &[&str]) -> Self {
        let temp = tempfile::tempdir().expect("temporary directory");
        let bin = temp.path().join("bin");
        let log = temp.path().join("log");
        let work = temp.path().join("work");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&log).unwrap();
        fs::create_dir_all(&work).unwrap();
        for program in programs {
            write_stub(&bin.join(program));
        }
        Self {
            _temp: temp,
            bin,
            log,
            work,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command(args).output().expect("dtr should start")
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dtr"));
        command
            .args(args)
            .current_dir(&self.work)
            .env_clear()
            .env("PATH", &self.bin)
            .env("DTR_TEST_LOG_DIR", &self.log)
            .env("DTR_TEST_GH_OWNER", "mevanlc");
        command
    }

    fn invocation(&self, program: &str) -> Invocation {
        let text = fs::read_to_string(self.log.join(program))
            .unwrap_or_else(|error| panic!("missing {program} invocation: {error}"));
        let mut cwd = None;
        let mut args = Vec::new();
        for line in text.lines() {
            if let Some(value) = line
                .strip_prefix("cwd=<")
                .and_then(|line| line.strip_suffix('>'))
            {
                cwd = Some(PathBuf::from(value));
            } else if let Some(value) = line
                .strip_prefix("arg=<")
                .and_then(|line| line.strip_suffix('>'))
            {
                args.push(value.to_owned());
            }
        }
        Invocation {
            cwd: cwd.expect("stub recorded cwd"),
            args,
        }
    }

    fn was_invoked(&self, program: &str) -> bool {
        self.log.join(program).exists()
    }
}

#[derive(Debug)]
struct Invocation {
    cwd: PathBuf,
    args: Vec<String>,
}

fn write_stub(path: &Path) {
    let script = r#"#!/bin/sh
if [ "${0##*/}" = git ] && [ "${1-}" = clone ] && [ "${2-}" = -h ]; then
  printf '%s\n' \
    'usage: git clone [<options>] [--] <repo> [<dir>]' \
    '    -v, --[no-]verbose    be more verbose' \
    '    -q, --[no-]quiet      be more quiet' \
    '    -n, --no-checkout     do not create a checkout' \
    '    -b, --[no-]branch <branch>  checkout a branch' \
    '    --[no-]depth <depth>  create a shallow clone' \
    '    --[no-]recursive[=<pathspec>]' >&2
  exit 129
fi

if [ "${0##*/}" = gh ] && [ "${1-}" = api ] && [ "${2-}" = user ]; then
  printf '%s\n' "${DTR_TEST_GH_OWNER}"
  exit 0
fi

log="${DTR_TEST_LOG_DIR}/${0##*/}"
: > "$log"
printf 'cwd=<%s>\n' "$PWD" >> "$log"
for arg in "$@"; do
  printf 'arg=<%s>\n' "$arg" >> "$log"
done
exit "${DTR_TEST_EXIT-0}"
"#;
    fs::write(path, script).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn github_clone_prefers_gh_and_forwards_flexible_git_options() {
    let harness = Harness::new(&["git", "gh"]);
    let output = harness.run(&[
        "clone",
        "--depth",
        "1",
        "owner/repo",
        "--branch=main",
        "destination",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("gh").args,
        [
            "repo",
            "clone",
            "owner/repo",
            "destination",
            "--",
            "--depth",
            "1",
            "--branch=main",
        ]
    );
}

#[test]
fn github_url_prefers_gh_and_normalizes_a_trailing_slash() {
    let harness = Harness::new(&["git", "gh"]);
    let output = harness.run(&["clone", "https://github.com/owner/repo.git/", "destination"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("gh").args,
        [
            "repo",
            "clone",
            "https://github.com/owner/repo.git",
            "destination",
        ]
    );
}

#[test]
fn github_clone_falls_back_to_git_when_gh_is_absent() {
    let harness = Harness::new(&["git"]);
    let output = harness.run(&["clone", "owner/repo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("git").args,
        ["clone", "https://github.com/owner/repo.git"]
    );
}

#[test]
fn bare_repo_name_is_delegated_to_authenticated_gh_context() {
    let harness = Harness::new(&["git", "gh"]);
    let output = harness.run(&["clone", "my-tool"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(harness.invocation("gh").args, ["repo", "clone", "my-tool"]);
}

#[test]
fn local_and_generic_repositories_go_directly_to_git() {
    for repository in ["./local/repo", "https://example.com/team/repo.git"] {
        let harness = Harness::new(&["git", "gh", "glab"]);
        let output = harness.run(&["clone", repository, "destination"]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(
            harness.invocation("git").args,
            ["clone", repository, "destination"]
        );
        assert!(!harness.was_invoked("gh"));
        assert!(!harness.was_invoked("glab"));
    }
}

#[test]
fn gitlab_clone_prefers_glab_and_preserves_nested_owner_directory() {
    let harness = Harness::new(&["git", "glab"]);
    let remote = "https://gitlab.com/group/subgroup/repo.git";
    let output = harness.run(&["clone", "-D", remote, "-n"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("glab").args,
        ["repo", "clone", remote, "group/subgroup/repo", "--", "-n",]
    );
    assert!(harness.work.join("group/subgroup").is_dir());
}

#[test]
fn gitlab_clone_falls_back_to_git_when_glab_is_absent() {
    let harness = Harness::new(&["git"]);
    let remote = "https://gitlab.com/group/repo.git/";
    let output = harness.run(&["clone", remote]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("git").args,
        ["clone", "https://gitlab.com/group/repo.git"]
    );
}

#[test]
fn explain_reports_the_exact_plan_without_starting_the_operation() {
    let harness = Harness::new(&["git", "gh"]);
    let output = harness.run(&["-n", "clone", "-O", "owner/repo", "--depth=1"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!harness.was_invoked("gh"));
    assert_eq!(
        stdout(&output),
        "repospec: GitHub repository owner/repo\n\
backend:  gh\n\
target:   owner--repo\n\
command:  gh repo clone owner/repo owner--repo -- --depth=1\n"
    );
}

#[test]
fn explain_reports_directory_preparation_without_creating_it() {
    let harness = Harness::new(&["git", "gh"]);
    let output = harness.run(&["-n", "clone", "-D", "owner/repo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("prepare:  mkdir -p owner\n"));
    assert!(!harness.work.join("owner").exists());
    assert!(!harness.was_invoked("gh"));
}

#[test]
fn clone_n_after_subcommand_remains_git_no_checkout() {
    let harness = Harness::new(&["git", "gh"]);
    let output = harness.run(&["clone", "-n", "owner/repo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("gh").args,
        ["repo", "clone", "owner/repo", "--", "-n"]
    );
}

#[test]
fn go_remote_install_adds_latest_by_default() {
    let harness = Harness::new(&["go"]);
    let output = harness.run(&["install", "--go", "https://github.com/hjr265/gittop"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("go").args,
        ["install", "github.com/hjr265/gittop@latest"]
    );
}

#[test]
fn go_remote_install_honors_no_latest_and_i_alias() {
    let harness = Harness::new(&["go"]);
    let output = harness.run(&["i", "--go", "--no-latest", "git@example.com:owner/tool.git"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("go").args,
        ["install", "example.com/owner/tool"]
    );
}

#[test]
fn bare_go_repo_uses_authenticated_github_owner() {
    let harness = Harness::new(&["go", "gh"]);
    let output = harness.run(&["install", "--go", "my-tool"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("go").args,
        ["install", "github.com/mevanlc/my-tool@latest"]
    );
}

#[test]
fn local_go_repo_installs_all_commands_from_that_directory() {
    let harness = Harness::new(&["go"]);
    let repo = harness.work.join("local repo");
    fs::create_dir(&repo).unwrap();
    let output = harness.run(&["install", "--go", "./local repo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let invocation = harness.invocation("go");
    assert_eq!(
        fs::canonicalize(invocation.cwd).unwrap(),
        fs::canonicalize(repo).unwrap()
    );
    assert_eq!(invocation.args, ["install", "./..."]);
}

#[test]
fn no_latest_is_rejected_for_local_repo() {
    let harness = Harness::new(&["go"]);
    let output = harness.run(&["install", "--go", "--no-latest", "./repo"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("--no-latest applies only to remote"));
    assert!(!harness.was_invoked("go"));
}

#[test]
fn child_exit_status_is_propagated() {
    let harness = Harness::new(&["git", "gh"]);
    let output = harness
        .command(&["clone", "owner/repo"])
        .env("DTR_TEST_EXIT", "7")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
}

#[test]
fn missing_required_tool_gets_a_focused_error() {
    let harness = Harness::new(&[]);
    let output = harness.run(&["install", "--go", "owner/repo"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("go is required"));
}
