use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use crate::cli::InstallArgs;
use crate::clone_args::{CloneRequest, NameMode};
use crate::command::{CommandPlan, command_exists};
use crate::error::DtrError;
use crate::repospec::{Forge, RepoSpec};

pub(crate) fn plan_clone(request: CloneRequest) -> Result<CommandPlan, DtrError> {
    if request.name_mode != NameMode::Default
        && request.spec.forge_parts().is_none()
        && !matches!(request.spec, RepoSpec::GithubMine { .. })
    {
        return Err(DtrError::new(
            "-O/--name-owner and -D apply only to recognized GitHub or GitLab repositories",
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
                request.git_options,
                target_dir,
                pass_target,
                preparations,
                repospec,
            ))
        }
        RepoSpec::Forge { forge, remote, .. } => {
            let preferred = match forge {
                Forge::GitHub => "gh",
                Forge::GitLab => "glab",
            };
            if command_exists(preferred) {
                let repository = remote
                    .clone()
                    .unwrap_or_else(|| request.spec.forge_slug().expect("forge slug").into());
                Ok(forge_clone_plan(
                    preferred,
                    repository,
                    request.git_options,
                    target_dir,
                    pass_target,
                    preparations,
                    repospec,
                ))
            } else {
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
    if !args.go {
        return Err(DtrError::new("MVP00 requires the --go installer selector"));
    }
    require_command("go", "installing a Go command")?;
    let spec = RepoSpec::parse(&args.repospec)?;
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
        });
    }

    let owner = if matches!(spec, RepoSpec::GithubMine { .. }) {
        Some(resolve_github_owner()?)
    } else {
        None
    };
    let mut import_path = spec.go_import_path(owner.as_deref())?;
    if !args.no_latest {
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
    })
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
    git_options: Vec<OsString>,
    target_dir: PathBuf,
    pass_target: bool,
    preparations: Vec<PathBuf>,
    repospec: String,
) -> CommandPlan {
    let mut args = ["repo", "clone"].map(OsString::from).to_vec();
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
        };
        let owner_dir = CloneRequest {
            spec,
            directory: None,
            git_options: Vec::new(),
            name_mode: NameMode::OwnerDirectory,
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
        assert_eq!(plan.args[1], remote);
    }
}
