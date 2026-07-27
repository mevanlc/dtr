use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use crate::cli::{InstallArgs, InstallTool};
use crate::clone_args::{CloneRequest, NameMode};
use crate::command::{CommandPlan, SecretEnvironment, command_exists};
use crate::error::DtrError;
use crate::github_auth;
use crate::github_auth::GithubAuthSelection;
use crate::install_detect;
use crate::repospec::{Forge, InstallSource, RepoSpec};

pub(crate) fn plan_clone(request: CloneRequest) -> Result<CommandPlan, DtrError> {
    if request.name_mode != NameMode::Default
        && request.spec.forge_parts().is_none()
        && !matches!(request.spec, RepoSpec::GithubMine { .. })
    {
        return Err(DtrError::new(
            "-O/--name-owner and -D apply only to recognized GitHub or GitLab repositories",
        ));
    }

    if request.upstream_remote_name.is_some()
        && !matches!(
            &request.spec,
            RepoSpec::GithubMine { .. }
                | RepoSpec::Forge {
                    forge: Forge::GitHub,
                    ..
                }
        )
    {
        return Err(DtrError::new(
            "-U/--upstream-remote-name applies only to recognized GitHub repositories",
        ));
    }

    let (target_dir, pass_target, preparations) = clone_target(&request)?;
    let repospec = request.spec.description();

    match &request.spec {
        RepoSpec::GithubMine { repo } => {
            require_command("gh", "cloning a bare GitHub repository name")?;
            Ok(forge_clone_plan(
                "gh",
                repo.clone().into(),
                request.upstream_remote_name,
                request.git_options,
                target_dir,
                pass_target,
                preparations,
                repospec,
                None,
                Vec::new(),
                Vec::new(),
            ))
        }
        RepoSpec::Forge {
            forge,
            host,
            namespace,
            repo,
            remote,
        } => {
            let preferred = match forge {
                Forge::GitHub => "gh",
                Forge::GitLab => "glab",
            };
            if command_exists(preferred) {
                let mut repository = remote
                    .clone()
                    .unwrap_or_else(|| request.spec.forge_slug().expect("forge slug").into());
                let (auth, environment, removed_environment) = if *forge == Forge::GitHub {
                    if let Some(selection) = github_auth::select_for_owner(&namespace[0])? {
                        repository =
                            format!("https://{host}/{}/{}.git", namespace.join("/"), repo).into();
                        let auth = format!(
                            "auto-switch to {} (process-scoped; active gh account unchanged)",
                            selection.account
                        );
                        (
                            Some(auth),
                            vec![SecretEnvironment::new("GH_TOKEN", selection.token)],
                            vec![OsString::from("GITHUB_TOKEN")],
                        )
                    } else {
                        (None, Vec::new(), Vec::new())
                    }
                } else {
                    (None, Vec::new(), Vec::new())
                };
                Ok(forge_clone_plan(
                    preferred,
                    repository,
                    request.upstream_remote_name,
                    request.git_options,
                    target_dir,
                    pass_target,
                    preparations,
                    repospec,
                    auth,
                    environment,
                    removed_environment,
                ))
            } else {
                if request.upstream_remote_name.is_some() {
                    require_command("gh", "using -U/--upstream-remote-name")?;
                }
                require_command("git", "cloning a repository")?;
                let remote = request.spec.git_remote()?;
                Ok(git_clone_plan(
                    remote,
                    request.git_options,
                    target_dir,
                    pass_target,
                    preparations,
                    repospec,
                ))
            }
        }
        RepoSpec::Local { .. } | RepoSpec::GitUrl { .. } | RepoSpec::ScpLike { .. } => {
            require_command("git", "cloning a repository")?;
            let remote = request.spec.git_remote()?;
            Ok(git_clone_plan(
                remote,
                request.git_options,
                target_dir,
                pass_target,
                preparations,
                repospec,
            ))
        }
    }
}

pub(crate) fn plan_install(args: InstallArgs) -> Result<CommandPlan, DtrError> {
    let requested_tool = args.tool;
    let InstallSource { spec, go_query } = InstallSource::parse(&args.repospec)?;
    if go_query.is_some() && args.no_latest {
        return Err(DtrError::new(
            "an explicit Go version query conflicts with --no-latest",
        ));
    }
    if let Some(query) = &go_query
        && !matches!(requested_tool, InstallTool::Auto | InstallTool::Go)
    {
        return Err(DtrError::new(format!(
            "Go version query @{query} requires --tool go, but {requested_tool} was selected"
        )));
    }
    let github_owner = if matches!(spec, RepoSpec::GithubMine { .. }) {
        Some(resolve_github_owner()?)
    } else {
        None
    };
    let github_selection = if requested_tool == InstallTool::Auto {
        GithubSelection::Resolved(select_github_auth(&spec)?)
    } else {
        GithubSelection::Unresolved
    };
    let selected_tool = if requested_tool == InstallTool::Auto {
        install_detect::detect_tool(&spec, github_owner.as_deref(), github_selection.selection())?
    } else {
        requested_tool
    };
    if let Some(query) = &go_query
        && selected_tool != InstallTool::Go
    {
        return Err(DtrError::new(format!(
            "Go version query @{query} requires --tool go, but {selected_tool} was selected"
        )));
    }
    let context = InstallContext {
        args,
        spec,
        go_query,
        github_owner,
        github_selection,
    };

    match selected_tool {
        InstallTool::Go => plan_go_install(context),
        InstallTool::Cargo => plan_cargo_install(context),
        InstallTool::Uv => plan_python_install(context, PythonBackend::Uv),
        InstallTool::Pipx => plan_python_install(context, PythonBackend::Pipx),
        InstallTool::Npm => plan_npm_install(context),
        InstallTool::Auto => unreachable!("auto tool was resolved before backend planning"),
    }
}

struct InstallContext {
    args: InstallArgs,
    spec: RepoSpec,
    go_query: Option<String>,
    github_owner: Option<String>,
    github_selection: GithubSelection,
}

enum GithubSelection {
    Unresolved,
    Resolved(Option<GithubAuthSelection>),
}

impl GithubSelection {
    fn selection(&self) -> Option<&GithubAuthSelection> {
        match self {
            Self::Unresolved | Self::Resolved(None) => None,
            Self::Resolved(Some(selection)) => Some(selection),
        }
    }

    fn resolve(self, spec: &RepoSpec) -> Result<Option<GithubAuthSelection>, DtrError> {
        match self {
            Self::Unresolved => select_github_auth(spec),
            Self::Resolved(selection) => Ok(selection),
        }
    }
}

fn select_github_auth(spec: &RepoSpec) -> Result<Option<GithubAuthSelection>, DtrError> {
    match spec {
        RepoSpec::Forge {
            forge: Forge::GitHub,
            namespace,
            ..
        } => github_auth::select_for_owner(&namespace[0]),
        _ => Ok(None),
    }
}

fn plan_go_install(context: InstallContext) -> Result<CommandPlan, DtrError> {
    let InstallContext {
        args,
        spec,
        go_query,
        github_owner,
        ..
    } = context;
    if !args.install_args.is_empty() {
        return Err(DtrError::new(
            "arguments after -- are supported only by the Cargo, uv, pipx, and npm installers",
        ));
    }
    require_command("go", "installing a Go command")?;
    let repospec = spec.description();

    if spec.is_local() {
        if args.no_latest {
            return Err(DtrError::new(
                "--no-latest applies only to remote Go repository installs",
            ));
        }
        let directory = spec
            .local_path()
            .expect("local spec has a path")
            .to_path_buf();
        return Ok(CommandPlan {
            program: "go".into(),
            args: ["install", "./..."].map(OsString::from).to_vec(),
            current_dir: Some(directory),
            target_dir: None,
            preparations: Vec::new(),
            repospec,
            backend: "go",
            auth: None,
            environment: Vec::new(),
            removed_environment: Vec::new(),
        });
    }

    let mut import_path = spec.go_import_path(github_owner.as_deref())?;
    if let Some(query) = go_query {
        import_path.push('@');
        import_path.push_str(&query);
    } else if !args.no_latest {
        import_path.push_str("@latest");
    }

    Ok(CommandPlan {
        program: "go".into(),
        args: vec!["install".into(), import_path.into()],
        current_dir: None,
        target_dir: None,
        preparations: Vec::new(),
        repospec,
        backend: "go",
        auth: None,
        environment: Vec::new(),
        removed_environment: Vec::new(),
    })
}

fn plan_cargo_install(context: InstallContext) -> Result<CommandPlan, DtrError> {
    let InstallContext {
        args,
        spec,
        github_owner,
        github_selection,
        ..
    } = context;
    if args.no_latest {
        return Err(DtrError::new(
            "--no-latest applies only to the Go installer",
        ));
    }
    reject_cargo_source_arguments(&args.install_args)?;
    require_command("cargo", "installing a Rust binary")?;

    let repospec = spec.description();
    let mut command_args = vec![OsString::from("install")];
    let mut auth = None;
    let mut environment = Vec::new();
    let mut removed_environment = Vec::new();

    if let Some(path) = spec.local_path() {
        command_args.push("--path".into());
        command_args.push(path.as_os_str().to_os_string());
    } else {
        let remote = spec.install_git_remote(github_owner.as_deref())?;
        command_args.push("--git".into());
        command_args.push(remote);

        if let Some(selection) = github_selection.resolve(&spec)? {
            auth = Some(format!(
                "auto-switch to {} (process-scoped; active gh account unchanged)",
                selection.account
            ));
            (environment, removed_environment) =
                github_auth::cargo_git_environment(&selection.token)?;
        }
    }
    command_args.extend(args.install_args);

    Ok(CommandPlan {
        program: "cargo".into(),
        args: command_args,
        current_dir: None,
        target_dir: None,
        preparations: Vec::new(),
        repospec,
        backend: "cargo",
        auth,
        environment,
        removed_environment,
    })
}

#[derive(Clone, Copy)]
enum PythonBackend {
    Uv,
    Pipx,
}

impl PythonBackend {
    fn program(self) -> &'static str {
        match self {
            Self::Uv => "uv",
            Self::Pipx => "pipx",
        }
    }

    fn purpose(self) -> &'static str {
        match self {
            Self::Uv => "installing a Python tool with uv",
            Self::Pipx => "installing a Python tool with pipx",
        }
    }
}

fn plan_python_install(
    context: InstallContext,
    backend: PythonBackend,
) -> Result<CommandPlan, DtrError> {
    let InstallContext {
        args,
        spec,
        github_owner,
        github_selection,
        ..
    } = context;
    if args.no_latest {
        return Err(DtrError::new(
            "--no-latest applies only to the Go installer",
        ));
    }
    if matches!(backend, PythonBackend::Pipx) {
        validate_pipx_arguments(&args.install_args)?;
    }
    require_command(backend.program(), backend.purpose())?;

    let repospec = spec.description();
    let source = spec.python_package_source(github_owner.as_deref())?;
    let mut auth = None;
    let mut environment = Vec::new();
    let mut removed_environment = Vec::new();

    if let Some(selection) = github_selection.resolve(&spec)? {
        auth = Some(format!(
            "auto-switch to {} (process-scoped; active gh account unchanged)",
            selection.account
        ));
        (environment, removed_environment) = github_auth::python_git_environment(&selection.token)?;
    }

    let command_args = match backend {
        PythonBackend::Uv => {
            let mut command_args = vec!["tool".into(), "install".into(), source];
            command_args.extend(args.install_args);
            command_args
        }
        PythonBackend::Pipx => {
            let mut command_args = vec![OsString::from("install")];
            command_args.extend(args.install_args);
            command_args.extend([OsString::from("--"), source]);
            command_args
        }
    };

    Ok(CommandPlan {
        program: backend.program().into(),
        args: command_args,
        current_dir: None,
        target_dir: None,
        preparations: Vec::new(),
        repospec,
        backend: backend.program(),
        auth,
        environment,
        removed_environment,
    })
}

fn plan_npm_install(context: InstallContext) -> Result<CommandPlan, DtrError> {
    let InstallContext {
        args,
        spec,
        github_owner,
        github_selection,
        ..
    } = context;
    if args.no_latest {
        return Err(DtrError::new(
            "--no-latest applies only to the Go installer",
        ));
    }
    validate_npm_arguments(&args.install_args)?;
    require_command("npm", "installing a JavaScript tool with npm")?;

    let repospec = spec.description();
    let source = spec.npm_package_source(github_owner.as_deref())?;
    let mut auth = None;
    let mut environment = Vec::new();
    let mut removed_environment = Vec::new();

    if let Some(selection) = github_selection.resolve(&spec)? {
        auth = Some(format!(
            "auto-switch to {} (process-scoped; active gh account unchanged)",
            selection.account
        ));
        (environment, removed_environment) = github_auth::npm_git_environment(&selection.token)?;
    }

    let mut command_args = vec![OsString::from("install"), OsString::from("--global")];
    command_args.extend(args.install_args);
    command_args.extend([OsString::from("--"), source]);

    Ok(CommandPlan {
        program: "npm".into(),
        args: command_args,
        current_dir: None,
        target_dir: None,
        preparations: Vec::new(),
        repospec,
        backend: "npm",
        auth,
        environment,
        removed_environment,
    })
}

fn validate_pipx_arguments(arguments: &[OsString]) -> Result<(), DtrError> {
    for argument in arguments {
        let bytes = argument.as_encoded_bytes();
        if bytes.first() != Some(&b'-') {
            return Err(DtrError::new(
                "pipx option values after -- must use an attached spelling such as --python=3.14; a separate value could be another package spec",
            ));
        }
        if argument == "--" {
            return Err(DtrError::new(
                "a forwarded -- would bypass dtr's single-source pipx boundary",
            ));
        }
        if let Some(argument) = argument.to_str()
            && (argument == "--lock" || argument.starts_with("--lock="))
        {
            return Err(DtrError::new(
                "pipx source option --lock conflicts with dtr's resolved repository",
            ));
        }
    }
    Ok(())
}

fn validate_npm_arguments(arguments: &[OsString]) -> Result<(), DtrError> {
    for argument in arguments {
        let bytes = argument.as_encoded_bytes();
        if bytes.first() != Some(&b'-') {
            return Err(DtrError::new(
                "npm option values after -- must use an attached spelling such as --prefix=/opt/npm; a separate value could be another package spec",
            ));
        }
        if argument == "--" {
            return Err(DtrError::new(
                "a forwarded -- would bypass dtr's single-source npm boundary",
            ));
        }
        if let Some(argument) = argument.to_str()
            && (matches!(argument, "-g" | "--global" | "--no-global")
                || argument.starts_with("-g=")
                || argument.starts_with("--global=")
                || argument.starts_with("--no-global="))
        {
            return Err(DtrError::new(
                "npm global-mode options conflict with dtr's global repository install",
            ));
        }
    }
    Ok(())
}

fn reject_cargo_source_arguments(arguments: &[OsString]) -> Result<(), DtrError> {
    const SOURCE_OPTIONS: [&str; 4] = ["--git", "--path", "--registry", "--index"];
    for argument in arguments {
        let Some(argument) = argument.to_str() else {
            continue;
        };
        if let Some(option) = SOURCE_OPTIONS
            .iter()
            .find(|option| argument == **option || argument.starts_with(&format!("{option}=")))
        {
            return Err(DtrError::new(format!(
                "Cargo source option {option} conflicts with dtr's resolved repository; remove it from the arguments after --"
            )));
        }
    }
    Ok(())
}

fn clone_target(request: &CloneRequest) -> Result<(PathBuf, bool, Vec<PathBuf>), DtrError> {
    if let Some(directory) = &request.directory {
        return Ok((PathBuf::from(directory), true, Vec::new()));
    }

    if request.name_mode == NameMode::Default {
        return Ok((request.spec.default_target()?, false, Vec::new()));
    }

    let (namespace, repo) = match &request.spec {
        RepoSpec::Forge {
            namespace, repo, ..
        } => (namespace.clone(), repo.clone()),
        RepoSpec::GithubMine { repo } => (vec![resolve_github_owner()?], repo.clone()),
        _ => unreachable!("name mode checked before target derivation"),
    };

    match request.name_mode {
        NameMode::NameOwner => {
            let mut components = namespace;
            components.push(repo);
            Ok((PathBuf::from(components.join("--")), true, Vec::new()))
        }
        NameMode::OwnerDirectory => {
            let mut target = PathBuf::new();
            for component in &namespace {
                target.push(component);
            }
            let parent = target.clone();
            target.push(repo);
            Ok((target, true, vec![parent]))
        }
        NameMode::Default => unreachable!(),
    }
}

#[allow(clippy::too_many_arguments)]
fn forge_clone_plan(
    program: &'static str,
    repository: OsString,
    upstream_remote_name: Option<OsString>,
    git_options: Vec<OsString>,
    target_dir: PathBuf,
    pass_target: bool,
    preparations: Vec<PathBuf>,
    repospec: String,
    auth: Option<String>,
    environment: Vec<SecretEnvironment>,
    removed_environment: Vec<OsString>,
) -> CommandPlan {
    let mut args = ["repo", "clone"].map(OsString::from).to_vec();
    if let Some(name) = upstream_remote_name {
        args.push("--upstream-remote-name".into());
        args.push(name);
    }
    args.push(repository);
    if pass_target {
        args.push(target_dir.as_os_str().to_os_string());
    }
    if !git_options.is_empty() {
        args.push("--".into());
        args.extend(git_options);
    }
    CommandPlan {
        program: program.into(),
        args,
        current_dir: None,
        target_dir: Some(target_dir),
        preparations,
        repospec,
        backend: program,
        auth,
        environment,
        removed_environment,
    }
}

fn git_clone_plan(
    remote: OsString,
    git_options: Vec<OsString>,
    target_dir: PathBuf,
    pass_target: bool,
    preparations: Vec<PathBuf>,
    repospec: String,
) -> CommandPlan {
    let mut args = vec![OsString::from("clone")];
    args.extend(git_options);
    // Terminate options with `--` so a dash-leading remote or destination
    // (e.g. an scp-like `-x:y` repospec) can never be parsed by git as a flag.
    args.push(OsString::from("--"));
    args.push(remote);
    if pass_target {
        args.push(target_dir.as_os_str().to_os_string());
    }
    CommandPlan {
        program: "git".into(),
        args,
        current_dir: None,
        target_dir: Some(target_dir),
        preparations,
        repospec,
        backend: "git",
        auth: None,
        environment: Vec::new(),
        removed_environment: Vec::new(),
    }
}

fn require_command(program: &str, purpose: &str) -> Result<(), DtrError> {
    if command_exists(program) {
        Ok(())
    } else {
        Err(DtrError::new(format!(
            "{program} is required for {purpose}, but was not found on PATH"
        )))
    }
}

fn resolve_github_owner() -> Result<String, DtrError> {
    require_command("gh", "resolving your GitHub repository")?;
    let output = Command::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output()
        .map_err(|error| DtrError::new(format!("could not start gh: {error}")))?;
    if !output.status.success() {
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        let diagnostic = diagnostic.trim();
        return Err(DtrError::new(if diagnostic.is_empty() {
            "could not resolve the current GitHub user with 'gh api user'".to_owned()
        } else {
            format!("could not resolve the current GitHub user: {diagnostic}")
        }));
    }
    let owner = String::from_utf8(output.stdout)
        .map_err(|_| DtrError::new("gh returned a non-UTF-8 GitHub username"))?;
    let owner = owner.trim();
    if owner.is_empty()
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(DtrError::new(
            "gh returned an empty or invalid GitHub username",
        ));
    }
    Ok(owner.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn github_nested_target_modes_are_exact() {
        let spec = RepoSpec::parse(OsStr::new("owner/repo")).unwrap();
        let name_owner = CloneRequest {
            spec: spec.clone(),
            directory: None,
            git_options: Vec::new(),
            name_mode: NameMode::NameOwner,
            upstream_remote_name: None,
        };
        let owner_dir = CloneRequest {
            spec,
            directory: None,
            git_options: Vec::new(),
            name_mode: NameMode::OwnerDirectory,
            upstream_remote_name: None,
        };
        assert_eq!(
            clone_target(&name_owner).unwrap().0,
            PathBuf::from("owner--repo")
        );
        let (target, _, preparations) = clone_target(&owner_dir).unwrap();
        assert_eq!(target, PathBuf::from("owner/repo"));
        assert_eq!(preparations, vec![PathBuf::from("owner")]);
    }

    #[test]
    fn nested_gitlab_target_modes_preserve_all_namespace_components() {
        let spec = RepoSpec::parse(OsStr::new("https://gitlab.com/group/subgroup/repo")).unwrap();
        let request = CloneRequest {
            spec,
            directory: None,
            git_options: Vec::new(),
            name_mode: NameMode::NameOwner,
            upstream_remote_name: None,
        };
        assert_eq!(
            clone_target(&request).unwrap().0,
            PathBuf::from("group--subgroup--repo")
        );
    }

    #[test]
    fn explicit_directory_wins_over_derived_name_mode() {
        let request = CloneRequest {
            spec: RepoSpec::parse(OsStr::new("owner/repo")).unwrap(),
            directory: Some("chosen".into()),
            git_options: Vec::new(),
            name_mode: NameMode::OwnerDirectory,
            upstream_remote_name: None,
        };
        let (target, pass, preparations) = clone_target(&request).unwrap();
        assert_eq!(target, PathBuf::from("chosen"));
        assert!(pass);
        assert!(preparations.is_empty());
    }

    #[test]
    fn bare_go_import_uses_resolved_owner() {
        let spec = RepoSpec::GithubMine {
            repo: "tool".to_owned(),
        };
        assert_eq!(
            spec.go_import_path(Some("mevanlc")).unwrap(),
            "github.com/mevanlc/tool"
        );
    }

    #[test]
    fn rejects_cargo_source_arguments_in_separated_and_equals_forms() {
        for argument in [
            "--git",
            "--git=https://example.com/repo",
            "--path",
            "--path=./repo",
            "--registry",
            "--registry=private",
            "--index",
            "--index=https://example.com/index",
        ] {
            assert!(
                reject_cargo_source_arguments(&[argument.into()]).is_err(),
                "{argument}"
            );
        }
        assert!(
            reject_cargo_source_arguments(
                ["--locked", "--bin", "tool"].map(OsString::from).as_ref()
            )
            .is_ok()
        );
    }

    #[test]
    fn pipx_arguments_preserve_only_unambiguous_options() {
        assert!(
            validate_pipx_arguments(
                ["--python=3.14", "--force", "-q"]
                    .map(OsString::from)
                    .as_ref()
            )
            .is_ok()
        );
        for argument in [
            "3.14",
            "another-package",
            "--",
            "--lock",
            "--lock=pylock.toml",
        ] {
            assert!(
                validate_pipx_arguments(&[argument.into()]).is_err(),
                "{argument}"
            );
        }
    }

    #[test]
    fn npm_arguments_preserve_only_unambiguous_non_global_options() {
        assert!(
            validate_npm_arguments(
                &["--prefix=/opt/npm", "--force", "--ignore-scripts"].map(OsString::from)
            )
            .is_ok()
        );
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
            assert!(
                validate_npm_arguments(&[argument.into()]).is_err(),
                "{argument}"
            );
        }
        assert!(validate_npm_arguments(&["--global-style".into()]).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn git_command_plan_preserves_non_utf8_repository_argv() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let remote = OsString::from_vec(b"./repo-\xff".to_vec());
        let plan = git_clone_plan(
            remote.clone(),
            Vec::new(),
            PathBuf::from(OsStr::from_bytes(b"repo-\xff")),
            false,
            Vec::new(),
            "local repository".to_owned(),
        );
        assert_eq!(
            plan.args,
            [OsString::from("clone"), OsString::from("--"), remote]
        );
    }
}
