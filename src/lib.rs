mod cli;
mod clone_args;
mod command;
mod config;
mod error;
mod github_auth;
mod install_detect;
mod kit;
mod repospec;
mod resolve;
mod self_rebuild;

use std::io::{self, Write};

use clap::{CommandFactory, Parser};
use clap_complete::generate;

use crate::cli::{Cli, DtrCommand};
use crate::clone_args::{ParsedClone, parse_clone_args};
use crate::error::DtrError;
use crate::resolve::{plan_clone, plan_install};

pub fn main_entry() -> i32 {
    // Both executable entry points call this before starting any threads.
    let result = self_rebuild::adopt_locks().and_then(|_locks| run());
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("dtr: error: {error}");
            2
        }
    }
}

fn run() -> Result<i32, DtrError> {
    let Cli {
        explain,
        narration,
        no_narration,
        command,
    } = Cli::parse();

    if !explain {
        self_rebuild::sweep_stale_old_binary();
    }

    let narration_override = if narration {
        Some(true)
    } else if no_narration {
        Some(false)
    } else {
        None
    };

    let plan = match command {
        DtrCommand::Completion(args) => {
            if explain {
                return Err(DtrError::new("--explain does not apply to dtr completion"));
            }
            let mut command = Cli::command();
            let binary_name = command.get_name().to_owned();
            let mut script = Vec::new();
            generate(args.shell, &mut command, binary_name, &mut script);
            if let Err(error) = io::stdout().write_all(&script)
                && error.kind() != io::ErrorKind::BrokenPipe
            {
                return Err(DtrError::new(format!(
                    "failed to write completion script: {error}"
                )));
            }
            return Ok(0);
        }
        DtrCommand::Clone(args) => match parse_clone_args(args.argv)? {
            ParsedClone::Help => {
                print!("{}", clone_args::HELP);
                return Ok(0);
            }
            ParsedClone::Request(request) => plan_clone(*request)?,
        },
        DtrCommand::Install(args) if args.add => {
            return kit::run_install_and_add(args, explain, narration_override);
        }
        DtrCommand::Install(args) => {
            if !explain
                && let home = home::home_dir()
                && let Ok((entry, _)) = kit::InstallEntry::from_install_args(&args, home.as_deref())
                && self_rebuild::is_self_install_entry(&entry)
                && let Some(package_root) =
                    self_rebuild::resolve_local_package_root(&entry.repospec)
            {
                return self_rebuild::execute(&package_root, None);
            }
            plan_install(args)?
        }
        DtrCommand::Kit(args) => {
            return kit::run(args, explain, narration_override);
        }
        DtrCommand::Config(args) => {
            return config::run(args, explain, narration_override);
        }
    };

    if explain {
        plan.explain();
        Ok(0)
    } else {
        let narration = match narration_override {
            Some(narration) => narration,
            None => config::Config::load_for_runtime()?.narration(),
        };
        plan.execute(narration)
    }
}
