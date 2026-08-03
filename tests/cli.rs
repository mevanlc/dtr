#![cfg(unix)]

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

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

    fn install_all_file(&self) -> PathBuf {
        self.config.join("install-all.toml")
    }

    fn write_install_all(&self, text: &str) {
        fs::create_dir_all(&self.config).unwrap();
        fs::write(self.install_all_file(), text).unwrap();
    }

    fn local_repository(&self, name: &str, files: &[&str], directories: &[&str]) -> String {
        let repository = self.work.join(name);
        fs::create_dir_all(&repository).unwrap();
        for file in files {
            fs::write(repository.join(file), "synthetic manifest\n").unwrap();
        }
        for directory in directories {
            fs::create_dir_all(repository.join(directory)).unwrap();
        }
        format!("./{name}")
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

if [ "${0##*/}" = git ] && [ "${1-}" = -C ] && [ "${3-}" = rev-parse ] && [ "${4-}" = --show-toplevel ]; then
  log="${DTR_TEST_LOG_DIR}/git-worktree-root"
  : > "$log"
  printf 'cwd=<%s>\n' "$PWD" >> "$log"
  for arg in "$@"; do
    printf 'arg=<%s>\n' "$arg" >> "$log"
  done
  if [ -n "${DTR_TEST_GIT_TOPLEVEL-}" ]; then
    printf '%s\n' "$DTR_TEST_GIT_TOPLEVEL"
    exit 0
  fi
  exit 128
fi

if [ "${0##*/}" = gh ] && [ "${1-}" = api ] && [ "${2-}" = user ]; then
  printf '%s\n' "${DTR_TEST_GH_OWNER}"
  exit 0
fi

if [ "${0##*/}" = gh ] && [ "${1-}" = api ] && [ "${2#repos/}" != "${2}" ]; then
  log="${DTR_TEST_LOG_DIR}/gh-api-tree"
  : > "$log"
  printf 'cwd=<%s>\n' "$PWD" >> "$log"
  case "${GH_TOKEN-}" in
    token-*) printf 'gh_token_account=<%s>\n' "${GH_TOKEN#token-}" >> "$log" ;;
    ?*) printf 'gh_token_account=<other>\n' >> "$log" ;;
  esac
  if [ -n "${GITHUB_TOKEN-}" ]; then
    printf 'github_token_present=<yes>\n' >> "$log"
  fi
  for arg in "$@"; do
    printf 'arg=<%s>\n' "$arg" >> "$log"
  done
  if [ -n "${DTR_TEST_GITHUB_TREE_JSON-}" ]; then
    printf '%s\n' "$DTR_TEST_GITHUB_TREE_JSON"
  else
    printf '%s\n' '{"truncated":false,"tree":[]}'
  fi
  exit "${DTR_TEST_GITHUB_TREE_EXIT-0}"
fi

if [ "${0##*/}" = glab ] && [ "${1-}" = api ]; then
  log="${DTR_TEST_LOG_DIR}/glab-api-tree"
  : > "$log"
  printf 'cwd=<%s>\n' "$PWD" >> "$log"
  for arg in "$@"; do
    printf 'arg=<%s>\n' "$arg" >> "$log"
  done
  if [ -n "${DTR_TEST_GITLAB_TREE_JSON-}" ]; then
    printf '%s\n' "$DTR_TEST_GITLAB_TREE_JSON"
  else
    printf '%s\n' '[]'
  fi
  exit "${DTR_TEST_GITLAB_TREE_EXIT-0}"
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

if [ "${0##*/}" = git ] && [ "${1-}" = clone ] && [ -n "${DTR_TEST_GIT_TREE_MARKER-}" ]; then
  log="${DTR_TEST_LOG_DIR}/git-inspection-clone"
  : > "$log"
  printf 'cwd=<%s>\n' "$PWD" >> "$log"
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
  if [ -n "${GH_TOKEN-}" ]; then
    printf 'gh_token_account=<other>\n' >> "$log"
  fi
  if [ -n "${GITHUB_TOKEN-}" ]; then
    printf 'github_token_present=<yes>\n' >> "$log"
  fi
  destination=
  for arg in "$@"; do
    printf 'arg=<%s>\n' "$arg" >> "$log"
    destination=$arg
  done
  mkdir -p "$destination"
  exit "${DTR_TEST_GIT_CLONE_EXIT-0}"
fi

if [ "${0##*/}" = git ] && [ "${1-}" = -C ] && [ -n "${DTR_TEST_GIT_TREE_MARKER-}" ]; then
  log="${DTR_TEST_LOG_DIR}/git-inspection-tree"
  : > "$log"
  printf 'cwd=<%s>\n' "$PWD" >> "$log"
  for arg in "$@"; do
    printf 'arg=<%s>\n' "$arg" >> "$log"
  done
  printf '100644 blob abc\t%s\0' "$DTR_TEST_GIT_TREE_MARKER"
  exit 0
fi

if [ "${0##*/}" = go ] && [ "${1-}" = list ]; then
  log="${DTR_TEST_LOG_DIR}/go-list"
  : > "$log"
  printf 'cwd=<%s>\n' "$PWD" >> "$log"
  for arg in "$@"; do
    printf 'arg=<%s>\n' "$arg" >> "$log"
  done
  printf '%s\n' "${DTR_TEST_GO_LIST_OUTPUT-}"
  exit "${DTR_TEST_GO_LIST_EXIT-0}"
fi

if [ "${0##*/}" = go ] && [ "${1-}" = env ]; then
  log="${DTR_TEST_LOG_DIR}/go-env"
  : > "$log"
  printf 'cwd=<%s>\n' "$PWD" >> "$log"
  for arg in "$@"; do
    printf 'arg=<%s>\n' "$arg" >> "$log"
  done
  printf '%s\n%s\n' "${DTR_TEST_GO_GOBIN-}" "${DTR_TEST_GO_GOPATH-}"
  exit "${DTR_TEST_GO_ENV_EXIT-0}"
fi

if [ -n "${DTR_TEST_PARALLEL_BARRIER-}" ]; then
  parallel_log="${DTR_TEST_LOG_DIR}/parallel-starts"
  printf 'start\n' >> "$parallel_log"
  attempts=0
  while :; do
    started=0
    while IFS= read -r marker; do
      started=$((started + 1))
    done < "$parallel_log"
    if [ "$started" -ge "$DTR_TEST_PARALLEL_BARRIER" ]; then
      break
    fi
    attempts=$((attempts + 1))
    if [ "$attempts" -ge 200 ]; then
      exit 98
    fi
    /bin/sleep 0.01
  done
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
if [ -n "${DTR_TEST_CHILD_STDERR-}" ]; then
  printf '%s\n' "$DTR_TEST_CHILD_STDERR" >&2
fi
if [ "${DTR_TEST_INTERRUPT_PROGRAM-}" = "${0##*/}" ]; then
  printf '%s\n' "$$" > "${DTR_TEST_LOG_DIR}/active-child-pid"
  : > "${DTR_TEST_LOG_DIR}/signal-ready"
  while [ ! -e "${DTR_TEST_LOG_DIR}/release-running-job" ]; do
    /bin/sleep 0.01
  done
  if [ -n "${DTR_TEST_POST_INTERRUPT_STDERR-}" ]; then
    printf '%s\n' "$DTR_TEST_POST_INTERRUPT_STDERR" >&2
  fi
fi
exit "${DTR_TEST_EXIT-0}"
"#;
    fs::write(path, script).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

fn write_executable(path: &Path) {
    fs::write(path, "#!/bin/sh\n").unwrap();
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
fn config_set_get_list_and_unset_round_trip_narration() {
    let harness = Harness::new(&[]);
    let set = harness.run(&["config", "set", "narration", "false"]);
    assert!(set.status.success(), "{}", stderr(&set));
    assert_eq!(
        fs::read_to_string(harness.config_file()).unwrap(),
        "narration = false\n"
    );

    let get = harness.run(&["config", "get", "narration"]);
    assert!(get.status.success(), "{}", stderr(&get));
    assert_eq!(stdout(&get), "false\n");

    let list = harness.run(&["config", "list"]);
    assert!(list.status.success(), "{}", stderr(&list));
    assert_eq!(stdout(&list), "narration=false\n");

    let invalid = harness.run(&["config", "set", "narration", "yes"]);
    assert_eq!(invalid.status.code(), Some(2));
    assert!(stderr(&invalid).contains("narration must be true or false"));

    let unset = harness.run(&["config", "unset", "narration"]);
    assert!(unset.status.success(), "{}", stderr(&unset));
    let get = harness.run(&["config", "get", "narration"]);
    assert_eq!(get.status.code(), Some(2));
    assert!(stderr(&get).contains("is not set"));
}

#[test]
fn config_rejects_unknown_keys_and_invalid_account_lists() {
    let harness = Harness::new(&[]);
    let unknown = harness.run(&["config", "set", "github.auth.surprise", "mevanlc"]);
    assert_eq!(unknown.status.code(), Some(2));
    let error = stderr(&unknown);
    assert!(error.contains("unknown configuration key"));
    assert!(error.contains("available keys: github.auth.auto_switch"));

    for value in ["", "mevanlc,", "not an account"] {
        let output = harness.run(&["config", "set", "github.auth.auto_switch", value]);
        assert_eq!(output.status.code(), Some(2), "{value:?}");
    }
    assert!(!harness.config_file().exists());
}

#[test]
fn config_long_help_documents_available_keys() {
    let harness = Harness::new(&[]);
    let long = harness.run(&["config", "--help"]);
    assert!(long.status.success(), "{}", stderr(&long));
    let long_help = stdout(&long);
    assert!(
        long_help.contains("Available configuration keys:"),
        "{long_help}"
    );
    assert!(long_help.contains("github.auth.auto_switch"), "{long_help}");
    assert!(
        long_help.contains("Comma-separated GitHub CLI account names"),
        "{long_help}"
    );
    assert!(long_help.contains("list   List configured values"));

    let short = harness.run(&["config", "-h"]);
    assert!(short.status.success(), "{}", stderr(&short));
    assert!(
        !stdout(&short).contains("Available configuration keys:"),
        "short help should remain compact"
    );
}

#[test]
fn config_list_prints_configured_values_or_names() {
    let harness = Harness::new(&[]);
    let empty = harness.run(&["config", "list"]);
    assert!(empty.status.success(), "{}", stderr(&empty));
    assert_eq!(stdout(&empty), "");
    assert!(!harness.config_file().exists());

    let set = harness.run(&[
        "config",
        "set",
        "github.auth.auto_switch",
        "mevanlc,mike-clark-8192",
    ]);
    assert!(set.status.success(), "{}", stderr(&set));

    let values = harness.run(&["config", "list"]);
    assert!(values.status.success(), "{}", stderr(&values));
    assert_eq!(
        stdout(&values),
        "github.auth.auto_switch=mevanlc,mike-clark-8192\n"
    );

    let names = harness.run(&["config", "list", "--name-only"]);
    assert!(names.status.success(), "{}", stderr(&names));
    assert_eq!(stdout(&names), "github.auth.auto_switch\n");
}

#[test]
fn install_all_runs_configured_backends_and_forwards_cargo_features() {
    let harness = Harness::new(&["cargo", "go"]);
    let cargo_repo = harness.work.join("ripgrep");
    let go_repo = harness.work.join("gdu/cmd/gdu");
    fs::create_dir_all(&cargo_repo).unwrap();
    fs::create_dir_all(&go_repo).unwrap();
    harness.write_install_all(&format!(
        r#"
            [[install]]
            repospec = {cargo_repo:?}
            tool = "cargo"
            args = ["--force", "--features", "pcre2"]

            [[install]]
            repospec = {go_repo:?}
            tool = "go"
        "#,
        cargo_repo = cargo_repo.display().to_string(),
        go_repo = go_repo.display().to_string(),
    ));

    let output = harness.run(&["ia"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("cargo").args,
        [
            "install",
            "--path",
            cargo_repo.to_str().unwrap(),
            "--force",
            "--features",
            "pcre2",
        ]
    );
    let go = harness.invocation("go");
    assert_eq!(go.args, ["install", "./..."]);
    assert_eq!(
        fs::canonicalize(go.cwd).unwrap(),
        fs::canonicalize(go_repo).unwrap()
    );
}

#[test]
fn install_all_warns_and_continues_after_an_entry_cannot_be_planned() {
    let harness = Harness::new(&["cargo"]);
    let cargo_repo = harness.work.join("working-tool");
    fs::create_dir_all(&cargo_repo).unwrap();
    harness.write_install_all(&format!(
        r#"
            [[install]]
            repospec = "./missing"

            [[install]]
            repospec = {cargo_repo:?}
            tool = "cargo"
        "#,
        cargo_repo = cargo_repo.display().to_string(),
    ));

    let output = harness.run(&["install-all"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("warning: install-all entry 1 (\"./missing\")"),
        "{}",
        stderr(&output)
    );
    assert_eq!(
        harness.invocation("cargo").args,
        ["install", "--path", cargo_repo.to_str().unwrap()]
    );
}

#[test]
fn install_all_explain_prints_every_plan_without_running_installers() {
    let harness = Harness::new(&["cargo"]);
    let cargo_repo = harness.work.join("ripgrep");
    fs::create_dir_all(&cargo_repo).unwrap();
    harness.write_install_all(&format!(
        r#"
            [[install]]
            repospec = {cargo_repo:?}
            tool = "cargo"
            args = ["--force", "--features", "pcre2"]
        "#,
        cargo_repo = cargo_repo.display().to_string(),
    ));

    let output = harness.run(&["--explain", "ia", "--jobs", "3"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let output_text = stdout(&output);
    assert!(output_text.starts_with("jobs: 3\n\n"), "{output_text}");
    assert!(
        output_text.contains("install-all entry 1:"),
        "{output_text}"
    );
    assert!(
        output_text.contains(&format!(
            "command:  cargo install --path {} --force --features pcre2",
            cargo_repo.display()
        )),
        "{output_text}"
    );
    assert!(!harness.was_invoked("cargo"));
}

#[test]
fn install_all_jobs_runs_multiple_installers_concurrently() {
    let harness = Harness::new(&["cargo"]);
    harness.write_install_all(
        r#"
            [[install]]
            repospec = "./one"
            tool = "cargo"

            [[install]]
            repospec = "./two"
            tool = "cargo"
        "#,
    );

    let output = harness
        .command(&["ia", "-j", "2"])
        .env("DTR_TEST_PARALLEL_BARRIER", "2")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        fs::read_to_string(harness.log.join("parallel-starts")).unwrap(),
        "start\nstart\n"
    );
}

#[test]
fn install_all_replays_narration_after_all_native_child_output() {
    let harness = Harness::new(&["cargo"]);
    harness.write_install_all(
        r#"
            [[install]]
            repospec = "./one"
            tool = "cargo"

            [[install]]
            repospec = "./two"
            tool = "cargo"
        "#,
    );

    let output = harness
        .command(&["ia", "--jobs", "2"])
        .env("DTR_TEST_CHILD_STDERR", "native child output")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let stderr = stderr(&output);
    assert_eq!(
        stderr.matches("native child output\n").count(),
        2,
        "{stderr}"
    );
    assert_eq!(
        stderr.matches("→ cargo install --path ./one\n").count(),
        2,
        "{stderr}"
    );
    assert_eq!(
        stderr.matches("→ cargo install --path ./two\n").count(),
        2,
        "{stderr}"
    );
    assert!(
        stderr.ends_with(
            "→ cargo install --path ./one\n\
             → cargo install --path ./two\n"
        ),
        "{stderr}"
    );
    assert!(
        stderr.rfind("native child output").unwrap()
            < stderr.rfind("→ cargo install --path ./one").unwrap(),
        "{stderr}"
    );
}

#[test]
fn install_all_replays_path_warnings_when_narration_is_disabled() {
    let harness = Harness::new(&["go"]);
    let install_directory = harness.work.join("go-bin");
    fs::create_dir(&install_directory).unwrap();
    write_executable(&install_directory.join("tool"));
    write_executable(&harness.bin.join("tool"));
    let path = env::join_paths([harness.bin.as_path(), install_directory.as_path()]).unwrap();
    harness.write_install_all(
        r#"
            [[install]]
            repospec = "owner/tool"
            tool = "go"
        "#,
    );

    let output = harness
        .command(&["--no-narration", "ia"])
        .env("PATH", path)
        .env("DTR_TEST_GO_GOBIN", &install_directory)
        .env("DTR_TEST_CHILD_STDERR", "native child output")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let warning = format!(
        "warning: 'tool' is shadowed by {} (earlier on PATH)",
        harness.bin.join("tool").display()
    );
    let stderr = stderr(&output);
    assert_eq!(stderr.matches(&warning).count(), 2, "{stderr}");
    assert!(!stderr.contains("→ go install"), "{stderr}");
    assert!(stderr.ends_with(&format!("{warning}\n")), "{stderr}");
}

#[test]
fn install_all_ctrl_c_allows_started_jobs_to_finish_then_replays_and_exits_130() {
    let harness = Harness::new(&["cargo", "npm", "uv"]);
    harness.write_install_all(
        r#"
            [[install]]
            repospec = "./finished"
            tool = "cargo"

            [[install]]
            repospec = "./finishing"
            tool = "npm"

            [[install]]
            repospec = "./not-started"
            tool = "uv"
        "#,
    );

    let mut command = harness.command(&["ia", "--jobs", "1"]);
    command
        .env("DTR_TEST_CHILD_STDERR", "native child output")
        .env(
            "DTR_TEST_POST_INTERRUPT_STDERR",
            "post-interrupt installation chatter",
        )
        .env("DTR_TEST_INTERRUPT_PROGRAM", "npm")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let child = command.spawn().unwrap();
    let process_group = i32::try_from(child.id()).unwrap();
    let ready = harness.log.join("signal-ready");
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !ready.exists() {
        unsafe {
            libc::kill(-process_group, libc::SIGKILL);
        }
        let output = child.wait_with_output().unwrap();
        panic!(
            "timed out waiting for interrupt target: {}",
            stderr(&output)
        );
    }

    assert_eq!(unsafe { libc::kill(-process_group, libc::SIGINT) }, 0);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(unsafe { libc::kill(-process_group, 0) }, 0);
    assert!(!harness.was_invoked("uv"));
    fs::write(harness.log.join("release-running-job"), []).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(130), "{}", stderr(&output));
    assert!(!harness.was_invoked("uv"));
    let stderr = stderr(&output);
    assert_eq!(
        stderr
            .matches("→ cargo install --path ./finished\n")
            .count(),
        2,
        "{stderr}"
    );
    assert_eq!(
        stderr
            .matches("→ npm install --global -- ./finishing\n")
            .count(),
        2,
        "{stderr}"
    );
    assert!(
        stderr.ends_with(
            "→ cargo install --path ./finished\n\
             → npm install --global -- ./finishing\n"
        ),
        "{stderr}"
    );
    assert_eq!(
        stderr
            .matches("post-interrupt installation chatter\n")
            .count(),
        1,
        "{stderr}"
    );
    assert!(
        stderr.find("post-interrupt installation chatter").unwrap()
            < stderr.rfind("→ cargo install --path ./finished").unwrap(),
        "{stderr}"
    );
    assert!(!stderr.contains("exited with status"), "{stderr}");
}

#[test]
fn install_all_second_ctrl_c_terminates_active_child_groups_and_exits_130() {
    let harness = Harness::new(&["npm"]);
    harness.write_install_all(
        r#"
            [[install]]
            repospec = "./running"
            tool = "npm"
        "#,
    );

    let mut command = harness.command(&["ia", "--jobs", "1"]);
    command
        .env("DTR_TEST_INTERRUPT_PROGRAM", "npm")
        .env("DTR_TEST_CHILD_STDERR", "native child output")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let child = command.spawn().unwrap();
    let dtr_process_group = i32::try_from(child.id()).unwrap();
    let ready = harness.log.join("signal-ready");
    for _ in 0..500 {
        if ready.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !ready.exists() {
        unsafe {
            libc::kill(-dtr_process_group, libc::SIGKILL);
        }
        let output = child.wait_with_output().unwrap();
        panic!(
            "timed out waiting for interrupt target: {}",
            stderr(&output)
        );
    }
    let active_child_group: i32 = fs::read_to_string(harness.log.join("active-child-pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    assert_eq!(unsafe { libc::kill(-dtr_process_group, libc::SIGINT) }, 0);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(unsafe { libc::kill(-dtr_process_group, 0) }, 0);
    assert_eq!(unsafe { libc::kill(-active_child_group, 0) }, 0);
    assert_eq!(unsafe { libc::kill(-dtr_process_group, libc::SIGINT) }, 0);

    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || sender.send(child.wait_with_output()).unwrap());
    let output = match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(output) => output.unwrap(),
        Err(error) => {
            unsafe {
                libc::kill(-dtr_process_group, libc::SIGKILL);
                libc::kill(-active_child_group, libc::SIGKILL);
            }
            panic!("timed out waiting for forced Ctrl-C exit: {error}");
        }
    };
    assert_eq!(output.status.code(), Some(130), "{}", stderr(&output));
    let stderr = stderr(&output);
    assert_eq!(
        stderr
            .matches("→ npm install --global -- ./running\n")
            .count(),
        2,
        "{stderr}"
    );
    assert!(
        stderr.ends_with("→ npm install --global -- ./running\n"),
        "{stderr}"
    );
}

#[test]
fn install_all_jobs_help_and_zero_validation_are_explicit() {
    let harness = Harness::new(&[]);
    let help = harness.run(&["ia", "--help"]);
    assert!(help.status.success(), "{}", stderr(&help));
    let help_text = stdout(&help);
    assert!(help_text.contains("-j, --jobs <n|auto>"), "{help_text}");
    assert!(help_text.contains("[default: auto]"), "{help_text}");
    assert!(
        help_text.contains("ceil(available CPU cores / 2)"),
        "{help_text}"
    );
    assert!(help_text.contains("--list"), "{help_text}");
    assert!(help_text.contains("--edit"), "{help_text}");
    assert!(help_text.contains("--file <FILE>"), "{help_text}");

    let zero = harness.run(&["ia", "--jobs", "0"]);
    assert_eq!(zero.status.code(), Some(2));
    assert!(stderr(&zero).contains("expected 'auto' or a positive integer"));
}

#[test]
fn install_add_tracks_a_successful_install_once_and_preserves_comments() {
    let harness = Harness::new(&["cargo"]);
    let repo = harness.local_repository("rip grep", &["Cargo.toml"], &[]);
    let original = r#"# keep this comment
[[install]]
repospec = "owner/tool"
tool = "go"
"#;
    harness.write_install_all(original);

    let explain = harness.run(&[
        "--explain",
        "install",
        "--add",
        &repo,
        "--",
        "--force",
        "--features",
        "pcre2",
    ]);
    assert!(explain.status.success(), "{}", stderr(&explain));
    assert!(stdout(&explain).contains("track:    "));
    assert_eq!(
        fs::read_to_string(harness.install_all_file()).unwrap(),
        original
    );
    assert!(!harness.was_invoked("cargo"));

    let arguments = [
        "install",
        "--add",
        &repo,
        "--",
        "--force",
        "--features",
        "pcre2",
    ];
    let added = harness.run(&arguments);
    assert!(added.status.success(), "{}", stderr(&added));
    assert!(stderr(&added).contains("tracked:"), "{}", stderr(&added));
    let config = fs::read_to_string(harness.install_all_file()).unwrap();
    assert!(config.starts_with(original));
    assert_eq!(config.matches("[[install]]").count(), 2);
    assert!(config.contains("tool = \"cargo\""), "{config}");
    assert!(
        config.contains("args = [\"--force\", \"--features\", \"pcre2\"]"),
        "{config}"
    );
    assert!(!config.contains("add ="), "{config}");

    let duplicate = harness.run(&arguments);
    assert!(duplicate.status.success(), "{}", stderr(&duplicate));
    assert!(
        stderr(&duplicate).contains("already tracked:"),
        "{}",
        stderr(&duplicate)
    );
    assert_eq!(
        fs::read_to_string(harness.install_all_file()).unwrap(),
        config
    );
}

#[test]
fn install_add_does_not_track_a_failed_install() {
    let harness = Harness::new(&["cargo"]);
    let repo = harness.local_repository("broken", &["Cargo.toml"], &[]);
    let output = harness
        .command(&["install", "--add", &repo])
        .env("DTR_TEST_EXIT", "17")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(17));
    assert!(!harness.install_all_file().exists());
}

#[test]
fn install_all_list_uses_the_alternate_config_and_prints_reusable_commands() {
    let harness = Harness::new(&[]);
    let alternate = harness.work.join("alternate/install-all.toml");
    fs::create_dir_all(alternate.parent().unwrap()).unwrap();
    fs::write(
        &alternate,
        r#"
            [[install]]
            repospec = "~/p/my/ripgrep"
            tool = "cargo"
            args = ["--force", "--features", "pcre2"]

            [[install]]
            repospec = "owner/tool"
            tool = "go"
            no_latest = true

            [[install]]
            repospec = "./path with spaces"
        "#,
    )
    .unwrap();

    let output = harness.run(&["ia", "--list", "--file", alternate.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));
    let home = home::home_dir().unwrap();
    assert_eq!(
        stdout(&output),
        format!(
            "dtr install --tool cargo {}/p/my/ripgrep -- --force --features pcre2\n\
             dtr install --tool go --no-latest owner/tool\n\
             dtr install './path with spaces'\n",
            home.display()
        )
    );
}

#[test]
fn install_all_execution_uses_the_alternate_config() {
    let harness = Harness::new(&["cargo"]);
    let alternate = harness.work.join("alternate.toml");
    fs::write(
        &alternate,
        "[[install]]\nrepospec = \"./alternate tool\"\ntool = \"cargo\"\n",
    )
    .unwrap();

    let output = harness.run(&["ia", "--file", alternate.to_str().unwrap(), "--jobs", "1"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("cargo").args,
        ["install", "--path", "./alternate tool"]
    );
    assert!(!harness.install_all_file().exists());
}

#[test]
fn install_all_edit_honors_editor_precedence_and_arguments() {
    let harness = Harness::new(&["dtr-editor", "visual-editor", "plain-editor"]);
    let alternate = harness.work.join("nested/install-all.toml");
    let output = harness
        .command(&["ia", "--edit", "--file", alternate.to_str().unwrap()])
        .env("DTR_EDITOR", "dtr-editor --wait")
        .env("VISUAL", "visual-editor")
        .env("EDITOR", "plain-editor")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("dtr-editor").args,
        ["--wait", alternate.to_str().unwrap()]
    );
    assert!(!harness.was_invoked("visual-editor"));
    assert!(!harness.was_invoked("plain-editor"));
    assert!(alternate.parent().unwrap().is_dir());
}

#[test]
fn install_all_edit_explain_is_read_only_and_falls_back_to_vim_before_vi() {
    let explain = Harness::new(&["vim", "vi"]);
    let alternate = explain.work.join("missing/install-all.toml");
    let output = explain.run(&[
        "--explain",
        "ia",
        "--edit",
        "--file",
        alternate.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!("command:  vim {}\n", alternate.display())
    );
    assert!(!explain.was_invoked("vim"));
    assert!(!explain.was_invoked("vi"));
    assert!(!alternate.parent().unwrap().exists());

    let execute = Harness::new(&["vim", "vi"]);
    let output = execute.run(&["ia", "--edit"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(execute.was_invoked("vim"));
    assert!(!execute.was_invoked("vi"));
}

#[test]
fn install_all_edit_prefers_visual_over_editor() {
    let harness = Harness::new(&["visual-editor", "plain-editor"]);
    let output = harness
        .command(&["ia", "--edit"])
        .env("VISUAL", "visual-editor --foreground")
        .env("EDITOR", "plain-editor")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("visual-editor").args,
        ["--foreground", harness.install_all_file().to_str().unwrap()]
    );
    assert!(!harness.was_invoked("plain-editor"));
}

#[test]
fn install_all_requires_a_valid_dedicated_configuration_file() {
    let harness = Harness::new(&["cargo"]);
    let missing = harness.run(&["ia"]);
    assert_eq!(missing.status.code(), Some(2));
    assert!(
        stderr(&missing).contains("could not read install-all configuration"),
        "{}",
        stderr(&missing)
    );

    harness.write_install_all(
        r#"
            [[install]]
            repospec = "./tool"
            feature = "pcre2"
        "#,
    );
    let malformed = harness.run(&["ia"]);
    assert_eq!(malformed.status.code(), Some(2));
    assert!(stderr(&malformed).contains("unknown field `feature`"));
    assert!(!harness.was_invoked("cargo"));
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
fn clone_narration_echoes_the_command_and_prints_only_the_absolute_target() {
    let harness = Harness::new(&["git", "gh"]);
    let output = harness.run(&["clone", "owner/repo", "destination"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        format!(
            "{}\n",
            harness
                .work
                .canonicalize()
                .unwrap()
                .join("destination")
                .display()
        )
    );
    assert_eq!(stderr(&output), "→ gh repo clone owner/repo destination\n");
}

#[test]
fn narration_config_and_cli_overrides_have_the_expected_precedence() {
    let harness = Harness::new(&["git", "gh"]);
    assert!(
        harness
            .run(&["config", "set", "narration", "false"])
            .status
            .success()
    );

    let configured_off = harness.run(&["clone", "owner/repo", "configured-off"]);
    assert!(
        configured_off.status.success(),
        "{}",
        stderr(&configured_off)
    );
    assert_eq!(stdout(&configured_off), "");
    assert_eq!(stderr(&configured_off), "");

    let opted_in = harness.run(&["--narration", "clone", "owner/repo", "opted-in"]);
    assert!(opted_in.status.success(), "{}", stderr(&opted_in));
    assert_eq!(
        stdout(&opted_in),
        format!(
            "{}\n",
            harness
                .work
                .canonicalize()
                .unwrap()
                .join("opted-in")
                .display()
        )
    );
    assert!(stderr(&opted_in).starts_with("→ gh repo clone"));

    let fresh = Harness::new(&["git", "gh"]);
    let opted_out = fresh.run(&["--no-narration", "clone", "owner/repo", "opted-out"]);
    assert!(opted_out.status.success(), "{}", stderr(&opted_out));
    assert_eq!(stdout(&opted_out), "");
    assert_eq!(stderr(&opted_out), "");
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
        ["clone", "--", "https://github.com/mevanlc/foo.git"]
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
fn forge_clone_strips_repository_url_fragments() {
    let github = Harness::new(&["git", "gh"]);
    let output = github.run(&["clone", "https://github.com/owner/repo.git/#installation"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        github.invocation("gh").args,
        ["repo", "clone", "https://github.com/owner/repo.git"]
    );

    let gitlab = Harness::new(&["git", "glab"]);
    let output = gitlab.run(&["clone", "https://gitlab.com/group/repo#readme"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        gitlab.invocation("glab").args,
        ["repo", "clone", "https://gitlab.com/group/repo"]
    );
}

#[test]
fn github_upstream_remote_name_is_forwarded_to_gh() {
    let short = Harness::new(&["git", "gh"]);
    let output = short.run(&["clone", "--depth", "1", "owner/repo", "-U", "parent"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        short.invocation("gh").args,
        [
            "repo",
            "clone",
            "--upstream-remote-name",
            "parent",
            "owner/repo",
            "--",
            "--depth",
            "1",
        ]
    );

    let long = Harness::new(&["git", "gh"]);
    let output = long.run(&["clone", "--upstream-remote-name=source", "owner/repo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        long.invocation("gh").args,
        [
            "repo",
            "clone",
            "--upstream-remote-name",
            "source",
            "owner/repo",
        ]
    );
}

#[test]
fn upstream_remote_name_requires_github_and_gh() {
    let gitlab = Harness::new(&["git", "gh", "glab"]);
    let output = gitlab.run(&["clone", "-U", "parent", "https://gitlab.com/group/repo"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("applies only to recognized GitHub repositories"));
    assert!(!gitlab.was_invoked("gh"));
    assert!(!gitlab.was_invoked("glab"));
    assert!(!gitlab.was_invoked("git"));

    let missing_gh = Harness::new(&["git"]);
    let output = missing_gh.run(&["clone", "-U", "parent", "owner/repo"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("gh is required for using -U/--upstream-remote-name"));
    assert!(!missing_gh.was_invoked("git"));
}

#[test]
fn github_ssh_forms_use_gh_and_support_derived_directories() {
    for remote in [
        "git@github.com:owner/repo.git",
        "ssh://git@github.com/owner/repo.git",
    ] {
        let harness = Harness::new(&["git", "gh"]);
        let output = harness.run(&["clone", "-D", remote]);
        assert!(output.status.success(), "{remote}: {}", stderr(&output));
        assert_eq!(
            harness.invocation("gh").args,
            ["repo", "clone", remote, "owner/repo"],
            "{remote}"
        );
        assert!(harness.work.join("owner").is_dir(), "{remote}");
    }
}

#[test]
fn github_ssh_clone_preserves_transport_on_git_fallback() {
    let harness = Harness::new(&["git"]);
    let remote = "git@github.com:owner/repo.git";
    let output = harness.run(&["clone", remote]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(harness.invocation("git").args, ["clone", "--", remote]);
}

#[test]
fn github_ssh_owner_match_uses_process_scoped_account() {
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

    let output = harness.run(&["clone", "git@github.com:mike-clark-8192/private.git"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let clone = harness.invocation("gh");
    assert_eq!(
        clone.args,
        [
            "repo",
            "clone",
            "https://github.com/mike-clark-8192/private.git",
        ]
    );
    assert_eq!(clone.gh_token_account.as_deref(), Some("mike-clark-8192"));
}

#[test]
fn github_clone_falls_back_to_git_when_gh_is_absent() {
    let harness = Harness::new(&["git"]);
    let output = harness.run(&["clone", "owner/repo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("git").args,
        ["clone", "--", "https://github.com/owner/repo.git"]
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
            ["clone", "--", repository, "destination"]
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
        ["clone", "--", "https://gitlab.com/group/repo.git"]
    );
}

#[test]
fn dash_leading_scp_remote_is_forced_after_option_terminator_for_git() {
    // Regression (F2): a dash-leading scp-like repospec must reach git after a
    // `--` option terminator, never in git's option position. Two `--` are
    // needed at the shell: clap (trailing_var_arg) consumes the first, and
    // parse_clone_args consumes the second to mark `-x:y` as a positional.
    let harness = Harness::new(&["git"]);
    let output = harness.run(&["clone", "--", "--", "-x:y"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let clone = harness.invocation("git");
    assert_eq!(&clone.args[..3], ["clone", "--", "-x:y"]);
}

#[test]
fn clone_multibyte_short_option_errors_cleanly_without_panicking() {
    // Regression (F1): `-é` has a multi-byte second codepoint; classifying it as
    // a short option must not slice inside the codepoint and panic. A clean
    // exit code 2 (not a 101 panic) is the signal the fix holds.
    let harness = Harness::new(&["git"]);
    let output = harness.run(&["clone", "-é"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("unknown option"),
        "{}",
        stderr(&output)
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
fn auto_local_install_selects_go_cargo_and_npm_from_root_manifests() {
    let go = Harness::new(&["go"]);
    let go_repo = go.local_repository("go-tool", &["go.mod"], &[]);
    let output = go.run(&["install", &go_repo]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(go.invocation("go").args, ["install", "./..."]);
    assert_eq!(
        go.invocation("go").cwd,
        go.work.join("go-tool").canonicalize().unwrap()
    );

    let cargo = Harness::new(&["cargo"]);
    let cargo_repo = cargo.local_repository("cargo-tool", &["Cargo.toml"], &[]);
    let output = cargo.run(&["install", &cargo_repo]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        cargo.invocation("cargo").args,
        ["install", "--path", "./cargo-tool"]
    );

    let npm = Harness::new(&["npm"]);
    let npm_repo = npm.local_repository("npm-tool", &["package.json"], &[]);
    let output = npm.run(&["install", &npm_repo]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        npm.invocation("npm").args,
        ["install", "--global", "--", "./npm-tool"]
    );
}

#[test]
fn auto_local_go_subdirectory_uses_an_ancestor_module_and_keeps_its_working_directory() {
    let harness = Harness::new(&["git", "go"]);
    let repository = harness.work.join("go-tool");
    let command_directory = repository.join("cmd/tool");
    fs::create_dir_all(&command_directory).unwrap();
    fs::write(repository.join("go.mod"), "module example.com/tool\n").unwrap();
    fs::write(command_directory.join("entrypoint.go"), "package main\n").unwrap();
    fs::write(command_directory.join("README.md"), "tool docs\n").unwrap();

    let output = harness
        .command(&["install", "./go-tool/cmd/tool"])
        .env("DTR_TEST_GIT_TOPLEVEL", &repository)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let go = harness.invocation("go");
    assert_eq!(go.args, ["install", "./..."]);
    assert_eq!(go.cwd, command_directory.canonicalize().unwrap());
    assert_eq!(
        harness.invocation("git-worktree-root").args,
        ["-C", "./go-tool/cmd/tool", "rev-parse", "--show-toplevel",]
    );
}

#[test]
fn local_go_ancestor_detection_requires_source_a_module_and_a_git_boundary() {
    for (case, source, root_module, programs, expect_git_probe) in [
        ("test-only", "main_test.go", true, &["git", "go"][..], false),
        ("no-source", "README.md", true, &["git", "go"][..], false),
        ("no-module", "main.go", false, &["git", "go"][..], true),
        ("no-git", "main.go", true, &["go"][..], false),
    ] {
        let harness = Harness::new(programs);
        let repository = harness.work.join(case);
        let command_directory = repository.join("cmd/tool");
        fs::create_dir_all(&command_directory).unwrap();
        fs::write(command_directory.join(source), "package main\n").unwrap();
        if root_module {
            fs::write(repository.join("go.mod"), "module example.com/tool\n").unwrap();
        } else {
            fs::write(harness.work.join("go.mod"), "module outside\n").unwrap();
        }

        let repospec = format!("./{case}/cmd/tool");
        let output = harness
            .command(&["install", &repospec])
            .env("DTR_TEST_GIT_TOPLEVEL", &repository)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{case}: {}", stderr(&output));
        assert!(
            stderr(&output).contains("no supported manifest was found"),
            "{case}: {}",
            stderr(&output)
        );
        assert_eq!(
            harness.was_invoked("git-worktree-root"),
            expect_git_probe,
            "{case}"
        );
        assert!(!harness.was_invoked("go"), "{case}");
    }
}

#[test]
fn omitted_install_repospec_auto_installs_the_current_directory() {
    let harness = Harness::new(&["cargo"]);
    fs::write(harness.work.join("Cargo.toml"), "synthetic manifest\n").unwrap();

    let output = harness.run(&["i"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let invocation = harness.invocation("cargo");
    assert_eq!(
        invocation.cwd.canonicalize().unwrap(),
        harness.work.canonicalize().unwrap()
    );
    assert_eq!(invocation.args, ["install", "--path", "."]);
}

#[test]
fn auto_python_prefers_uv_and_falls_back_to_pipx() {
    let uv = Harness::new(&["uv", "pipx"]);
    let repository = uv.local_repository("python-tool", &["pyproject.toml", "setup.cfg"], &[]);
    let output = uv.run(&["install", &repository]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        uv.invocation("uv").args,
        ["tool", "install", "./python-tool"]
    );
    assert!(!uv.was_invoked("pipx"));

    let pipx = Harness::new(&["pipx"]);
    let repository = pipx.local_repository("legacy-python", &["setup.py"], &[]);
    let output = pipx.run(&["install", &repository]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        pipx.invocation("pipx").args,
        ["install", "--", "./legacy-python"]
    );
}

#[test]
fn auto_declines_missing_and_mixed_root_evidence_before_installing() {
    let harness = Harness::new(&["cargo", "npm", "uv", "pipx", "go"]);
    let mixed = harness.local_repository("mixed", &["Cargo.toml", "package.json"], &[]);
    let output = harness.run(&["install", &mixed]);
    assert_eq!(output.status.code(), Some(2));
    let error = stderr(&output);
    assert!(error.contains("Cargo.toml (cargo)"), "{error}");
    assert!(error.contains("package.json (npm)"), "{error}");
    assert!(error.contains("--tool <go|cargo|uv|pipx|npm>"), "{error}");

    let empty = harness.local_repository("empty", &["README.md"], &["Cargo.toml"]);
    let output = harness.run(&["install", &empty]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("no supported manifest"));
    for program in ["cargo", "npm", "uv", "pipx", "go"] {
        assert!(!harness.was_invoked(program), "{program}");
    }
}

#[test]
fn auto_github_api_detection_selects_backend_in_explain_mode() {
    let harness = Harness::new(&["gh", "cargo"]);
    let output = harness
        .command(&["--explain", "install", "owner/tool"])
        .env(
            "DTR_TEST_GITHUB_TREE_JSON",
            r#"{"truncated":false,"tree":[{"path":"Cargo.toml","type":"blob"}]}"#,
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "repospec: GitHub repository owner/tool\n\
backend:  cargo\n\
command:  cargo install --git https://github.com/owner/tool.git\n"
    );
    assert_eq!(
        harness.invocation("gh-api-tree").args,
        [
            "api",
            "repos/owner/tool/git/trees/HEAD",
            "--hostname",
            "github.com",
        ]
    );
    assert!(!harness.was_invoked("cargo"));
}

#[test]
fn auto_go_query_inspects_the_base_repository_and_preserves_the_query() {
    let harness = Harness::new(&["gh", "go"]);
    let output = harness
        .command(&[
            "--explain",
            "install",
            "https://github.com/yuser/reepo@some-go-stuff",
        ])
        .env(
            "DTR_TEST_GITHUB_TREE_JSON",
            r#"{"truncated":false,"tree":[{"path":"go.mod","type":"blob"}]}"#,
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "repospec: GitHub repository yuser/reepo\n\
backend:  go\n\
command:  go install github.com/yuser/reepo@some-go-stuff\n"
    );
    assert_eq!(
        harness.invocation("gh-api-tree").args,
        [
            "api",
            "repos/yuser/reepo/git/trees/HEAD",
            "--hostname",
            "github.com",
        ]
    );
    assert!(!harness.was_invoked("go"));
}

#[test]
fn auto_github_detection_reuses_process_scoped_account_selection() {
    let harness = Harness::new(&["gh", "cargo"]);
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
        .command(&["install", "mike-clark-8192/tool"])
        .env(
            "DTR_TEST_GITHUB_TREE_JSON",
            r#"{"truncated":false,"tree":[{"path":"Cargo.toml","type":"blob"}]}"#,
        )
        .env("GH_TOKEN", "parent-gh-token")
        .env("GITHUB_TOKEN", "parent-github-token")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let api = harness.invocation("gh-api-tree");
    assert_eq!(api.gh_token_account.as_deref(), Some("mike-clark-8192"));
    assert!(!api.github_token_present);
    let cargo = harness.invocation("cargo");
    assert!(cargo.cargo_git_fetch_cli);
    assert!(cargo.git_auth_header_present);
}

#[test]
fn auto_falls_back_from_forge_api_to_filtered_git_without_checkout() {
    let harness = Harness::new(&["gh", "git", "npm"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc",])
            .status
            .success()
    );
    let output = harness
        .command(&["install", "mevanlc/tool"])
        .env("DTR_TEST_GITHUB_TREE_EXIT", "1")
        .env("DTR_TEST_GIT_TREE_MARKER", "package.json")
        .env("GH_TOKEN", "parent-gh-token")
        .env("GITHUB_TOKEN", "parent-github-token")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let clone = harness.invocation("git-inspection-clone");
    assert_eq!(clone.args[0], "clone");
    for option in [
        "--depth=1",
        "--single-branch",
        "--no-tags",
        "--filter=blob:none",
        "--no-checkout",
    ] {
        assert!(clone.args.contains(&option.to_owned()), "{option:?}");
    }
    assert!(
        clone
            .args
            .contains(&"https://github.com/mevanlc/tool.git".to_owned())
    );
    assert_eq!(clone.git_config_count.as_deref(), Some("2"));
    assert!(clone.git_auth_header_present);
    assert_eq!(clone.gh_token_account, None);
    assert!(!clone.github_token_present);
    assert_eq!(
        harness.invocation("npm").args,
        [
            "install",
            "--global",
            "--",
            "git+https://github.com/mevanlc/tool.git",
        ]
    );
}

#[test]
fn auto_falls_back_when_github_api_tree_is_truncated_or_malformed() {
    for tree_json in [
        r#"{"truncated":true,"tree":[]}"#,
        r#"{"not":"a tree response"}"#,
    ] {
        let harness = Harness::new(&["gh", "git", "cargo"]);
        let output = harness
            .command(&["--explain", "install", "owner/tool"])
            .env("DTR_TEST_GITHUB_TREE_JSON", tree_json)
            .env("DTR_TEST_GIT_TREE_MARKER", "Cargo.toml")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        assert!(harness.was_invoked("gh-api-tree"));
        assert!(harness.was_invoked("git-inspection-clone"));
        assert!(!harness.was_invoked("cargo"));
    }
}

#[test]
fn auto_gitlab_api_detection_uses_the_default_root_tree() {
    let harness = Harness::new(&["glab", "go"]);
    let output = harness
        .command(&["install", "https://gitlab.com/group/subgroup/tool"])
        .env(
            "DTR_TEST_GITLAB_TREE_JSON",
            r#"[{"name":"docs","type":"tree"}][{"name":"go.mod","type":"blob"}]"#,
        )
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("go").args,
        ["install", "gitlab.com/group/subgroup/tool@latest"]
    );
    let glab = harness.invocation("glab-api-tree");
    assert!(
        glab.args.contains(
            &"projects/group%2Fsubgroup%2Ftool/repository/tree?pagination=keyset&per_page=100"
                .to_owned()
        )
    );
    assert!(glab.args.contains(&"--paginate".to_owned()));
}

#[test]
fn explicit_tool_skips_repository_inspection() {
    let harness = Harness::new(&["gh", "git", "cargo"]);
    let output = harness.run(&["install", "--tool", "cargo", "owner/tool"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(harness.was_invoked("cargo"));
    assert!(!harness.was_invoked("gh-api-tree"));
    assert!(!harness.was_invoked("git-inspection-clone"));
}

#[test]
fn rust_local_install_maps_to_cargo_path_and_preserves_native_arguments() {
    let harness = Harness::new(&["cargo"]);
    let output = harness.run(&[
        "install",
        "--tool",
        "cargo",
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
            &["install", "--tool", "cargo", "owner/tool", "--", "--locked"],
            &[
                "install",
                "--git",
                "https://github.com/owner/tool.git",
                "--locked",
            ],
        ),
        (
            &[
                "install",
                "--tool",
                "cargo",
                "http://gitlab.com/group/subgroup/tool",
            ],
            &[
                "install",
                "--git",
                "https://gitlab.com/group/subgroup/tool.git",
            ],
        ),
        (
            &[
                "install",
                "--tool",
                "cargo",
                "ssh://git@example.com/srv/tool.git",
            ],
            &["install", "--git", "ssh://git@example.com/srv/tool.git"],
        ),
        (
            &[
                "install",
                "--tool",
                "cargo",
                "git@example.com:owner/tool.git",
            ],
            &["install", "--git", "ssh://git@example.com/~/owner/tool.git"],
        ),
        (
            &[
                "install",
                "--tool",
                "cargo",
                "git@example.com:/srv/tool.git",
            ],
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
    let output = harness.run(&["install", "--tool", "cargo", "my-tool"]);
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
        let output = harness.run(&["install", "--tool", "cargo", "owner/tool", "--", argument]);
        assert_eq!(output.status.code(), Some(2), "{argument}");
        assert!(stderr(&output).contains("conflicts with dtr's resolved repository"));
        assert!(!harness.was_invoked("cargo"));
    }
}

#[test]
fn rust_rejects_no_latest_and_go_rejects_cargo_arguments() {
    let harness = Harness::new(&["cargo", "go"]);
    let cargo = harness.run(&["install", "--tool", "cargo", "--no-latest", "owner/tool"]);
    assert_eq!(cargo.status.code(), Some(2));
    assert!(stderr(&cargo).contains("applies only to the Go installer"));
    assert!(!harness.was_invoked("cargo"));

    let go = harness.run(&["install", "--tool", "go", "owner/tool", "--", "--locked"]);
    assert_eq!(go.status.code(), Some(2));
    assert!(stderr(&go).contains("Cargo, uv, pipx, and npm installers"));
    assert!(!harness.was_invoked("go"));
}

#[test]
fn missing_cargo_and_cargo_exit_status_are_reported() {
    let missing = Harness::new(&[]);
    let output = missing.run(&["install", "--tool", "cargo", "owner/tool"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("cargo is required"));

    let failing = Harness::new(&["cargo"]);
    let output = failing
        .command(&["install", "--tool", "cargo", "owner/tool"])
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

    let output = harness.run(&["install", "--tool", "cargo", "mike-clark-8192/tool"]);
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
        .command(&["install", "--tool", "cargo", "mevanlc/tool"])
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
        .command(&["install", "--tool", "cargo", "mevanlc/tool"])
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
        "--tool",
        "cargo",
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
        let output = harness.run(&["install", "--tool", "cargo", repository]);
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
        "--tool",
        "uv",
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
        "--tool",
        "pipx",
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
            "git://example.com/owner/tool.git",
            "git+git://example.com/owner/tool.git",
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
        let output = uv.run(&["install", "--tool", "uv", repospec]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(uv.invocation("uv").args, ["tool", "install", source]);

        let pipx = Harness::new(&["pipx"]);
        let output = pipx.run(&["install", "--tool", "pipx", repospec]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(pipx.invocation("pipx").args, ["install", "--", source]);
    }
}

#[test]
fn bare_python_repo_uses_the_active_github_owner() {
    for backend in ["uv", "pipx"] {
        let harness = Harness::new(&[backend, "gh"]);
        let output = harness.run(&["install", "--tool", backend, "my-tool"]);
        assert!(output.status.success(), "{}", stderr(&output));
        let invocation = harness.invocation(backend);
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
        let output = harness.run(&["install", "--tool", "pipx", "owner/tool", "--", argument]);
        assert_eq!(output.status.code(), Some(2), "{argument}");
        assert!(!harness.was_invoked("pipx"));
    }
}

#[test]
fn python_installers_reject_go_options_and_propagate_failures() {
    for backend in ["uv", "pipx"] {
        let harness = Harness::new(&[backend]);
        let rejected = harness.run(&["install", "--tool", backend, "--no-latest", "owner/tool"]);
        assert_eq!(rejected.status.code(), Some(2));
        assert!(stderr(&rejected).contains("only to the Go installer"));
        assert!(!harness.was_invoked(backend));

        let failed = harness
            .command(&["install", "--tool", backend, "owner/tool"])
            .env("DTR_TEST_EXIT", "19")
            .output()
            .unwrap();
        assert_eq!(failed.status.code(), Some(19));

        let missing = Harness::new(&[]);
        let output = missing.run(&["install", "--tool", backend, "owner/tool"]);
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains(&format!("{backend} is required")));
    }
}

#[test]
fn python_install_auto_switches_with_git_http_auth_only() {
    for backend in ["uv", "pipx"] {
        let harness = Harness::new(&[backend, "gh"]);
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
            .command(&["install", "--tool", backend, "mike-clark-8192/tool"])
            .env("GH_TOKEN", "parent-gh-token")
            .env("GITHUB_TOKEN", "parent-github-token")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", stderr(&output));
        let invocation = harness.invocation(backend);
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
        .command(&["install", "--tool", "uv", "mevanlc/tool"])
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
        .command(&["install", "--tool", "uv", "mevanlc/tool"])
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
        "--tool",
        "uv",
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
    for backend in ["uv", "pipx"] {
        let harness = Harness::new(&[backend]);
        let output = harness.run(&["install", "--tool", backend, "cli/cli"]);
        assert!(output.status.success(), "{}", stderr(&output));
        let invocation = harness.invocation(backend);
        assert!(!invocation.uv_no_github_fast_path);
        assert!(!invocation.git_auth_header_present);
    }
}

#[test]
fn npm_local_install_maps_to_global_source_and_preserves_native_options() {
    let harness = Harness::new(&["npm"]);
    let output = harness.run(&[
        "install",
        "--tool",
        "npm",
        "./local repo",
        "--",
        "--prefix=/opt/npm tools",
        "--force",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("npm").args,
        [
            "install",
            "--global",
            "--prefix=/opt/npm tools",
            "--force",
            "--",
            "./local repo",
        ]
    );
}

#[test]
fn npm_remote_repositories_map_to_vcs_package_sources() {
    let cases: &[(&str, &str)] = &[
        ("owner/tool", "git+https://github.com/owner/tool.git"),
        (
            "http://gitlab.com/group/subgroup/tool",
            "git+https://gitlab.com/group/subgroup/tool.git",
        ),
        (
            "https://example.com/git/tool.git",
            "git+https://example.com/git/tool.git",
        ),
        (
            "ssh://git@example.com/srv/tool.git",
            "git+ssh://git@example.com/srv/tool.git",
        ),
        (
            "git://example.com/owner/tool.git",
            "git://example.com/owner/tool.git",
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
        let harness = Harness::new(&["npm"]);
        let output = harness.run(&["install", "--tool", "npm", repospec]);
        assert!(output.status.success(), "{}", stderr(&output));
        assert_eq!(
            harness.invocation("npm").args,
            ["install", "--global", "--", source]
        );
    }
}

#[test]
fn bare_npm_repo_uses_the_active_github_owner() {
    let harness = Harness::new(&["npm", "gh"]);
    let output = harness.run(&["install", "--tool", "npm", "my-tool"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("npm").args,
        [
            "install",
            "--global",
            "--",
            "git+https://github.com/mevanlc/my-tool.git",
        ]
    );
    assert!(!harness.was_invoked("gh-auth-token"));
}

#[test]
fn npm_native_arguments_cannot_add_a_source_or_disable_global_mode() {
    for argument in [
        "another-package",
        "/opt/npm",
        "--",
        "-g",
        "-g=false",
        "--global",
        "--global=false",
        "--no-global",
        "--no-global=true",
    ] {
        let harness = Harness::new(&["npm"]);
        let output = harness.run(&["install", "--tool", "npm", "owner/tool", "--", argument]);
        assert_eq!(output.status.code(), Some(2), "{argument}");
        assert!(!harness.was_invoked("npm"));
    }

    let valid = Harness::new(&["npm"]);
    let output = valid.run(&[
        "install",
        "--tool",
        "npm",
        "owner/tool",
        "--",
        "--global-style",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn npm_rejects_go_options_and_propagates_failures() {
    let harness = Harness::new(&["npm"]);
    let rejected = harness.run(&["install", "--tool", "npm", "--no-latest", "owner/tool"]);
    assert_eq!(rejected.status.code(), Some(2));
    assert!(stderr(&rejected).contains("only to the Go installer"));
    assert!(!harness.was_invoked("npm"));

    let failed = harness
        .command(&["install", "--tool", "npm", "owner/tool"])
        .env("DTR_TEST_EXIT", "23")
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(23));

    let missing = Harness::new(&[]);
    let output = missing.run(&["install", "--tool", "npm", "owner/tool"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("npm is required"));
}

#[test]
fn npm_install_auto_switches_with_git_http_auth_only() {
    let harness = Harness::new(&["npm", "gh"]);
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
        .command(&["install", "--tool", "npm", "mike-clark-8192/tool"])
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "test.existing")
        .env("GIT_CONFIG_VALUE_0", "preserved")
        .env("GH_TOKEN", "parent-gh-token")
        .env("GITHUB_TOKEN", "parent-github-token")
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let npm = harness.invocation("npm");
    assert_eq!(npm.git_config_count.as_deref(), Some("3"));
    assert_eq!(
        npm.git_config_keys,
        [
            "test.existing",
            "http.https://github.com/.extraHeader",
            "http.https://github.com/.extraHeader",
        ]
    );
    assert!(npm.git_auth_header_present);
    assert_eq!(npm.gh_token_account, None);
    assert!(!npm.github_token_present);
    assert!(!npm.uv_no_github_fast_path);
}

#[test]
fn npm_auto_switch_fails_closed_before_npm() {
    let harness = Harness::new(&["npm", "gh"]);
    assert!(
        harness
            .run(&["config", "set", "github.auth.auto_switch", "mevanlc"])
            .status
            .success()
    );
    let output = harness
        .command(&["install", "--tool", "npm", "mevanlc/tool"])
        .env("DTR_TEST_GH_TOKEN_FAIL_ACCOUNT", "mevanlc")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("auto-switch account mevanlc"));
    assert!(!harness.was_invoked("npm"));
}

#[test]
fn npm_auto_switch_explain_is_exact_and_secret_free() {
    let harness = Harness::new(&["npm", "gh"]);
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
        "--tool",
        "npm",
        "mike-clark-8192/tool",
        "--",
        "--force",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        stdout(&output),
        "repospec: GitHub repository mike-clark-8192/tool\n\
backend:  npm\n\
auth:     auto-switch to mike-clark-8192 (process-scoped; active gh account unchanged)\n\
command:  npm install --global --force -- git+https://github.com/mike-clark-8192/tool.git\n"
    );
    assert!(!stdout(&output).contains("token"));
    assert!(!stdout(&output).contains("Authorization"));
    assert!(harness.was_invoked("gh-auth-token"));
    assert!(!harness.was_invoked("npm"));
}

#[test]
fn unmatched_npm_owner_does_not_auto_switch() {
    let harness = Harness::new(&["npm"]);
    let output = harness.run(&["install", "--tool", "npm", "cli/cli"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let npm = harness.invocation("npm");
    assert!(!npm.git_auth_header_present);
    assert!(!harness.was_invoked("gh-auth-token"));
}

#[test]
fn install_help_lists_the_tool_option_and_values() {
    let harness = Harness::new(&[]);
    let output = harness.run(&["install", "--help"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let help = stdout(&output);
    assert!(help.contains("-t, --tool <TOOL>"), "{help}");
    assert!(help.contains("-a, --add"), "{help}");
    assert!(
        help.contains("[possible values: go, cargo, uv, pipx, npm, auto]"),
        "{help}"
    );
    assert!(help.contains("rust is an alias for cargo"), "{help}");
    assert!(
        help.contains("remote Go sources may end in @<query>"),
        "{help}"
    );
}

#[test]
fn go_remote_install_adds_latest_by_default() {
    let harness = Harness::new(&["go"]);
    let output = harness.run(&[
        "install",
        "--tool",
        "go",
        "https://github.com/hjr265/gittop",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("go").args,
        ["install", "github.com/hjr265/gittop@latest"]
    );
}

#[test]
fn explicit_go_queries_support_url_shorthand_and_scp_like_repositories() {
    let harness = Harness::new(&["go"]);
    for (source, expected) in [
        (
            "https://github.com/yuser/reepo@some-go-stuff",
            "github.com/yuser/reepo@some-go-stuff",
        ),
        (
            "yuser/reepo@feature/branch",
            "github.com/yuser/reepo@feature/branch",
        ),
        (
            "git@example.com:owner/reepo.git@deadbeef",
            "example.com/owner/reepo@deadbeef",
        ),
    ] {
        let output = harness.run(&["install", "--tool", "go", source]);
        assert!(output.status.success(), "{source}: {}", stderr(&output));
        assert_eq!(harness.invocation("go").args, ["install", expected]);
    }
}

#[test]
fn go_queries_conflict_with_no_latest_and_non_go_backends() {
    let go = Harness::new(&["go"]);
    let output = go.run(&[
        "install",
        "--tool",
        "go",
        "--no-latest",
        "owner/reepo@v1.2.3",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("explicit Go version query conflicts with --no-latest"));
    assert!(!go.was_invoked("go"));

    let cargo = Harness::new(&["cargo"]);
    let output = cargo.run(&["install", "--tool", "cargo", "reepo@v1.2.3"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("Go version query @v1.2.3 requires --tool go"));
    assert!(!cargo.was_invoked("cargo"));

    let auto = Harness::new(&["gh", "cargo"]);
    let output = auto
        .command(&["install", "owner/reepo@v1.2.3"])
        .env(
            "DTR_TEST_GITHUB_TREE_JSON",
            r#"{"truncated":false,"tree":[{"path":"Cargo.toml","type":"blob"}]}"#,
        )
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("but cargo was selected"));
    assert_eq!(
        auto.invocation("gh-api-tree").args[1],
        "repos/owner/reepo/git/trees/HEAD"
    );
    assert!(!auto.was_invoked("cargo"));
}

#[test]
fn go_remote_install_honors_no_latest_and_i_alias() {
    let harness = Harness::new(&["go"]);
    let output = harness.run(&[
        "i",
        "--tool",
        "go",
        "--no-latest",
        "git@example.com:owner/tool.git",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        harness.invocation("go").args,
        ["install", "example.com/owner/tool"]
    );
}

#[test]
fn bare_go_repo_uses_authenticated_github_owner() {
    let harness = Harness::new(&["go", "gh"]);
    let output = harness.run(&["install", "--tool", "go", "my-tool"]);
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
    let output = harness.run(&["install", "--tool", "go", "./local repo"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let invocation = harness.invocation("go");
    assert_eq!(
        fs::canonicalize(invocation.cwd).unwrap(),
        fs::canonicalize(repo).unwrap()
    );
    assert_eq!(invocation.args, ["install", "./..."]);
}

#[test]
fn local_go_install_reports_all_binaries_and_path_shadowing() {
    let harness = Harness::new(&["go"]);
    let repo = harness.work.join("local repo");
    let install_directory = harness.work.join("go-bin");
    fs::create_dir(&repo).unwrap();
    fs::create_dir(&install_directory).unwrap();
    let gh = install_directory.join("gh");
    let gen_docs = install_directory.join("gen-docs");
    write_executable(&gh);
    write_executable(&gen_docs);
    write_executable(&harness.bin.join("gh"));
    let path = env::join_paths([harness.bin.as_path(), install_directory.as_path()]).unwrap();
    let targets = format!("{}\n{}", gh.display(), gen_docs.display());

    let output = harness
        .command(&["install", "--tool", "go", "./local repo"])
        .env("PATH", path)
        .env("DTR_TEST_GO_LIST_OUTPUT", targets)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "");
    let narration = stderr(&output);
    assert!(
        narration.contains(&format!(
            "→ go install ./... (in '{}')",
            repo.canonicalize().unwrap().display()
        )),
        "{narration}"
    );
    assert!(
        narration.contains(&format!(
            "installed: gh, gen-docs → {}",
            install_directory.display()
        )),
        "{narration}"
    );
    assert!(
        narration.contains(&format!(
            "warning: 'gh' is shadowed by {} (earlier on PATH)",
            harness.bin.join("gh").display()
        )),
        "{narration}"
    );
    assert!(!narration.contains("'gen-docs' is shadowed"), "{narration}");
    assert_eq!(
        harness.invocation("go-list").args,
        [
            "list",
            "-f",
            "{{if eq .Name \"main\"}}{{.Target}}{{end}}",
            "./...",
        ]
    );
}

#[test]
fn go_path_warning_survives_no_narration() {
    let harness = Harness::new(&["go"]);
    let install_directory = harness.work.join("go-bin");
    fs::create_dir(&install_directory).unwrap();
    write_executable(&install_directory.join("tool"));

    let output = harness
        .command(&["--no-narration", "install", "--tool", "go", "owner/tool"])
        .env("DTR_TEST_GO_GOBIN", &install_directory)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "");
    assert_eq!(
        stderr(&output),
        format!("warning: {} is not on PATH\n", install_directory.display())
    );
}

#[test]
fn path_symlink_to_the_installed_go_binary_is_not_reported_as_a_shadow() {
    let harness = Harness::new(&["go"]);
    let install_directory = harness.work.join("go-bin");
    fs::create_dir(&install_directory).unwrap();
    let installed = install_directory.join("tool");
    write_executable(&installed);
    std::os::unix::fs::symlink(&installed, harness.bin.join("tool")).unwrap();
    let path = env::join_paths([harness.bin.as_path(), install_directory.as_path()]).unwrap();

    let output = harness
        .command(&["install", "--tool", "go", "owner/tool"])
        .env("PATH", path)
        .env("DTR_TEST_GO_GOBIN", &install_directory)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("installed: tool"));
    assert!(!stderr(&output).contains("shadowed"), "{}", stderr(&output));
}

#[test]
fn remote_go_install_uses_gopath_and_skips_a_major_version_suffix_for_the_binary_name() {
    let harness = Harness::new(&["go"]);
    let gopath = harness.work.join("gopath");
    let install_directory = gopath.join("bin");
    fs::create_dir_all(&install_directory).unwrap();
    write_executable(&install_directory.join("tool"));
    let path = env::join_paths([harness.bin.as_path(), install_directory.as_path()]).unwrap();

    let output = harness
        .command(&[
            "install",
            "--tool",
            "go",
            "https://example.com/owner/tool/v2",
        ])
        .env("PATH", path)
        .env("DTR_TEST_GO_GOPATH", &gopath)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains(&format!(
            "installed: tool → {}",
            install_directory.display()
        )),
        "{}",
        stderr(&output)
    );
}

#[test]
fn no_latest_is_rejected_for_local_repo() {
    let harness = Harness::new(&["go"]);
    let output = harness.run(&["install", "--tool", "go", "--no-latest", "./repo"]);
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
    let output = harness.run(&["install", "--tool", "go", "owner/repo"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("go is required"));
}
