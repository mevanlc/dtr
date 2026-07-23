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
    config: PathBuf,
}

impl Harness {
    fn new(programs: &[&str]) -> Self {
        let temp = tempfile::tempdir().expect("temporary directory");
        let bin = temp.path().join("bin");
        let log = temp.path().join("log");
        let work = temp.path().join("work");
        let config = temp.path().join("config");
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
            config,
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
            .env("DTR_TEST_GH_OWNER", "mevanlc")
            .env("DTR_CONFIG_DIR", &self.config);
        command
    }

    fn invocation(&self, program: &str) -> Invocation {
        let text = fs::read_to_string(self.log.join(program))
            .unwrap_or_else(|error| panic!("missing {program} invocation: {error}"));
        let mut cwd = None;
        let mut args = Vec::new();
        let mut gh_token_account = None;
        let mut github_token_present = false;
        let mut cargo_git_fetch_cli = false;
        let mut uv_no_github_fast_path = false;
        let mut git_config_count = None;
        let mut git_config_keys = Vec::new();
        let mut git_auth_header_present = false;
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
            } else if let Some(value) = line
                .strip_prefix("gh_token_account=<")
                .and_then(|line| line.strip_suffix('>'))
            {
                gh_token_account = Some(value.to_owned());
            } else if line == "github_token_present=<yes>" {
                github_token_present = true;
            } else if line == "cargo_git_fetch_cli=<true>" {
                cargo_git_fetch_cli = true;
            } else if line == "uv_no_github_fast_path=<true>" {
                uv_no_github_fast_path = true;
            } else if let Some(value) = line
                .strip_prefix("git_config_count=<")
                .and_then(|line| line.strip_suffix('>'))
            {
                git_config_count = Some(value.to_owned());
            } else if let Some(value) = line
                .strip_prefix("git_config_key=<")
                .and_then(|line| line.strip_suffix('>'))
            {
                git_config_keys.push(value.to_owned());
            } else if line == "git_auth_header_present=<yes>" {
                git_auth_header_present = true;
            }
        }
        Invocation {
            cwd: cwd.expect("stub recorded cwd"),
            args,
            gh_token_account,
            github_token_present,
            cargo_git_fetch_cli,
            uv_no_github_fast_path,
            git_config_count,
            git_config_keys,
            git_auth_header_present,
        }
    }

    fn was_invoked(&self, program: &str) -> bool {
        self.log.join(program).exists()
    }

    fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }
}

#[derive(Debug)]
struct Invocation {
    cwd: PathBuf,
    args: Vec<String>,
    gh_token_account: Option<String>,
    github_token_present: bool,
    cargo_git_fetch_cli: bool,
    uv_no_github_fast_path: bool,
    git_config_count: Option<String>,
    git_config_keys: Vec<String>,
    git_auth_header_present: bool,
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

if [ "${0##*/}" = gh ] && [ "${1-}" = auth ] && [ "${2-}" = token ]; then
  log="${DTR_TEST_LOG_DIR}/gh-auth-token"
  : > "$log"
  printf 'gh_token_inherited=<%s>\n' "${GH_TOKEN-<unset>}" >> "$log"
  printf 'github_token_inherited=<%s>\n' "${GITHUB_TOKEN-<unset>}" >> "$log"
  for arg in "$@"; do
    printf 'arg=<%s>\n' "$arg" >> "$log"
  done
  if [ "${DTR_TEST_GH_TOKEN_FAIL_ACCOUNT-}" = "${6-}" ]; then
    exit 1
  fi
  printf 'token-%s\n' "${6-}"
  exit 0
fi

log="${DTR_TEST_LOG_DIR}/${0##*/}"
: > "$log"
printf 'cwd=<%s>\n' "$PWD" >> "$log"
if [ -n "${GH_TOKEN-}" ]; then
  case "$GH_TOKEN" in
    token-*) printf 'gh_token_account=<%s>\n' "${GH_TOKEN#token-}" >> "$log" ;;
    *) printf 'gh_token_account=<other>\n' >> "$log" ;;
  esac
fi
if [ -n "${GITHUB_TOKEN-}" ]; then
  printf 'github_token_present=<yes>\n' >> "$log"
fi
if [ "${CARGO_NET_GIT_FETCH_WITH_CLI-}" = true ]; then
  printf 'cargo_git_fetch_cli=<true>\n' >> "$log"
fi
if [ "${UV_NO_GITHUB_FAST_PATH-}" = true ]; then
  printf 'uv_no_github_fast_path=<true>\n' >> "$log"
fi
if [ -n "${GIT_CONFIG_COUNT-}" ]; then
  printf 'git_config_count=<%s>\n' "$GIT_CONFIG_COUNT" >> "$log"
fi
for key in "${GIT_CONFIG_KEY_0-}" "${GIT_CONFIG_KEY_1-}" "${GIT_CONFIG_KEY_2-}"; do
  if [ -n "$key" ]; then
    printf 'git_config_key=<%s>\n' "$key" >> "$log"
  fi
done
case "${GIT_CONFIG_VALUE_0-}${GIT_CONFIG_VALUE_1-}${GIT_CONFIG_VALUE_2-}" in
  *'Authorization: Basic '*) printf 'git_auth_header_present=<yes>\n' >> "$log" ;;
esac
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
fn config_set_get_and_unset_round_trip_the_auto_switch_allowlist() {
    let harness = Harness::new(&[]);
    let set = harness.run(&[
        "config",
        "set",
        "github.auth.auto_switch",
        " mevanlc,MIKE-clark-8192,MeVaNlC ",
    ]);
    assert!(set.status.success(), "{}", stderr(&set));

    let config = fs::read_to_string(harness.config_file()).unwrap();
    assert!(config.contains("auto_switch = ["));
    assert!(config.contains("\"mevanlc\""));
    assert!(config.contains("\"MIKE-clark-8192\""));
    assert!(!config.contains("token-"));

    let get = harness.run(&["config", "get", "github.auth.auto_switch"]);
    assert!(get.status.success(), "{}", stderr(&get));
    assert_eq!(stdout(&get), "mevanlc,MIKE-clark-8192\n");

    for _ in 0..2 {
        let unset = harness.run(&["config", "unset", "github.auth.auto_switch"]);
        assert!(unset.status.success(), "{}", stderr(&unset));
    }
    let get = harness.run(&["config", "get", "github.auth.auto_switch"]);
    assert_eq!(get.status.code(), Some(2));
    assert!(stderr(&get).contains("is not set"));
}

#[test]
fn config_rejects_unknown_keys_and_invalid_account_lists() {
    let harness = Harness::new(&[]);
    let unknown = harness.run(&["config", "set", "github.auth.surprise", "mevanlc"]);
    assert_eq!(unknown.status.code(), Some(2));
    assert!(stderr(&unknown).contains("unknown configuration key"));

    for value in ["", "mevanlc,", "not an account"] {
        let output = harness.run(&["config", "set", "github.auth.auto_switch", value]);
        assert_eq!(output.status.code(), Some(2), "{value:?}");
    }
    assert!(!harness.config_file().exists());
}

#[test]
fn malformed_auth_configuration_blocks_github_account_resolution() {
    let harness = Harness::new(&["git", "gh"]);
    fs::create_dir_all(&harness.config).unwrap();
    fs::write(
        harness.config_file(),
        "[github.auth]\nauto_switch = [\"mevanlc\"]\nsurprise = true\n",
    )
    .unwrap();

    let output = harness.run(&["clone", "mevanlc/foo"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("could not parse configuration"));
    assert!(stderr(&output).contains("unknown field"));
    assert!(!harness.was_invoked("gh-auth-token"));
    assert!(!harness.was_invoked("gh"));
}

#[test]
fn explain_is_rejected_for_mutating_config_commands() {
    let harness = Harness::new(&[]);
    let output = harness.run(&["-n", "config", "set", "github.auth.auto_switch", "mevanlc"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("--explain does not apply"));
    assert!(!harness.config_file().exists());
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
fn github_owner_match_auto_switches_only_the_clone_process() {
    let harness = Harness::new(&["git", "gh"]);
    let set = harness.run(&[
        "config",
        "set",
        "github.auth.auto_switch",
        "mevanlc,mike-clark-8192",
    ]);
    assert!(set.status.success(), "{}", stderr(&set));

    let output = harness.run(&["clone", "mike-clark-8192/foo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let token_lookup = fs::read_to_string(harness.log.join("gh-auth-token")).unwrap();
    assert!(token_lookup.contains("arg=<--user>\narg=<mike-clark-8192>\n"));
    assert!(token_lookup.contains("gh_token_inherited=<<unset>>"));
    assert!(token_lookup.contains("github_token_inherited=<<unset>>"));

    let clone = harness.invocation("gh");
    assert_eq!(
        clone.args,
        [
            "repo",
            "clone",
            "https://github.com/mike-clark-8192/foo.git",
        ]
    );
    assert_eq!(clone.gh_token_account.as_deref(), Some("mike-clark-8192"));
}

#[test]
fn github_url_owner_match_is_case_insensitive_and_forces_https() {
    let harness = Harness::new(&["git", "gh"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "MeVaNlC",])
            .status
            .success()
    );

    let output = harness.run(&["clone", "http://github.com/mevanlc/bar"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let clone = harness.invocation("gh");
    assert_eq!(
        clone.args,
        ["repo", "clone", "https://github.com/mevanlc/bar.git"]
    );
    assert_eq!(clone.gh_token_account.as_deref(), Some("MeVaNlC"));
}

#[test]
fn unmatched_and_bare_github_repositories_keep_active_account_behavior() {
    let harness = Harness::new(&["git", "gh"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc",])
            .status
            .success()
    );

    let unmatched = harness.run(&["clone", "cli/cli"]);
    assert!(unmatched.status.success(), "{}", stderr(&unmatched));
    let clone = harness.invocation("gh");
    assert_eq!(clone.args, ["repo", "clone", "cli/cli"]);
    assert_eq!(clone.gh_token_account, None);
    assert!(!harness.was_invoked("gh-auth-token"));

    let bare = harness.run(&["clone", "my-tool"]);
    assert!(bare.status.success(), "{}", stderr(&bare));
    let clone = harness.invocation("gh");
    assert_eq!(clone.args, ["repo", "clone", "my-tool"]);
    assert_eq!(clone.gh_token_account, None);
    assert!(!harness.was_invoked("gh-auth-token"));
}

#[test]
fn configured_owner_match_fails_closed_when_token_lookup_fails() {
    let harness = Harness::new(&["git", "gh"]);
    assert!(
        harness
            .run(&[
                "config",
                "set",
                "github.auth.auto_switch",
                "mike-clark-8192",
            ])
            .status
            .success()
    );

    let output = harness
        .command(&["clone", "mike-clark-8192/private"])
        .env("DTR_TEST_GH_TOKEN_FAIL_ACCOUNT", "mike-clark-8192")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("auto-switch account mike-clark-8192"));
    assert!(!harness.was_invoked("gh"));
}

#[test]
fn parent_token_variables_do_not_override_auto_switch_lookup() {
    let harness = Harness::new(&["git", "gh"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc",])
            .status
            .success()
    );

    let output = harness
        .command(&["clone", "mevanlc/foo"])
        .env("GH_TOKEN", "parent-gh-token")
        .env("GITHUB_TOKEN", "parent-github-token")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let token_lookup = fs::read_to_string(harness.log.join("gh-auth-token")).unwrap();
    assert!(token_lookup.contains("gh_token_inherited=<<unset>>"));
    assert!(token_lookup.contains("github_token_inherited=<<unset>>"));
    assert_eq!(
        harness.invocation("gh").gh_token_account.as_deref(),
        Some("mevanlc")
    );
    assert!(!harness.invocation("gh").github_token_present);
}

#[test]
fn explain_reports_auto_switch_without_exposing_or_using_the_token() {
    let harness = Harness::new(&["git", "gh"]);
    assert!(
        harness
            .run(&[
                "config",
                "set",
                "github.auth.auto_switch",
                "mike-clark-8192",
            ])
            .status
            .success()
    );

    let output = harness.run(&["-n", "clone", "mike-clark-8192/foo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "repospec: GitHub repository mike-clark-8192/foo\n\
backend:  gh\n\
auth:     auto-switch to mike-clark-8192 (process-scoped; active gh account unchanged)\n\
target:   foo\n\
command:  gh repo clone https://github.com/mike-clark-8192/foo.git\n"
    );
    assert!(!stdout(&output).contains("token-"));
    assert!(!stderr(&output).contains("token-"));
    assert!(harness.was_invoked("gh-auth-token"));
    assert!(!harness.was_invoked("gh"));
}

#[test]
fn auto_switch_configuration_does_not_remove_missing_gh_fallback() {
    let harness = Harness::new(&["git"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc",])
            .status
            .success()
    );

    let output = harness.run(&["clone", "mevanlc/foo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("git").args,
        ["clone", "https://github.com/mevanlc/foo.git"]
    );
    assert!(!harness.was_invoked("gh-auth-token"));
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
fn rust_local_install_maps_to_cargo_path_and_preserves_native_arguments() {
    let harness = Harness::new(&["cargo"]);
    let output = harness.run(&[
        "install",
        "--rust",
        "./local repo",
        "--",
        "--locked",
        "--bin",
        "tool",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("cargo").args,
        [
            "install",
            "--path",
            "./local repo",
            "--locked",
            "--bin",
            "tool",
        ]
    );
}

#[test]
fn cargo_alias_and_remote_repositories_map_to_cargo_git() {
    let cases: &[(&[&str], &[&str])] = &[
        (
            &["install", "--cargo", "owner/tool", "--", "--locked"],
            &[
                "install",
                "--git",
                "https://github.com/owner/tool.git",
                "--locked",
            ],
        ),
        (
            &["install", "--rust", "http://gitlab.com/group/subgroup/tool"],
            &[
                "install",
                "--git",
                "https://gitlab.com/group/subgroup/tool.git",
            ],
        ),
        (
            &["install", "--rust", "ssh://git@example.com/srv/tool.git"],
            &["install", "--git", "ssh://git@example.com/srv/tool.git"],
        ),
        (
            &["install", "--rust", "git@example.com:owner/tool.git"],
            &["install", "--git", "ssh://git@example.com/~/owner/tool.git"],
        ),
        (
            &["install", "--rust", "git@example.com:/srv/tool.git"],
            &["install", "--git", "ssh://git@example.com/srv/tool.git"],
        ),
    ];

    for (dtr_args, cargo_args) in cases {
        let harness = Harness::new(&["cargo"]);
        let output = harness.run(dtr_args);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(harness.invocation("cargo").args, *cargo_args);
    }
}

#[test]
fn bare_rust_repo_uses_the_active_github_owner() {
    let harness = Harness::new(&["cargo", "gh"]);
    let output = harness.run(&["install", "--rust", "my-tool"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("cargo").args,
        ["install", "--git", "https://github.com/mevanlc/my-tool.git",]
    );
    assert!(!harness.was_invoked("gh-auth-token"));
}

#[test]
fn cargo_source_arguments_cannot_replace_the_resolved_repository() {
    for argument in [
        "--git",
        "--git=https://example.com/other",
        "--path",
        "--path=./other",
        "--registry",
        "--registry=private",
        "--index",
        "--index=https://example.com/index",
    ] {
        let harness = Harness::new(&["cargo"]);
        let output = harness.run(&["install", "--rust", "owner/tool", "--", argument]);
        assert_eq!(output.status.code(), Some(2), "{argument}");
        assert!(stderr(&output).contains("conflicts with dtr's resolved repository"));
        assert!(!harness.was_invoked("cargo"));
    }
}

#[test]
fn rust_rejects_no_latest_and_go_rejects_cargo_arguments() {
    let harness = Harness::new(&["cargo", "go"]);
    let rust = harness.run(&["install", "--rust", "--no-latest", "owner/tool"]);
    assert_eq!(rust.status.code(), Some(2));
    assert!(stderr(&rust).contains("applies only to the Go installer"));
    assert!(!harness.was_invoked("cargo"));

    let go = harness.run(&["install", "--go", "owner/tool", "--", "--locked"]);
    assert_eq!(go.status.code(), Some(2));
    assert!(stderr(&go).contains("Rust/Cargo, uv, and pipx installers"));
    assert!(!harness.was_invoked("go"));
}

#[test]
fn missing_cargo_and_cargo_exit_status_are_reported() {
    let missing = Harness::new(&[]);
    let output = missing.run(&["install", "--rust", "owner/tool"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("cargo is required"));

    let failing = Harness::new(&["cargo"]);
    let output = failing
        .command(&["install", "--rust", "owner/tool"])
        .env("DTR_TEST_EXIT", "17")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(17));
}

#[test]
fn rust_install_auto_switches_github_git_auth_without_token_variables() {
    let harness = Harness::new(&["cargo", "gh"]);
    assert!(
        harness
            .run(&[
                "config",
                "set",
                "github.auth.auto_switch",
                "mevanlc,mike-clark-8192",
            ])
            .status
            .success()
    );

    let output = harness.run(&["install", "--rust", "mike-clark-8192/tool"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let token_lookup = fs::read_to_string(harness.log.join("gh-auth-token")).unwrap();
    assert!(token_lookup.contains("arg=<--user>\narg=<mike-clark-8192>\n"));

    let cargo = harness.invocation("cargo");
    assert_eq!(
        cargo.args,
        [
            "install",
            "--git",
            "https://github.com/mike-clark-8192/tool.git",
        ]
    );
    assert!(cargo.cargo_git_fetch_cli);
    assert_eq!(cargo.git_config_count.as_deref(), Some("2"));
    assert_eq!(
        cargo.git_config_keys,
        [
            "http.https://github.com/.extraHeader",
            "http.https://github.com/.extraHeader",
        ]
    );
    assert!(cargo.git_auth_header_present);
    assert_eq!(cargo.gh_token_account, None);
    assert!(!cargo.github_token_present);
}

#[test]
fn rust_auto_switch_extends_existing_process_git_configuration() {
    let harness = Harness::new(&["cargo", "gh"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc",])
            .status
            .success()
    );

    let output = harness
        .command(&["install", "--rust", "mevanlc/tool"])
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "test.existing")
        .env("GIT_CONFIG_VALUE_0", "preserved")
        .env("GH_TOKEN", "parent-gh-token")
        .env("GITHUB_TOKEN", "parent-github-token")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let cargo = harness.invocation("cargo");
    assert_eq!(cargo.git_config_count.as_deref(), Some("3"));
    assert_eq!(
        cargo.git_config_keys,
        [
            "test.existing",
            "http.https://github.com/.extraHeader",
            "http.https://github.com/.extraHeader",
        ]
    );
    assert!(cargo.git_auth_header_present);
    assert_eq!(cargo.gh_token_account, None);
    assert!(!cargo.github_token_present);
}

#[test]
fn rust_auto_switch_fails_closed_before_cargo() {
    let harness = Harness::new(&["cargo", "gh"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc",])
            .status
            .success()
    );
    let output = harness
        .command(&["install", "--rust", "mevanlc/tool"])
        .env("DTR_TEST_GH_TOKEN_FAIL_ACCOUNT", "mevanlc")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("auto-switch account mevanlc"));
    assert!(!harness.was_invoked("cargo"));
}

#[test]
fn rust_auto_switch_explain_is_exact_and_secret_free() {
    let harness = Harness::new(&["cargo", "gh"]);
    assert!(
        harness
            .run(&[
                "config",
                "set",
                "github.auth.auto_switch",
                "mike-clark-8192",
            ])
            .status
            .success()
    );
    let output = harness.run(&[
        "-n",
        "install",
        "--rust",
        "mike-clark-8192/tool",
        "--",
        "--locked",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "repospec: GitHub repository mike-clark-8192/tool\n\
backend:  cargo\n\
auth:     auto-switch to mike-clark-8192 (process-scoped; active gh account unchanged)\n\
command:  cargo install --git https://github.com/mike-clark-8192/tool.git --locked\n"
    );
    assert!(!stdout(&output).contains("token"));
    assert!(!stdout(&output).contains("Authorization"));
    assert!(harness.was_invoked("gh-auth-token"));
    assert!(!harness.was_invoked("cargo"));
}

#[test]
fn unmatched_and_bare_rust_installs_do_not_auto_switch() {
    let harness = Harness::new(&["cargo", "gh"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc",])
            .status
            .success()
    );
    for repository in ["cli/cli", "my-tool"] {
        let output = harness.run(&["install", "--rust", repository]);
        assert!(output.status.success(), "{}", stderr(&output));
        let cargo = harness.invocation("cargo");
        assert!(!cargo.cargo_git_fetch_cli);
        assert!(!cargo.git_auth_header_present);
        assert!(!harness.was_invoked("gh-auth-token"));
    }
}

#[test]
fn python_local_installs_map_to_exact_uv_and_pipx_commands() {
    let uv = Harness::new(&["uv"]);
    let output = uv.run(&[
        "install",
        "--uv",
        "./local repo",
        "--",
        "--python",
        "3.14",
        "--force",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        uv.invocation("uv").args,
        [
            "tool",
            "install",
            "./local repo",
            "--python",
            "3.14",
            "--force",
        ]
    );

    let pipx = Harness::new(&["pipx"]);
    let output = pipx.run(&[
        "install",
        "--pipx",
        "./local repo",
        "--",
        "--python=3.14",
        "--force",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        pipx.invocation("pipx").args,
        ["install", "--python=3.14", "--force", "--", "./local repo",]
    );
}

#[test]
fn python_remote_repositories_map_to_vcs_requirements() {
    let cases: &[(&str, &str)] = &[
        ("owner/tool", "git+https://github.com/owner/tool.git"),
        (
            "http://gitlab.com/group/subgroup/tool",
            "git+https://gitlab.com/group/subgroup/tool.git",
        ),
        (
            "ssh://git@example.com/srv/tool.git",
            "git+ssh://git@example.com/srv/tool.git",
        ),
        (
            "git@example.com:owner/tool.git",
            "git+ssh://git@example.com/~/owner/tool.git",
        ),
        (
            "git@example.com:/srv/tool.git",
            "git+ssh://git@example.com/srv/tool.git",
        ),
    ];

    for (repospec, source) in cases {
        let uv = Harness::new(&["uv"]);
        let output = uv.run(&["install", "--uv", repospec]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(uv.invocation("uv").args, ["tool", "install", source]);

        let pipx = Harness::new(&["pipx"]);
        let output = pipx.run(&["install", "--pipx", repospec]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(pipx.invocation("pipx").args, ["install", "--", source]);
    }
}

#[test]
fn bare_python_repo_uses_the_active_github_owner() {
    for backend in ["--uv", "--pipx"] {
        let program = backend.trim_start_matches("--");
        let harness = Harness::new(&[program, "gh"]);
        let output = harness.run(&["install", backend, "my-tool"]);
        assert!(output.status.success(), "{}", stderr(&output));
        let invocation = harness.invocation(program);
        assert!(
            invocation
                .args
                .contains(&"git+https://github.com/mevanlc/my-tool.git".to_owned())
        );
        assert!(!harness.was_invoked("gh-auth-token"));
    }
}

#[test]
fn pipx_native_arguments_cannot_add_or_replace_a_source() {
    for argument in [
        "another-package",
        "3.14",
        "--",
        "--lock",
        "--lock=pylock.toml",
    ] {
        let harness = Harness::new(&["pipx"]);
        let output = harness.run(&["install", "--pipx", "owner/tool", "--", argument]);
        assert_eq!(output.status.code(), Some(2), "{argument}");
        assert!(!harness.was_invoked("pipx"));
    }
}

#[test]
fn python_installers_reject_go_options_and_propagate_failures() {
    for backend in ["--uv", "--pipx"] {
        let program = backend.trim_start_matches("--");
        let harness = Harness::new(&[program]);
        let rejected = harness.run(&["install", backend, "--no-latest", "owner/tool"]);
        assert_eq!(rejected.status.code(), Some(2));
        assert!(stderr(&rejected).contains("only to the Go installer"));
        assert!(!harness.was_invoked(program));

        let failed = harness
            .command(&["install", backend, "owner/tool"])
            .env("DTR_TEST_EXIT", "19")
            .output()
            .unwrap();
        assert_eq!(failed.status.code(), Some(19));

        let missing = Harness::new(&[]);
        let output = missing.run(&["install", backend, "owner/tool"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains(&format!("{program} is required")));
    }
}

#[test]
fn python_install_auto_switches_with_git_http_auth_only() {
    for backend in ["--uv", "--pipx"] {
        let program = backend.trim_start_matches("--");
        let harness = Harness::new(&[program, "gh"]);
        assert!(
            harness
                .run(&[
                    "config",
                    "set",
                    "github.auth.auto_switch",
                    "mevanlc,mike-clark-8192",
                ])
                .status
                .success()
        );

        let output = harness
            .command(&["install", backend, "mike-clark-8192/tool"])
            .env("GH_TOKEN", "parent-gh-token")
            .env("GITHUB_TOKEN", "parent-github-token")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        let invocation = harness.invocation(program);
        assert!(invocation.uv_no_github_fast_path);
        assert_eq!(invocation.git_config_count.as_deref(), Some("2"));
        assert_eq!(
            invocation.git_config_keys,
            [
                "http.https://github.com/.extraHeader",
                "http.https://github.com/.extraHeader",
            ]
        );
        assert!(invocation.git_auth_header_present);
        assert_eq!(invocation.gh_token_account, None);
        assert!(!invocation.github_token_present);
    }
}

#[test]
fn python_auto_switch_extends_existing_git_config_and_fails_closed() {
    let harness = Harness::new(&["uv", "gh"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc"])
            .status
            .success()
    );
    let output = harness
        .command(&["install", "--uv", "mevanlc/tool"])
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "test.existing")
        .env("GIT_CONFIG_VALUE_0", "preserved")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let uv = harness.invocation("uv");
    assert_eq!(uv.git_config_count.as_deref(), Some("3"));
    assert_eq!(uv.git_config_keys[0], "test.existing");
    assert!(uv.git_auth_header_present);

    let failed = Harness::new(&["uv", "gh"]);
    assert!(
        failed
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc"])
            .status
            .success()
    );
    let output = failed
        .command(&["install", "--uv", "mevanlc/tool"])
        .env("DTR_TEST_GH_TOKEN_FAIL_ACCOUNT", "mevanlc")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!failed.was_invoked("uv"));
}

#[test]
fn python_auto_switch_explain_is_exact_and_secret_free() {
    let harness = Harness::new(&["uv", "gh"]);
    assert!(
        harness
            .run(&[
                "config",
                "set",
                "github.auth.auto_switch",
                "mike-clark-8192",
            ])
            .status
            .success()
    );
    let output = harness.run(&[
        "-n",
        "install",
        "--uv",
        "mike-clark-8192/tool",
        "--",
        "--force",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "repospec: GitHub repository mike-clark-8192/tool\n\
backend:  uv\n\
auth:     auto-switch to mike-clark-8192 (process-scoped; active gh account unchanged)\n\
command:  uv tool install git+https://github.com/mike-clark-8192/tool.git --force\n"
    );
    assert!(!stdout(&output).contains("token"));
    assert!(!stdout(&output).contains("Authorization"));
    assert!(!harness.was_invoked("uv"));
}

#[test]
fn unmatched_python_owners_do_not_auto_switch() {
    for backend in ["--uv", "--pipx"] {
        let program = backend.trim_start_matches("--");
        let harness = Harness::new(&[program]);
        let output = harness.run(&["install", backend, "cli/cli"]);
        assert!(output.status.success(), "{}", stderr(&output));
        let invocation = harness.invocation(program);
        assert!(!invocation.uv_no_github_fast_path);
        assert!(!invocation.git_auth_header_present);
    }
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
