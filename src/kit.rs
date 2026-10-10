use std::collections::{HashMap, HashSet, VecDeque};
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::thread;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use toml_edit::{Array, DocumentMut, InlineTable, Item, Table, Value, value};

use crate::cli::{InstallArgs, InstallTool, Jobs, KitArgs, KitCommand};
use crate::command::{
    CommandPlan, DtrMessage, GitReference, InstallRevision, InterruptState, apply_environment,
    config_parent, display_path, edit_file, friendly_path, shell_quote_argument,
};
use crate::config::{self, Config};
use crate::error::DtrError;
use crate::output::Verbosity;
use crate::repospec::{InstallSource, RepositoryIdentity};
use crate::resolve::plan_install;
use crate::self_rebuild;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct KitConfig {
    #[serde(default)]
    install: Vec<InstallEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallEntry {
    pub(crate) repospec: String,

    #[serde(default, skip_serializing_if = "install_tool_is_auto")]
    pub(crate) tool: InstallTool,

    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) no_latest: bool,

    #[serde(default, skip_serializing_if = "is_false")]
    main: bool,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) args: Vec<String>,

    #[serde(default, skip_serializing)]
    pub(crate) installed: Option<InstalledState>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstalledState {
    pub(crate) commit: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) branch: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub(crate) dirty: bool,
    pub(crate) request: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProbedRevision {
    commit: String,
    branch: Option<String>,
    dirty: bool,
}

struct LoadedConfig {
    config: KitConfig,
    text: String,
}

struct InstallJob {
    number: usize,
    repospec: String,
    plan: CommandPlan,
    entry: InstallEntry,
    request: String,
    before_revision: Option<ProbedRevision>,
}

struct InstallJobResult {
    number: usize,
    succeeded: bool,
    messages: Vec<DtrMessage>,
    state_update: Option<StateUpdate>,
}

struct StateUpdate {
    entry: InstallEntry,
    installed: Option<InstalledState>,
}

struct KitExecution {
    failed: bool,
    interrupted: bool,
    state_updates: Vec<StateUpdate>,
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
            main: self.main,
            repospec: expand_home(self.repospec, home)?,
            install_args: self.args.into_iter().map(OsString::from).collect(),
        })
    }

    pub(crate) fn from_install_args(
        args: &InstallArgs,
        home: Option<&Path>,
    ) -> Result<(Self, Option<PathBuf>), DtrError> {
        let source = InstallSource::parse(&args.repospec)?;
        let (repospec, local_path) = if let Some(path) = source.spec.local_path() {
            let canonical = fs::canonicalize(path).map_err(|error| {
                DtrError::new(format!(
                    "could not resolve local repository path {} for tracking: {error}",
                    path.display()
                ))
            })?;
            let canonical_home = home.and_then(|home| fs::canonicalize(home).ok());
            (
                tracked_local_path(&canonical, canonical_home.as_deref().or(home))?,
                Some(canonical),
            )
        } else {
            (
                args.repospec
                    .to_str()
                    .ok_or_else(|| {
                        DtrError::new("kit.toml cannot store a non-UTF-8 repository reference")
                    })?
                    .to_owned(),
                None,
            )
        };
        let install_args = args
            .install_args
            .iter()
            .map(|argument| {
                argument.to_str().map(str::to_owned).ok_or_else(|| {
                    DtrError::new("kit.toml cannot store a non-UTF-8 installer argument")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        Ok((
            Self {
                repospec,
                tool: args.tool,
                no_latest: args.no_latest,
                main: args.main,
                args: install_args,
                installed: None,
            },
            local_path,
        ))
    }
}

fn install_tool_is_auto(tool: &InstallTool) -> bool {
    *tool == InstallTool::Auto
}

fn is_false(value: &bool) -> bool {
    !value
}

pub(crate) fn run(
    args: KitArgs,
    dry_run: bool,
    verbosity_adjustment: i32,
) -> Result<i32, DtrError> {
    let args = match args.command {
        KitCommand::Add(args) => {
            let install = args.install();
            return run_add(
                args.into_install_args(),
                install,
                dry_run,
                verbosity_adjustment,
            );
        }
        KitCommand::List(args) => return list(&selected_file_path(args.file, !dry_run)?),
        KitCommand::Edit(args) => {
            let path = selected_file_path(args.file, !dry_run)?;
            let base = Config::load_for_runtime()
                .map(|config| config.verbosity())
                .unwrap_or(0);
            let verbosity = Verbosity::new(base, verbosity_adjustment);
            return edit_file(&path, dry_run, verbosity);
        }
        KitCommand::Install(args) => args,
    };
    let path = selected_file_path(args.file, !dry_run)?;

    let config = load_required(&path)?.config;
    let requested_jobs = args.jobs;
    let force = args.force;
    let job_count = requested_jobs.resolve();
    let runtime_config = Config::load_for_runtime()?;
    let verbosity = Verbosity::new(runtime_config.verbosity(), verbosity_adjustment);
    let interrupted = if dry_run {
        None
    } else {
        Some(InterruptState::install_handler()?)
    };
    let home = home::home_dir();
    let mut failed = false;
    let mut jobs = Vec::new();
    let mut preflight_results = Vec::new();

    if dry_run && verbosity.verbose() {
        match requested_jobs {
            Jobs::Auto => eprintln!("jobs: {job_count} (auto)"),
            Jobs::Count(_) => eprintln!("jobs: {job_count}"),
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
        let request = request_fingerprint(&entry, &runtime_config);
        let plan = entry
            .clone()
            .into_args(home.as_deref())
            .and_then(|args| plan_install(args, verbosity));
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
                if !dry_run {
                    preflight_results.push(InstallJobResult {
                        number,
                        succeeded: false,
                        messages: vec![DtrMessage::Stderr(warning)],
                        state_update: None,
                    });
                }
                failed = true;
                continue;
            }
        };

        let before_revision = if dry_run && force {
            None
        } else {
            match probe_install_revision(&plan) {
                Ok(revision) => revision,
                Err(error) => {
                    let warning = cache_warning(number, &repospec, &error);
                    eprintln!("{warning}");
                    if !dry_run {
                        preflight_results.push(InstallJobResult {
                            number,
                            succeeded: true,
                            messages: vec![DtrMessage::Stderr(warning)],
                            state_update: None,
                        });
                    }
                    None
                }
            }
        };
        let cached = !force
            && before_revision.as_ref().is_some_and(|probed| {
                !probed.dirty
                    && entry.installed.as_ref().is_some_and(|installed| {
                        !installed.dirty
                            && installed.commit == probed.commit
                            && installed.request == request
                    })
            });
        let cache_reason = if force {
            "forced installation"
        } else if cached {
            "revision and install request match; skipping"
        } else if before_revision.is_none() {
            "no cacheable revision; installing"
        } else if before_revision
            .as_ref()
            .is_some_and(|revision| revision.dirty)
        {
            "worktree is dirty; installing"
        } else if entry.installed.is_none() {
            "no recorded install state; installing"
        } else {
            "revision or install request changed; installing"
        };
        verbosity.debug(format_args!("kit entry {number}: {cache_reason}"));

        if dry_run {
            if cached {
                if verbosity.verbose() {
                    println!(
                        "skip: {repospec} @ {} (already installed)",
                        short_revision(&before_revision.as_ref().expect("cached revision").commit)
                    );
                }
            } else {
                println!("install: {repospec} ({})", plan.backend);
                if verbosity.verbose() {
                    eprintln!("{}", plan.command_message());
                }
            }
            continue;
        }

        if cached {
            let message = format!(
                "up to date: {} @ {} (skipped)",
                repospec,
                short_revision(&before_revision.as_ref().expect("cached revision").commit)
            );
            let messages = if verbosity.verbose() {
                vec![DtrMessage::Stderr(message)]
            } else {
                Vec::new()
            };
            preflight_results.push(InstallJobResult {
                number,
                succeeded: true,
                messages,
                state_update: None,
            });
            continue;
        }

        jobs.push(InstallJob {
            number,
            repospec,
            plan,
            entry,
            request,
            before_revision,
        });
    }

    if !dry_run {
        if let Some(self_job) = jobs
            .iter()
            .find(|job| self_rebuild::is_self_install_entry(&job.entry))
            && let Some(package_root) = self_rebuild::resolve_local_package_root(&self_job.repospec)
        {
            return self_rebuild::execute(&package_root, interrupted.clone(), verbosity);
        }

        let execution = execute_jobs(
            jobs,
            job_count,
            verbosity,
            preflight_results,
            interrupted
                .as_deref()
                .expect("live kit runs install an interrupt handler"),
        );
        if let Err(error) = persist_state_updates(&path, execution.state_updates, home.as_deref()) {
            eprintln!("dtr: warning: could not refresh install state: {error}");
        }
        if execution.interrupted {
            return Ok(130);
        }
        if execution.failed {
            failed = true;
        }
    }

    Ok(i32::from(failed))
}

fn run_add(
    args: InstallArgs,
    install: bool,
    dry_run: bool,
    verbosity_adjustment: i32,
) -> Result<i32, DtrError> {
    if install {
        return run_install_and_add(args, dry_run, verbosity_adjustment);
    }

    let path = if dry_run {
        config::kit_file_path()?
    } else {
        config::migrate_legacy_kit_file()?
    };
    let loaded = load_optional(&path)?;
    let home = home::home_dir();
    let (entry, local_path) = InstallEntry::from_install_args(&args, home.as_deref())?;

    if dry_run {
        println!("track:    {}", display_path(&path));
        return Ok(0);
    }

    let verbosity = Verbosity::load(verbosity_adjustment)?;
    track_entry(
        loaded,
        entry,
        local_path.as_deref(),
        &path,
        home.as_deref(),
        verbosity,
        true,
    )?;
    Ok(0)
}

pub(crate) fn run_install_and_add(
    args: InstallArgs,
    dry_run: bool,
    verbosity_adjustment: i32,
) -> Result<i32, DtrError> {
    let runtime_config = Config::load_for_runtime()?;
    let verbosity = Verbosity::new(runtime_config.verbosity(), verbosity_adjustment);
    let path = if dry_run {
        config::kit_file_path()?
    } else {
        config::migrate_legacy_kit_file()?
    };
    let _ = load_optional(&path)?;
    let home = home::home_dir();
    let (mut entry, local_path) = InstallEntry::from_install_args(&args, home.as_deref())?;
    let plan = plan_install(args, verbosity)?;
    if entry.tool == InstallTool::Auto {
        entry.tool = backend_tool(plan.backend)?;
    }

    if dry_run {
        plan.explain();
        println!("track:    {}", display_path(&path));
        return Ok(0);
    }

    if self_rebuild::is_self_install_entry(&entry)
        && let Some(package_root) = self_rebuild::resolve_local_package_root(&entry.repospec)
    {
        return self_rebuild::execute(&package_root, None, verbosity);
    }

    let request = request_fingerprint(&entry, &runtime_config);
    let before_revision = match probe_install_revision(&plan) {
        Ok(revision) => revision,
        Err(error) => {
            eprintln!("dtr: warning: install state could not be inspected: {error}");
            None
        }
    };
    let code = plan.execute(verbosity)?;
    if code != 0 {
        return Ok(code);
    }

    entry.installed = successful_install_state(&plan, before_revision, request, |warning| {
        eprintln!("dtr: warning: install state could not be refreshed: {warning}");
    });
    let loaded = load_optional(&path)?;
    track_entry(
        loaded,
        entry,
        local_path.as_deref(),
        &path,
        home.as_deref(),
        verbosity,
        false,
    )?;
    Ok(0)
}

fn track_entry(
    loaded: LoadedConfig,
    mut entry: InstallEntry,
    local_path: Option<&Path>,
    path: &Path,
    home: Option<&Path>,
    verbosity: Verbosity,
    preserve_matching_state: bool,
) -> Result<(), DtrError> {
    let identity = normalized_repository_identity(&entry, home)?;
    let existing = loaded.config.install.iter().position(|tracked| {
        normalized_repository_identity(tracked, home).is_ok_and(|tracked| tracked == identity)
    });
    if preserve_matching_state
        && let Some(index) = existing
        && same_install_request(&loaded.config.install[index], &entry)
    {
        entry.installed = loaded.config.install[index].installed.clone();
    }
    let changed = existing.is_none_or(|index| loaded.config.install[index] != entry);
    if changed {
        let text = match existing {
            Some(index) => replace_entry(loaded.text, index, &entry)?,
            None => append_entry(loaded.text, &entry)?,
        };
        write_atomic(path, &text)?;
    }
    if verbosity.results() && existing.is_none() {
        let repospec = local_path
            .map(display_path)
            .unwrap_or_else(|| entry.repospec.clone());
        eprintln!("{}", tracked_message(&repospec, path));
    }
    Ok(())
}

fn execute_jobs(
    jobs: Vec<InstallJob>,
    job_count: usize,
    verbosity: Verbosity,
    initial_results: Vec<InstallJobResult>,
    interrupted: &InterruptState,
) -> KitExecution {
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
                        .expect("kit work queue should not be poisoned")
                        .pop_front();
                    let Some(job) = job else {
                        break;
                    };
                    if interrupted.is_interrupted() {
                        break;
                    }
                    let result = execute_job(&job, verbosity, interrupted);
                    results
                        .lock()
                        .expect("kit results should not be poisoned")
                        .push(result);
                }
            });
        }
    });

    let mut results = results
        .into_inner()
        .expect("kit results should not be poisoned");
    results.sort_by_key(|result| result.number);
    let failed = results.iter().any(|result| !result.succeeded);
    let mut state_updates = Vec::new();
    for mut result in results {
        if let Some(update) = result.state_update.take() {
            state_updates.push(update);
        }
        for message in result.messages {
            message.emit();
        }
    }
    KitExecution {
        failed,
        interrupted: interrupted.is_interrupted(),
        state_updates,
    }
}

fn execute_job(
    job: &InstallJob,
    verbosity: Verbosity,
    interrupted: &InterruptState,
) -> InstallJobResult {
    let mut messages = Vec::new();
    let execution = job
        .plan
        .execute_with_replay(verbosity, &mut messages, interrupted);
    let mut state_update = None;
    let succeeded = if interrupted.is_interrupted() {
        false
    } else {
        match execution {
            Ok(0) => {
                let installed = successful_install_state(
                    &job.plan,
                    job.before_revision.clone(),
                    job.request.clone(),
                    |error| {
                        let warning = cache_warning(job.number, &job.repospec, &error);
                        eprintln!("{warning}");
                        messages.push(DtrMessage::Stderr(warning));
                    },
                );
                state_update = Some(StateUpdate {
                    entry: job.entry.clone(),
                    installed,
                });
                true
            }
            Ok(code) => {
                let warning = format!(
                    "dtr: warning: kit entry {} ({:?}) exited with status {code}",
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
        state_update,
    }
}

fn selected_file_path(
    alternate: Option<PathBuf>,
    migrate_legacy: bool,
) -> Result<PathBuf, DtrError> {
    let Some(path) = alternate else {
        return if migrate_legacy {
            config::migrate_legacy_kit_file()
        } else {
            config::kit_file_path()
        };
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
            "kit configuration {} must contain at least one [[install]] entry",
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
                config: KitConfig::default(),
                text: String::new(),
            });
        }
        Err(error) => {
            return Err(DtrError::new(format!(
                "could not read kit configuration {}: {error}",
                path.display()
            )));
        }
    };
    let config: KitConfig = toml::from_str(&text).map_err(|error| {
        DtrError::new(format!(
            "could not parse kit configuration {}: {error}",
            path.display()
        ))
    })?;
    validate_unique_repositories(&config, path, home::home_dir().as_deref())?;
    Ok(LoadedConfig { config, text })
}

fn validate_unique_repositories(
    config: &KitConfig,
    path: &Path,
    home: Option<&Path>,
) -> Result<(), DtrError> {
    let mut seen = HashMap::new();
    for (offset, entry) in config.install.iter().enumerate() {
        let Ok(identity) = normalized_repository_identity(entry, home) else {
            // Repospec errors remain per-entry planning errors so one bad entry
            // does not prevent the rest of a kit from running.
            continue;
        };
        if let Some((first_offset, first_repospec)) =
            seen.insert(identity, (offset, entry.repospec.as_str()))
        {
            return Err(DtrError::new(format!(
                "kit configuration {} has duplicate repositories: [[install]] entries {} ({first_repospec:?}) and {} ({:?}) have the same normalized repospec",
                path.display(),
                first_offset + 1,
                offset + 1,
                entry.repospec
            )));
        }
    }
    Ok(())
}

fn normalized_repository_identity(
    entry: &InstallEntry,
    home: Option<&Path>,
) -> Result<RepositoryIdentity, DtrError> {
    let repospec = expand_home(entry.repospec.clone(), home)?;
    let identity = InstallSource::parse(&repospec)?.spec.repository_identity();
    Ok(match identity {
        RepositoryIdentity::Local(path) => RepositoryIdentity::Local(normalize_local_path(&path)),
        identity => identity,
    })
}

fn normalize_local_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }

    let path = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(current) = env::current_dir() {
        current.join(path)
    } else {
        path.to_path_buf()
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
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
    if entry.main {
        arguments.push("--main".into());
    }
    arguments.push(expand_home(entry.repospec.clone(), home)?);
    if !entry.args.is_empty() {
        arguments.push("--".into());
        arguments.extend(entry.args.iter().cloned().map(OsString::from));
    }
    Ok(arguments
        .iter()
        .map(|argument| shell_quote_argument(argument))
        .collect::<Vec<_>>()
        .join(" "))
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

fn same_install_request(left: &InstallEntry, right: &InstallEntry) -> bool {
    left.repospec == right.repospec
        && left.tool == right.tool
        && left.no_latest == right.no_latest
        && left.main == right.main
        && left.args == right.args
}

fn request_fingerprint(entry: &InstallEntry, config: &Config) -> String {
    let mut digest = Sha256::new();
    digest.update(b"dtr-kit-install-request-v1");
    fingerprint_component(&mut digest, b"repospec");
    fingerprint_component(&mut digest, entry.repospec.as_bytes());
    fingerprint_component(&mut digest, b"tool");
    fingerprint_component(&mut digest, entry.tool.to_string().as_bytes());
    fingerprint_component(&mut digest, b"no_latest");
    fingerprint_component(&mut digest, &[u8::from(entry.no_latest)]);
    if entry.main {
        fingerprint_component(&mut digest, b"main");
        fingerprint_component(&mut digest, &[1]);
    }
    fingerprint_component(&mut digest, b"args");
    fingerprint_component(
        &mut digest,
        &u64::try_from(entry.args.len())
            .unwrap_or(u64::MAX)
            .to_be_bytes(),
    );
    for argument in &entry.args {
        fingerprint_component(&mut digest, argument.as_bytes());
    }
    if matches!(entry.tool, InstallTool::Auto | InstallTool::Uv) {
        let options = config.uv_install_options();
        fingerprint_component(&mut digest, b"uv_install_options");
        fingerprint_component(
            &mut digest,
            &u64::try_from(options.len())
                .unwrap_or(u64::MAX)
                .to_be_bytes(),
        );
        for option in options {
            fingerprint_component(&mut digest, option.as_bytes());
        }
    }
    format!("sha256:{:x}", digest.finalize())
}

fn fingerprint_component(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(u64::try_from(bytes.len()).unwrap_or(u64::MAX).to_be_bytes());
    digest.update(bytes);
}

fn probe_install_revision(plan: &CommandPlan) -> Result<Option<ProbedRevision>, String> {
    match plan.install_revision.as_ref() {
        Some(InstallRevision::LocalGit { path }) => probe_local_revision(path),
        Some(InstallRevision::RemoteGit { remote, reference }) => {
            probe_remote_revision(plan, remote, reference)
        }
        Some(InstallRevision::GoModule { query }) => probe_go_module_revision(plan, query),
        None => Ok(None),
    }
}

fn probe_local_revision(path: &Path) -> Result<Option<ProbedRevision>, String> {
    if !path
        .ancestors()
        .any(|ancestor| ancestor.join(".git").exists())
    {
        return Ok(None);
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--verify", "HEAD"])
        .output()
        .map_err(|error| format!("could not start git for local revision inspection: {error}"))?;
    if !output.status.success() {
        return Err("git rev-parse could not resolve the local repository HEAD".to_owned());
    }
    let commit = parse_single_object_id(&output.stdout)
        .ok_or_else(|| "git rev-parse did not return one full object ID".to_owned())?;

    let branch_output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["symbolic-ref", "--short", "-q", "HEAD"])
        .output()
        .map_err(|error| format!("could not start git for branch inspection: {error}"))?;
    let branch = if branch_output.status.success() {
        let text = std::str::from_utf8(&branch_output.stdout)
            .map_err(|_| "git symbolic-ref returned non-UTF-8 branch name".to_owned())?
            .trim();
        if text.is_empty() {
            None
        } else {
            Some(text.to_owned())
        }
    } else {
        None
    };

    let status = Command::new("git")
        .arg("-C")
        .arg(path)
        .args([
            "status",
            "--porcelain=v1",
            "--untracked-files=normal",
            "--ignore-submodules=none",
        ])
        .output()
        .map_err(|error| format!("could not start git for worktree inspection: {error}"))?;
    if !status.status.success() {
        return Err("git status could not inspect the local worktree".to_owned());
    }
    let dirty = !status.stdout.is_empty();
    Ok(Some(ProbedRevision {
        commit,
        branch,
        dirty,
    }))
}

fn probe_remote_revision(
    plan: &CommandPlan,
    remote: &std::ffi::OsStr,
    reference: &GitReference,
) -> Result<Option<ProbedRevision>, String> {
    if *reference == GitReference::Unsupported {
        return Ok(None);
    }
    if let GitReference::Revision(revision) = reference
        && revision.to_str().is_some_and(is_full_object_id)
    {
        return Ok(Some(ProbedRevision {
            commit: revision.to_string_lossy().to_ascii_lowercase(),
            branch: None,
            dirty: false,
        }));
    }

    let patterns = match reference {
        GitReference::Head => vec![std::ffi::OsString::from("HEAD")],
        GitReference::Branch(branch) => {
            vec![format!("refs/heads/{}", branch.to_string_lossy()).into()]
        }
        GitReference::Tag(tag) => vec![
            format!("refs/tags/{}", tag.to_string_lossy()).into(),
            format!("refs/tags/{}^{{}}", tag.to_string_lossy()).into(),
        ],
        GitReference::Revision(_) => Vec::new(),
        GitReference::Unsupported => unreachable!(),
    };
    let mut command = Command::new("git");
    command.args([
        std::ffi::OsStr::new("ls-remote"),
        std::ffi::OsStr::new("--"),
    ]);
    command.arg(remote).args(&patterns);
    apply_environment(&mut command, &plan.environment, &plan.removed_environment);
    let output = command
        .output()
        .map_err(|error| format!("could not start git for remote revision inspection: {error}"))?;
    if !output.status.success() {
        return Err("git ls-remote could not inspect the selected remote revision".to_owned());
    }
    let commit = resolve_ls_remote_output(&output.stdout, reference)?;
    Ok(commit.map(|commit| {
        let branch = match reference {
            GitReference::Branch(branch) => Some(branch.to_string_lossy().into_owned()),
            _ => None,
        };
        ProbedRevision {
            commit,
            branch,
            dirty: false,
        }
    }))
}

fn resolve_ls_remote_output(
    output: &[u8],
    reference: &GitReference,
) -> Result<Option<String>, String> {
    let text = std::str::from_utf8(output)
        .map_err(|_| "git ls-remote returned non-UTF-8 output".to_owned())?;
    let refs = text
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(object, _)| is_full_object_id(object))
        .collect::<Vec<_>>();
    let selected = match reference {
        GitReference::Head => refs
            .iter()
            .find(|(_, name)| *name == "HEAD")
            .map(|(object, _)| object.to_ascii_lowercase()),
        GitReference::Branch(branch) => {
            let name = format!("refs/heads/{}", branch.to_string_lossy());
            refs.iter()
                .find(|(_, candidate)| *candidate == name)
                .map(|(object, _)| object.to_ascii_lowercase())
        }
        GitReference::Tag(tag) => {
            let peeled = format!("refs/tags/{}^{{}}", tag.to_string_lossy());
            let direct = format!("refs/tags/{}", tag.to_string_lossy());
            refs.iter()
                .find(|(_, candidate)| *candidate == peeled)
                .or_else(|| refs.iter().find(|(_, candidate)| *candidate == direct))
                .map(|(object, _)| object.to_ascii_lowercase())
        }
        GitReference::Revision(revision) => {
            let Some(prefix) = revision.to_str().map(str::to_ascii_lowercase) else {
                return Ok(None);
            };
            let matches = refs
                .iter()
                .map(|(object, _)| object.to_ascii_lowercase())
                .filter(|object| object.starts_with(&prefix))
                .collect::<HashSet<_>>();
            (matches.len() == 1).then(|| matches.into_iter().next().expect("one object ID"))
        }
        GitReference::Unsupported => None,
    };
    Ok(selected)
}

#[derive(Deserialize)]
struct GoModuleResolution {
    #[serde(rename = "Origin")]
    origin: Option<GoModuleOrigin>,
}

#[derive(Deserialize)]
struct GoModuleOrigin {
    #[serde(rename = "VCS")]
    vcs: Option<String>,
    #[serde(rename = "Hash")]
    hash: Option<String>,
}

fn probe_go_module_revision(
    plan: &CommandPlan,
    query: &std::ffi::OsStr,
) -> Result<Option<ProbedRevision>, String> {
    let mut command = Command::new(&plan.executable);
    command.args([
        std::ffi::OsStr::new("list"),
        std::ffi::OsStr::new("-m"),
        std::ffi::OsStr::new("-json"),
    ]);
    command.arg(query);
    apply_environment(&mut command, &plan.environment, &plan.removed_environment);
    let output = command
        .output()
        .map_err(|error| format!("could not start go for module revision inspection: {error}"))?;
    if !output.status.success() {
        return Err("go list could not resolve the selected module version".to_owned());
    }
    let module: GoModuleResolution = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("go list returned invalid module metadata: {error}"))?;
    let Some(origin) = module
        .origin
        .filter(|origin| origin.vcs.as_deref() == Some("git"))
    else {
        return Ok(None);
    };
    Ok(origin
        .hash
        .filter(|hash| is_full_object_id(hash))
        .map(|hash| ProbedRevision {
            commit: hash.to_ascii_lowercase(),
            branch: None,
            dirty: false,
        }))
}

fn parse_single_object_id(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?.trim();
    is_full_object_id(text).then(|| text.to_ascii_lowercase())
}

fn is_full_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn successful_install_state(
    plan: &CommandPlan,
    before_revision: Option<ProbedRevision>,
    request: String,
    mut warn: impl FnMut(String),
) -> Option<InstalledState> {
    let before_revision = before_revision?;
    match probe_install_revision(plan) {
        Ok(Some(after_revision)) if after_revision == before_revision => Some(InstalledState {
            commit: after_revision.commit,
            branch: after_revision.branch,
            dirty: after_revision.dirty,
            request,
        }),
        Ok(Some(_)) => {
            warn("the selected Git revision changed while the installer was running".to_owned());
            None
        }
        Ok(None) => {
            warn("the installed Git revision could no longer be verified".to_owned());
            None
        }
        Err(error) => {
            warn(error);
            None
        }
    }
}

fn short_revision(revision: &str) -> &str {
    revision.get(..12).unwrap_or(revision)
}

fn cache_warning(number: usize, repospec: &str, error: &str) -> String {
    format!("dtr: warning: kit entry {number} ({repospec:?}) install cache: {error}")
}

fn persist_state_updates(
    path: &Path,
    updates: Vec<StateUpdate>,
    home: Option<&Path>,
) -> Result<(), DtrError> {
    if updates.is_empty() {
        return Ok(());
    }
    let mut loaded = load_required(path)?;
    let mut changed = false;
    for update in updates {
        let identity = normalized_repository_identity(&update.entry, home)?;
        let Some(index) = loaded.config.install.iter().position(|entry| {
            normalized_repository_identity(entry, home).is_ok_and(|candidate| candidate == identity)
        }) else {
            eprintln!(
                "dtr: warning: did not refresh install state for {:?} because its kit entry was removed",
                update.entry.repospec
            );
            continue;
        };
        if !same_install_request(&loaded.config.install[index], &update.entry) {
            eprintln!(
                "dtr: warning: did not refresh install state for {:?} because its kit entry changed",
                update.entry.repospec
            );
            continue;
        }
        if loaded.config.install[index].installed == update.installed {
            continue;
        }
        loaded.config.install[index].installed = update.installed;
        loaded.text = replace_entry(loaded.text, index, &loaded.config.install[index])?;
        changed = true;
    }
    if changed {
        write_atomic(path, &loaded.text)?;
    }
    Ok(())
}

fn tracked_local_path(path: &Path, home: Option<&Path>) -> Result<String, DtrError> {
    #[cfg(not(windows))]
    if let Some(home) = home {
        if path == home {
            return Ok("~".to_owned());
        }
        if let Ok(suffix) = path.strip_prefix(home) {
            let suffix = path_text(suffix)?;
            return Ok(format!("~/{suffix}"));
        }
    }
    #[cfg(windows)]
    let _ = home;
    path_text(path)
}

fn path_text(path: &Path) -> Result<String, DtrError> {
    friendly_path(path)
        .into_string()
        .map_err(|_| DtrError::new("kit.toml cannot store a non-UTF-8 local repository path"))
}

fn tracked_message(repospec: &str, config_path: &Path) -> String {
    format!("tracked: {repospec} → {}", display_path(config_path))
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
    text.push_str(
        &toml::to_string(entry)
            .map_err(|error| DtrError::new(format!("could not serialize kit entry: {error}")))?,
    );
    if let Some(installed) = &entry.installed {
        text.push_str("installed = ");
        text.push_str(&installed_value(installed).to_string());
        text.push('\n');
    }
    Ok(text)
}

fn replace_entry(text: String, index: usize, entry: &InstallEntry) -> Result<String, DtrError> {
    let mut document = text.parse::<DocumentMut>().map_err(|error| {
        DtrError::new(format!(
            "could not edit the existing kit configuration: {error}"
        ))
    })?;
    let table = document
        .get_mut("install")
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|entries| entries.get_mut(index))
        .ok_or_else(|| DtrError::new("could not locate the existing kit entry to update"))?;

    set_table_value(table, "repospec", Value::from(entry.repospec.as_str()));
    set_optional_table_value(
        table,
        "tool",
        (entry.tool != InstallTool::Auto).then(|| Value::from(entry.tool.to_string())),
    );
    set_optional_table_value(
        table,
        "no_latest",
        entry.no_latest.then(|| Value::from(true)),
    );
    set_optional_table_value(table, "main", entry.main.then(|| Value::from(true)));
    set_optional_table_value(
        table,
        "args",
        (!entry.args.is_empty())
            .then(|| Value::Array(Array::from_iter(entry.args.iter().map(String::as_str)))),
    );
    set_optional_table_value(
        table,
        "installed",
        entry.installed.as_ref().map(installed_value),
    );
    Ok(document.to_string())
}

fn installed_value(installed: &InstalledState) -> Value {
    let mut table = InlineTable::new();
    table.insert("commit", Value::from(installed.commit.as_str()));
    if let Some(branch) = &installed.branch {
        table.insert("branch", Value::from(branch.as_str()));
    }
    if installed.dirty {
        table.insert("dirty", Value::from(true));
    }
    table.insert("request", Value::from(installed.request.as_str()));
    Value::InlineTable(table)
}

fn set_optional_table_value(table: &mut Table, key: &str, new_value: Option<Value>) {
    if let Some(new_value) = new_value {
        set_table_value(table, key, new_value);
    } else {
        table.remove(key);
    }
}

fn set_table_value(table: &mut Table, key: &str, mut new_value: Value) {
    if let Some(item) = table.get_mut(key) {
        if let Some(old_value) = item.as_value() {
            *new_value.decor_mut() = old_value.decor().clone();
        }
        *item = Item::Value(new_value);
    } else {
        table.insert(key, value(new_value));
    }
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
            "could not replace kit configuration {}: {}",
            path.display(),
            error.error
        ))
    })?;
    Ok(())
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
    format!("dtr: warning: kit entry {number} ({repospec:?}): {error}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn config_accepts_native_args_and_rust_alias() {
        let config: KitConfig = toml::from_str(
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
        let error = toml::from_str::<KitConfig>(
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
            main: false,
            args: Vec::new(),
            installed: None,
        };
        assert_eq!(
            append_entry("# keep this comment\n".to_owned(), &entry).unwrap(),
            "# keep this comment\n\n[[install]]\nrepospec = \"~/p/my/tool\"\n"
        );
    }

    #[test]
    fn installed_state_round_trips_as_an_inline_generated_table() {
        let entry = InstallEntry {
            repospec: "owner/tool".to_owned(),
            tool: InstallTool::Cargo,
            no_latest: false,
            main: false,
            args: Vec::new(),
            installed: Some(InstalledState {
                commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                branch: Some("main".to_owned()),
                dirty: true,
                request: format!("sha256:{}", "a".repeat(64)),
            }),
        };
        let text = append_entry(String::new(), &entry).unwrap();
        assert!(
            text.contains(
                "installed = { commit = \"0123456789abcdef0123456789abcdef01234567\", branch = \"main\", dirty = true, request = \"sha256:"
            ),
            "{text}"
        );
        let parsed: KitConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed.install[0], entry);

        let clean_entry = InstallEntry {
            repospec: "owner/tool".to_owned(),
            tool: InstallTool::Cargo,
            no_latest: false,
            main: false,
            args: Vec::new(),
            installed: Some(InstalledState {
                commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                branch: Some("feature/foo".to_owned()),
                dirty: false,
                request: format!("sha256:{}", "a".repeat(64)),
            }),
        };
        let clean_text = append_entry(String::new(), &clean_entry).unwrap();
        assert!(
            clean_text.contains(
                "installed = { commit = \"0123456789abcdef0123456789abcdef01234567\", branch = \"feature/foo\", request = \"sha256:"
            ),
            "{clean_text}"
        );
        assert!(!clean_text.contains("dirty"), "{clean_text}");
        let clean_parsed: KitConfig = toml::from_str(&clean_text).unwrap();
        assert_eq!(clean_parsed.install[0], clean_entry);

        let detached_entry = InstallEntry {
            repospec: "owner/tool".to_owned(),
            tool: InstallTool::Cargo,
            no_latest: false,
            main: false,
            args: Vec::new(),
            installed: Some(InstalledState {
                commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
                branch: None,
                dirty: false,
                request: format!("sha256:{}", "a".repeat(64)),
            }),
        };
        let detached_text = append_entry(String::new(), &detached_entry).unwrap();
        assert!(
            detached_text.contains(
                "installed = { commit = \"0123456789abcdef0123456789abcdef01234567\", request = \"sha256:"
            ),
            "{detached_text}"
        );
        assert!(!detached_text.contains("branch"), "{detached_text}");
        assert!(!detached_text.contains("dirty"), "{detached_text}");
        let detached_parsed: KitConfig = toml::from_str(&detached_text).unwrap();
        assert_eq!(detached_parsed.install[0], detached_entry);
    }

    #[test]
    fn remote_ref_resolution_prefers_peeled_tags_and_requires_unique_revisions() {
        const DIRECT: &str = "1111111111111111111111111111111111111111";
        const PEELED: &str = "2222222222222222222222222222222222222222";
        let output = format!(
            "{DIRECT}\trefs/tags/v1\n{PEELED}\trefs/tags/v1^{{}}\n{PEELED}\trefs/heads/main\n"
        );
        assert_eq!(
            resolve_ls_remote_output(output.as_bytes(), &GitReference::Tag("v1".into())).unwrap(),
            Some(PEELED.to_owned())
        );
        assert_eq!(
            resolve_ls_remote_output(output.as_bytes(), &GitReference::Branch("main".into()))
                .unwrap(),
            Some(PEELED.to_owned())
        );
        assert_eq!(
            resolve_ls_remote_output(
                output.as_bytes(),
                &GitReference::Revision("22222222".into())
            )
            .unwrap(),
            Some(PEELED.to_owned())
        );
        assert_eq!(
            resolve_ls_remote_output(
                format!("{DIRECT}\trefs/heads/one\n{PEELED}\trefs/heads/two\n").as_bytes(),
                &GitReference::Revision("2".into())
            )
            .unwrap(),
            Some(PEELED.to_owned())
        );
    }

    #[cfg(not(windows))]
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
        let home = Path::new("/home/user");
        let entry = InstallEntry {
            repospec: "~/p/my/tool with spaces".to_owned(),
            tool: InstallTool::Cargo,
            no_latest: false,
            main: false,
            args: vec!["--features".to_owned(), "pcre2".to_owned()],
            installed: None,
        };
        let repository = home.join("p/my/tool with spaces");
        assert_eq!(
            render_install_command(&entry, Some(home)).unwrap(),
            format!(
                "dtr install --tool cargo {} -- --features pcre2",
                shell_quote_argument(repository.as_os_str())
            )
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_tracking_narration_uses_friendly_native_paths() {
        let repository = Path::new(r"\\?\C:\Users\mclark\p\my\gitoverit");
        let home = Path::new(r"\\?\C:\Users\mclark");
        let config = Path::new(r"C:\Users\mclark\.config\dtr\kit.toml");

        assert_eq!(
            tracked_local_path(repository, Some(home)).unwrap(),
            r"C:\Users\mclark\p\my\gitoverit"
        );
        let entry = InstallEntry {
            repospec: tracked_local_path(repository, Some(home)).unwrap(),
            tool: InstallTool::Cargo,
            no_latest: false,
            main: false,
            args: Vec::new(),
            installed: None,
        };
        assert_eq!(
            append_entry(String::new(), &entry).unwrap(),
            "[[install]]\nrepospec = 'C:\\Users\\mclark\\p\\my\\gitoverit'\ntool = \"cargo\"\n"
        );
        assert_eq!(
            tracked_message(&display_path(repository), config),
            r"tracked: C:\Users\mclark\p\my\gitoverit → C:\Users\mclark\.config\dtr\kit.toml"
        );
    }
}
