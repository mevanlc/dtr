use std::collections::VecDeque;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{self, Command};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};

use crate::cli::{InstallAllArgs, InstallArgs, InstallTool, Jobs};
use crate::command::{CommandPlan, DtrMessage, InterruptState, command_exists, shell_quote};
use crate::config::{self, Config};
use crate::error::DtrError;
use crate::repospec::InstallSource;
use crate::resolve::plan_install;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallAllConfig {
    #[serde(default)]
    install: Vec<InstallEntry>,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct InstallEntry {
    repospec: String,

    #[serde(default, skip_serializing_if = "install_tool_is_auto")]
    tool: InstallTool,

    #[serde(default, skip_serializing_if = "is_false")]
    no_latest: bool,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    args: Vec<String>,
}

struct LoadedConfig {
    config: InstallAllConfig,
    text: String,
}

struct InstallJob {
    number: usize,
    repospec: String,
    plan: CommandPlan,
}

struct InstallJobResult {
    number: usize,
    succeeded: bool,
    messages: Vec<DtrMessage>,
}

struct InstallAllExecution {
    failed: bool,
    interrupted: bool,
}

impl InstallEntry {
    fn into_args(self, home: Option<&Path>) -> Result<InstallArgs, DtrError> {
        if self.repospec.is_empty() {
            return Err(DtrError::new("repospec must not be empty"));
        }

        Ok(InstallArgs {
            add: false,
            tool: self.tool,
            no_latest: self.no_latest,
            repospec: expand_home(self.repospec, home)?,
            install_args: self.args.into_iter().map(OsString::from).collect(),
        })
    }

    fn from_install_args(args: &InstallArgs, home: Option<&Path>) -> Result<Self, DtrError> {
        let source = InstallSource::parse(&args.repospec)?;
        let repospec = if let Some(path) = source.spec.local_path() {
            let canonical = fs::canonicalize(path).map_err(|error| {
                DtrError::new(format!(
                    "could not resolve local repository path {} for tracking: {error}",
                    path.display()
                ))
            })?;
            tracked_local_path(&canonical, home)?
        } else {
            args.repospec
                .to_str()
                .ok_or_else(|| {
                    DtrError::new("install-all.toml cannot store a non-UTF-8 repository reference")
                })?
                .to_owned()
        };
        let install_args = args
            .install_args
            .iter()
            .map(|argument| {
                argument.to_str().map(str::to_owned).ok_or_else(|| {
                    DtrError::new("install-all.toml cannot store a non-UTF-8 installer argument")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            repospec,
            tool: args.tool,
            no_latest: args.no_latest,
            args: install_args,
        })
    }
}

fn install_tool_is_auto(tool: &InstallTool) -> bool {
    *tool == InstallTool::Auto
}

fn is_false(value: &bool) -> bool {
    !value
}

pub(crate) fn run(
    args: InstallAllArgs,
    explain: bool,
    narration_override: Option<bool>,
) -> Result<i32, DtrError> {
    let path = selected_file_path(args.file)?;
    if args.list {
        return list(&path);
    }
    if args.edit {
        return edit(&path, explain, narration_override);
    }

    let config = load_required(&path)?.config;
    let requested_jobs = args.jobs;
    let job_count = requested_jobs.resolve();
    let narration = if explain {
        false
    } else {
        match narration_override {
            Some(narration) => narration,
            None => Config::load_for_runtime()?.narration(),
        }
    };
    let interrupted = if explain {
        None
    } else {
        Some(install_interrupt_handler()?)
    };
    let home = home::home_dir();
    let mut failed = false;
    let mut jobs = Vec::new();
    let mut preflight_results = Vec::new();

    if explain {
        match requested_jobs {
            Jobs::Auto => println!("jobs: {job_count} (auto)"),
            Jobs::Count(_) => println!("jobs: {job_count}"),
        }
    }

    for (offset, entry) in config.install.into_iter().enumerate() {
        if interrupted
            .as_ref()
            .is_some_and(|interrupted| interrupted.is_interrupted())
        {
            break;
        }
        let number = offset + 1;
        let repospec = entry.repospec.clone();
        let plan = entry.into_args(home.as_deref()).and_then(plan_install);
        if interrupted
            .as_ref()
            .is_some_and(|interrupted| interrupted.is_interrupted())
        {
            break;
        }
        let plan = match plan {
            Ok(plan) => plan,
            Err(error) => {
                let warning = entry_warning(number, &repospec, &error);
                eprintln!("{warning}");
                if !explain {
                    preflight_results.push(InstallJobResult {
                        number,
                        succeeded: false,
                        messages: vec![DtrMessage::Stderr(warning)],
                    });
                }
                failed = true;
                continue;
            }
        };

        if explain {
            println!();
            println!("install-all entry {number}:");
            plan.explain();
            continue;
        }

        jobs.push(InstallJob {
            number,
            repospec,
            plan,
        });
    }

    if !explain {
        let execution = execute_jobs(
            jobs,
            job_count,
            narration,
            preflight_results,
            interrupted
                .as_deref()
                .expect("live install-all runs install an interrupt handler"),
        );
        if execution.interrupted {
            return Ok(130);
        }
        if execution.failed {
            failed = true;
        }
    }

    Ok(i32::from(failed))
}

pub(crate) fn run_install_and_add(
    args: InstallArgs,
    explain: bool,
    narration_override: Option<bool>,
) -> Result<i32, DtrError> {
    let path = config::install_all_file_path()?;
    let _ = load_optional(&path)?;
    let home = home::home_dir();
    let mut entry = InstallEntry::from_install_args(&args, home.as_deref())?;
    let plan = plan_install(args)?;
    if entry.tool == InstallTool::Auto {
        entry.tool = backend_tool(plan.backend)?;
    }

    if explain {
        plan.explain();
        println!("track:    {}", shell_quote(path.as_os_str()));
        return Ok(0);
    }

    let narration = match narration_override {
        Some(narration) => narration,
        None => Config::load_for_runtime()?.narration(),
    };
    let code = plan.execute(narration)?;
    if code != 0 {
        return Ok(code);
    }

    let loaded = load_optional(&path)?;
    let already_tracked = loaded.config.install.contains(&entry);
    if !already_tracked {
        let text = append_entry(loaded.text, &entry)?;
        write_atomic(&path, &text)?;
    }
    if narration {
        if already_tracked {
            eprintln!("already tracked: {}", entry.repospec);
        } else {
            eprintln!(
                "tracked: {} → {}",
                entry.repospec,
                shell_quote(path.as_os_str())
            );
        }
    }
    Ok(0)
}

fn execute_jobs(
    jobs: Vec<InstallJob>,
    job_count: usize,
    narration: bool,
    initial_results: Vec<InstallJobResult>,
    interrupted: &InterruptState,
) -> InstallAllExecution {
    let worker_count = job_count.min(jobs.len());
    let queue = Mutex::new(VecDeque::from(jobs));
    let results = Mutex::new(initial_results);

    thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    if interrupted.is_interrupted() {
                        break;
                    }
                    let job = queue
                        .lock()
                        .expect("install-all work queue should not be poisoned")
                        .pop_front();
                    let Some(job) = job else {
                        break;
                    };
                    if interrupted.is_interrupted() {
                        break;
                    }
                    let result = execute_job(&job, narration, interrupted);
                    results
                        .lock()
                        .expect("install-all results should not be poisoned")
                        .push(result);
                }
            });
        }
    });

    let mut results = results
        .into_inner()
        .expect("install-all results should not be poisoned");
    results.sort_by_key(|result| result.number);
    let failed = results.iter().any(|result| !result.succeeded);
    for result in results {
        for message in result.messages {
            message.emit();
        }
    }
    InstallAllExecution {
        failed,
        interrupted: interrupted.is_interrupted(),
    }
}

fn execute_job(
    job: &InstallJob,
    narration: bool,
    interrupted: &InterruptState,
) -> InstallJobResult {
    let mut messages = Vec::new();
    let execution = job
        .plan
        .execute_with_replay(narration, &mut messages, interrupted);
    let succeeded = if interrupted.is_interrupted() {
        false
    } else {
        match execution {
            Ok(0) => true,
            Ok(code) => {
                let warning = format!(
                    "dtr: warning: install-all entry {} ({:?}) exited with status {code}",
                    job.number, job.repospec
                );
                eprintln!("{warning}");
                messages.push(DtrMessage::Stderr(warning));
                false
            }
            Err(error) => {
                let warning = entry_warning(job.number, &job.repospec, &error);
                eprintln!("{warning}");
                messages.push(DtrMessage::Stderr(warning));
                false
            }
        }
    };
    InstallJobResult {
        number: job.number,
        succeeded,
        messages,
    }
}

fn install_interrupt_handler() -> Result<Arc<InterruptState>, DtrError> {
    let interrupted = Arc::new(InterruptState::new());
    let handler_state = Arc::clone(&interrupted);
    ctrlc::set_handler(move || match handler_state.request_interrupt() {
        0 => {}
        1 => {
            handler_state.stop_active_children();
        }
        _ => {
            process::exit(130);
        }
    })
    .map_err(|error| DtrError::new(format!("could not install Ctrl-C handler: {error}")))?;
    Ok(interrupted)
}

fn selected_file_path(alternate: Option<PathBuf>) -> Result<PathBuf, DtrError> {
    let Some(path) = alternate else {
        return config::install_all_file_path();
    };
    if path.as_os_str().is_empty() {
        return Err(DtrError::new("--file must not be empty"));
    }
    let Some(text) = path.to_str() else {
        return Ok(path);
    };
    Ok(PathBuf::from(expand_home(
        text.to_owned(),
        home::home_dir().as_deref(),
    )?))
}

fn load_required(path: &Path) -> Result<LoadedConfig, DtrError> {
    let loaded = load_config(path, false)?;
    if loaded.config.install.is_empty() {
        return Err(DtrError::new(format!(
            "install-all configuration {} must contain at least one [[install]] entry",
            path.display()
        )));
    }
    Ok(loaded)
}

fn load_optional(path: &Path) -> Result<LoadedConfig, DtrError> {
    load_config(path, true)
}

fn load_config(path: &Path, missing_ok: bool) -> Result<LoadedConfig, DtrError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if missing_ok && error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LoadedConfig {
                config: InstallAllConfig::default(),
                text: String::new(),
            });
        }
        Err(error) => {
            return Err(DtrError::new(format!(
                "could not read install-all configuration {}: {error}",
                path.display()
            )));
        }
    };
    let config = toml::from_str(&text).map_err(|error| {
        DtrError::new(format!(
            "could not parse install-all configuration {}: {error}",
            path.display()
        ))
    })?;
    Ok(LoadedConfig { config, text })
}

fn list(path: &Path) -> Result<i32, DtrError> {
    let home = home::home_dir();
    for entry in load_required(path)?.config.install {
        println!("{}", render_install_command(&entry, home.as_deref())?);
    }
    Ok(0)
}

fn render_install_command(entry: &InstallEntry, home: Option<&Path>) -> Result<String, DtrError> {
    let mut arguments = vec![OsString::from("dtr"), OsString::from("install")];
    if entry.tool != InstallTool::Auto {
        arguments.push("--tool".into());
        arguments.push(entry.tool.to_string().into());
    }
    if entry.no_latest {
        arguments.push("--no-latest".into());
    }
    arguments.push(expand_home(entry.repospec.clone(), home)?);
    if !entry.args.is_empty() {
        arguments.push("--".into());
        arguments.extend(entry.args.iter().cloned().map(OsString::from));
    }
    Ok(arguments
        .iter()
        .map(|argument| shell_quote(argument))
        .collect::<Vec<_>>()
        .join(" "))
}

fn edit(path: &Path, explain: bool, narration_override: Option<bool>) -> Result<i32, DtrError> {
    let (program, mut arguments) = editor_command()?;
    arguments.push(path.as_os_str().to_os_string());
    let rendered = std::iter::once(program.as_os_str())
        .chain(arguments.iter().map(OsString::as_os_str))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ");
    if explain {
        println!("command:  {rendered}");
        return Ok(0);
    }

    let parent = config_parent(path);
    fs::create_dir_all(parent).map_err(|error| {
        DtrError::new(format!(
            "could not create configuration directory {}: {error}",
            parent.display()
        ))
    })?;
    let narration = match narration_override {
        Some(narration) => narration,
        None => Config::load_for_runtime()?.narration(),
    };
    if narration {
        eprintln!("→ {rendered}");
    }
    let status = Command::new(&program)
        .args(&arguments)
        .status()
        .map_err(|error| {
            DtrError::new(format!(
                "could not start editor {}: {error}",
                program.to_string_lossy()
            ))
        })?;
    Ok(status.code().unwrap_or(1))
}

fn editor_command() -> Result<(OsString, Vec<OsString>), DtrError> {
    for variable in ["DTR_EDITOR", "VISUAL", "EDITOR"] {
        let Some(value) = env::var_os(variable) else {
            continue;
        };
        if value.is_empty() {
            continue;
        }
        let text = value.to_str().ok_or_else(|| {
            DtrError::new(format!(
                "{variable} must be valid UTF-8 to parse editor arguments"
            ))
        })?;
        let words = shlex::split(text).ok_or_else(|| {
            DtrError::new(format!("could not parse {variable} as an editor command"))
        })?;
        if words.is_empty() {
            continue;
        }
        let mut words = words.into_iter().map(OsString::from);
        let program = words.next().expect("nonempty editor command");
        return Ok((program, words.collect()));
    }
    for fallback in ["vim", "vi"] {
        if command_exists(fallback) {
            return Ok((fallback.into(), Vec::new()));
        }
    }
    Err(DtrError::new(
        "could not find an editor; set DTR_EDITOR, VISUAL, or EDITOR, or install vim or vi",
    ))
}

fn backend_tool(backend: &str) -> Result<InstallTool, DtrError> {
    match backend {
        "go" => Ok(InstallTool::Go),
        "cargo" => Ok(InstallTool::Cargo),
        "uv" => Ok(InstallTool::Uv),
        "pipx" => Ok(InstallTool::Pipx),
        "npm" => Ok(InstallTool::Npm),
        _ => Err(DtrError::new(format!(
            "cannot track unsupported install backend {backend:?}"
        ))),
    }
}

fn tracked_local_path(path: &Path, home: Option<&Path>) -> Result<String, DtrError> {
    if let Some(home) = home {
        if path == home {
            return Ok("~".to_owned());
        }
        if let Ok(suffix) = path.strip_prefix(home) {
            let suffix = suffix.to_str().ok_or_else(|| {
                DtrError::new("install-all.toml cannot store a non-UTF-8 local repository path")
            })?;
            return Ok(format!("~/{suffix}"));
        }
    }
    path.to_str().map(str::to_owned).ok_or_else(|| {
        DtrError::new("install-all.toml cannot store a non-UTF-8 local repository path")
    })
}

fn append_entry(mut text: String, entry: &InstallEntry) -> Result<String, DtrError> {
    if !text.is_empty() {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        if !text.ends_with("\n\n") {
            text.push('\n');
        }
    }
    text.push_str("[[install]]\n");
    text.push_str(&toml::to_string(entry).map_err(|error| {
        DtrError::new(format!("could not serialize install-all entry: {error}"))
    })?);
    Ok(text)
}

fn write_atomic(path: &Path, text: &str) -> Result<(), DtrError> {
    let parent = config_parent(path);
    fs::create_dir_all(parent).map_err(|error| {
        DtrError::new(format!(
            "could not create configuration directory {}: {error}",
            parent.display()
        ))
    })?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
        DtrError::new(format!(
            "could not create a temporary configuration file in {}: {error}",
            parent.display()
        ))
    })?;
    if let Ok(metadata) = fs::metadata(path) {
        temporary
            .as_file_mut()
            .set_permissions(metadata.permissions())
            .map_err(|error| {
                DtrError::new(format!(
                    "could not preserve configuration permissions: {error}"
                ))
            })?;
    }
    temporary.write_all(text.as_bytes()).map_err(|error| {
        DtrError::new(format!(
            "could not write temporary configuration file: {error}"
        ))
    })?;
    temporary.as_file_mut().sync_all().map_err(|error| {
        DtrError::new(format!(
            "could not flush temporary configuration file: {error}"
        ))
    })?;
    temporary.persist(path).map_err(|error| {
        DtrError::new(format!(
            "could not replace install-all configuration {}: {}",
            path.display(),
            error.error
        ))
    })?;
    Ok(())
}

fn config_parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn expand_home(repospec: String, home: Option<&Path>) -> Result<OsString, DtrError> {
    let suffix = if repospec == "~" {
        Some("")
    } else {
        repospec.strip_prefix("~/")
    };
    let Some(suffix) = suffix else {
        return Ok(repospec.into());
    };
    let home = home.ok_or_else(|| {
        DtrError::new(format!(
            "could not expand {repospec:?} because the user home directory could not be located"
        ))
    })?;
    Ok(home.join(suffix).into_os_string())
}

fn entry_warning(number: usize, repospec: &str, error: &DtrError) -> String {
    format!("dtr: warning: install-all entry {number} ({repospec:?}): {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn config_accepts_native_args_and_rust_alias() {
        let config: InstallAllConfig = toml::from_str(
            r#"
                [[install]]
                repospec = "~/p/my/ripgrep"
                tool = "rust"
                args = ["--force", "--features", "pcre2"]
            "#,
        )
        .unwrap();
        let entry = config.install.into_iter().next().unwrap();
        assert_eq!(entry.tool, InstallTool::Cargo);
        assert_eq!(entry.args, ["--force", "--features", "pcre2"]);
    }

    #[test]
    fn config_rejects_unknown_fields() {
        let error = toml::from_str::<InstallAllConfig>(
            r#"
                [[install]]
                repospec = "./tool"
                surprise = true
            "#,
        )
        .err()
        .expect("unknown field should fail");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn leading_tilde_expands_only_for_a_home_relative_path() {
        let home = Path::new("/home/user");
        assert_eq!(
            PathBuf::from(expand_home("~/p/my/tool".to_owned(), Some(home)).unwrap()),
            PathBuf::from("/home/user/p/my/tool")
        );
        assert_eq!(
            expand_home("~someone/tool".to_owned(), Some(home)).unwrap(),
            OsString::from("~someone/tool")
        );
        assert!(expand_home("~/tool".to_owned(), None).is_err());
    }

    #[test]
    fn appended_entries_preserve_existing_text_and_omit_defaults() {
        let entry = InstallEntry {
            repospec: "~/p/my/tool".to_owned(),
            tool: InstallTool::Auto,
            no_latest: false,
            args: Vec::new(),
        };
        assert_eq!(
            append_entry("# keep this comment\n".to_owned(), &entry).unwrap(),
            "# keep this comment\n\n[[install]]\nrepospec = \"~/p/my/tool\"\n"
        );
    }

    #[test]
    fn tracked_local_paths_use_home_shorthand_when_possible() {
        let home = Path::new("/home/user");
        assert_eq!(
            tracked_local_path(Path::new("/home/user/p/my/tool"), Some(home)).unwrap(),
            "~/p/my/tool"
        );
        assert_eq!(
            tracked_local_path(Path::new("/opt/tool"), Some(home)).unwrap(),
            "/opt/tool"
        );
    }

    #[test]
    fn listed_entries_render_as_shell_safe_install_commands() {
        let entry = InstallEntry {
            repospec: "~/p/my/tool with spaces".to_owned(),
            tool: InstallTool::Cargo,
            no_latest: false,
            args: vec!["--features".to_owned(), "pcre2".to_owned()],
        };
        assert_eq!(
            render_install_command(&entry, Some(Path::new("/home/user"))).unwrap(),
            "dtr install --tool cargo '/home/user/p/my/tool with spaces' -- --features pcre2"
        );
    }
}
