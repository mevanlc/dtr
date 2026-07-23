use std::ffi::OsString;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "dtr",
    version,
    about = "Do/develop the right repo",
    long_about = "Resolve the repository reference you already have and invoke the right underlying tool."
)]
pub(crate) struct Cli {
    /// Explain the fully resolved operation without performing it
    #[arg(short = 'n', long)]
    pub(crate) explain: bool,

    #[command(subcommand)]
    pub(crate) command: DtrCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum DtrCommand {
    /// Clone a local or remote repository
    #[command(disable_help_flag = true)]
    Clone(CloneArgs),

    /// Install tools from a repository
    #[command(visible_alias = "i")]
    Install(InstallArgs),

    /// Read or change dtr configuration
    Config(ConfigArgs),
}

#[derive(Debug, Args)]
#[command(trailing_var_arg = true)]
pub(crate) struct CloneArgs {
    /// dtr clone options, git clone options, repository, and optional directory
    #[arg(value_name = "ARG", num_args = 0.., allow_hyphen_values = true)]
    pub(crate) argv: Vec<OsString>,
}

#[derive(Debug, Args)]
pub(crate) struct InstallArgs {
    /// Install a Go command from the repository
    #[arg(long, required = true)]
    pub(crate) go: bool,

    /// Do not add @latest to a remote Go import path
    #[arg(long)]
    pub(crate) no_latest: bool,

    /// Repository name, path, or remote
    #[arg(value_name = "DTR_REPOSPEC")]
    pub(crate) repospec: OsString,
}

#[derive(Debug, Args)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Set a configuration value
    Set(ConfigSetArgs),

    /// Print a configuration value
    Get(ConfigKeyArgs),

    /// Remove a configuration value
    Unset(ConfigKeyArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ConfigSetArgs {
    /// Configuration key
    #[arg(value_name = "KEY")]
    pub(crate) key: String,

    /// Configuration value
    #[arg(value_name = "VALUE")]
    pub(crate) value: String,
}

#[derive(Debug, Args)]
pub(crate) struct ConfigKeyArgs {
    /// Configuration key
    #[arg(value_name = "KEY")]
    pub(crate) key: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explain_is_a_pre_command_option() {
        let cli = Cli::try_parse_from(["dtr", "-n", "clone", "owner/repo"]).expect("valid command");
        assert!(cli.explain);
    }

    #[test]
    fn clone_preserves_hyphenated_arguments() {
        let cli =
            Cli::try_parse_from(["dtr", "clone", "--depth", "1", "owner/repo", "destination"])
                .expect("valid command");
        let DtrCommand::Clone(args) = cli.command else {
            panic!("expected clone command");
        };
        assert_eq!(
            args.argv,
            ["--depth", "1", "owner/repo", "destination"]
                .map(OsString::from)
                .to_vec()
        );
    }

    #[test]
    fn install_alias_is_accepted() {
        let cli = Cli::try_parse_from(["dtr", "i", "--go", "owner/repo"]).expect("valid command");
        assert!(matches!(cli.command, DtrCommand::Install(_)));
    }

    #[test]
    fn config_set_accepts_the_dotted_auth_key() {
        let cli = Cli::try_parse_from([
            "dtr",
            "config",
            "set",
            "github.auth.auto_switch",
            "mevanlc,mike-clark-8192",
        ])
        .expect("valid command");
        assert!(matches!(cli.command, DtrCommand::Config(_)));
    }
}
