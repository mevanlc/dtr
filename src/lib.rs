mod cli;
mod clone_args;
mod command;
mod config;
mod error;
mod github_auth;
mod install_all;
mod install_detect;
mod repospec;
mod resolve;

use clap::Parser;

use crate::cli::{Cli, DtrCommand};
use crate::clone_args::{ParsedClone, parse_clone_args};
use crate::error::DtrError;
use crate::resolve::{plan_clone, plan_install};

pub fn main_entry() -> i32 {
    match run() {
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

    let narration_override = if narration {
        Some(true)
    } else if no_narration {
        Some(false)
    } else {
        None
    };

    let plan = match command {
        DtrCommand::Clone(args) => match parse_clone_args(args.argv)? {
            ParsedClone::Help => {
                print!("{}", clone_args::HELP);
                return Ok(0);
            }
            ParsedClone::Request(request) => plan_clone(*request)?,
        },
        DtrCommand::Install(args) if args.add => {
            return install_all::run_install_and_add(args, explain, narration_override);
        }
        DtrCommand::Install(args) => plan_install(args)?,
        DtrCommand::InstallAll(args) => {
            return install_all::run(args, explain, narration_override);
        }
        DtrCommand::Config(args) => {
            if explain {
                return Err(DtrError::new("--explain does not apply to dtr config"));
            }
            return config::run(args);
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
