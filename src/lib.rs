mod cli;
mod clone_args;
mod command;
mod config;
mod error;
mod github_auth;
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
    let Cli { explain, command } = Cli::parse();

    let plan = match command {
        DtrCommand::Clone(args) => match parse_clone_args(args.argv)? {
            ParsedClone::Help => {
                print!("{}", clone_args::HELP);
                return Ok(0);
            }
            ParsedClone::Request(request) => plan_clone(request)?,
        },
        DtrCommand::Install(args) => plan_install(args)?,
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
        plan.execute()
    }
}
