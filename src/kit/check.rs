use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use super::{normalize_local_path, parse_config, validate_unique_repositories};
use crate::cli::InstallTool;
use crate::command::{command_exists, npm_executable};
use crate::config::Config;
use crate::error::DtrError;
use crate::install_detect;
use crate::output::Verbosity;
use crate::repospec::{InstallSource, RepoSpec};
use crate::resolve::{resolve_github_owner, select_github_auth, validate_install_options};

pub(super) fn run(path: &Path, verbosity_adjustment: i32) -> Result<i32, DtrError> {
    let config = parse_config(path, false)?.config;
    let home = home::home_dir();
    let mut valid = true;
    let runtime_config = Config::load_for_runtime().unwrap_or_else(|error| {
        eprintln!("dtr: error: {error}");
        valid = false;
        Config::default()
    });
    let verbosity = Verbosity::new(runtime_config.verbosity(), verbosity_adjustment);
    if let Err(error) = validate_unique_repositories(&config, path, home.as_deref()) {
        eprintln!("dtr: error: {error}");
        valid = false;
    }

    let mut commands = BTreeSet::new();
    let mut directories = HashSet::new();
    let mut present_directories = 0;
    let mut missing_directories = 0;
    for (offset, entry) in config.install.iter().enumerate() {
        if entry.tool != InstallTool::Auto {
            commands.insert(entry.tool.to_string());
        }
        let mut check_entry = || -> Result<(), DtrError> {
            let args = entry.clone().into_args(home.as_deref())?;
            let source = InstallSource::parse(&args.repospec)?;
            let mut directory_exists = true;
            if let Some(directory) = source.spec.local_path() {
                let identity = normalize_local_path(directory);
                directory_exists = match fs::metadata(directory) {
                    Ok(metadata) if metadata.is_dir() => {
                        if directories.insert(identity) {
                            present_directories += 1;
                        }
                        true
                    }
                    result => {
                        if directories.insert(identity) {
                            missing_directories += 1;
                            let reason = match result {
                                Ok(_) => "path is not a directory".to_owned(),
                                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                                    "not found".to_owned()
                                }
                                Err(error) => error.to_string(),
                            };
                            eprintln!(
                                "dtr: error: missing directory: {} ({reason})",
                                directory.display()
                            );
                        }
                        false
                    }
                };
            }

            validate_install_options(&args, &source, args.tool)?;
            if args.tool == InstallTool::Auto && directory_exists {
                let owner = if matches!(source.spec, RepoSpec::GithubMine { .. }) {
                    Some(resolve_github_owner()?)
                } else {
                    None
                };
                let selection = select_github_auth(&source.spec)?;
                let tool = install_detect::detect_tool(
                    &source.spec,
                    owner.as_deref(),
                    selection.as_ref(),
                    verbosity,
                )?;
                commands.insert(tool.to_string());
                validate_install_options(&args, &source, tool)?;
            }
            Ok(())
        };
        if let Err(error) = check_entry() {
            eprintln!("dtr: error: kit entry {}: {error}", offset + 1);
            valid = false;
        }
    }

    let mut missing_commands = 0;
    for command in &commands {
        let exists = if command == "npm" {
            npm_executable().is_some()
        } else {
            command_exists(command)
        };
        if !exists {
            eprintln!("dtr: error: missing toolchain command: {command} (not found on PATH)");
            missing_commands += 1;
        }
    }
    println!(
        "configuration: {} ({} entries)",
        if valid { "valid" } else { "invalid" },
        config.install.len()
    );
    println!(
        "toolchains: {} found, {missing_commands} missing",
        commands.len() - missing_commands
    );
    println!("directories: {present_directories} present, {missing_directories} missing");
    Ok(i32::from(
        !valid || missing_commands > 0 || missing_directories > 0,
    ))
}
