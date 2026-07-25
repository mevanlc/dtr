use std::ffi::OsString;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "dtr",
    version,
    about = "Do The Repo repo",
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
    #[command(disable_help_flag = true, visible_alias = "c")]
    Clone(CloneArgs),

    /// Install tools from a repository
    #[command(visible_alias = "i")]
    Install(InstallArgs),

    /// Read or change dtr configuration
    #[command(
        long_about = "Read or change dtr configuration.\n\nAvailable configuration keys:\n  github.auth.auto_switch\n      Comma-separated GitHub CLI account names eligible for process-scoped\n      authentication when an explicit repository owner matches."
    )]
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
    /// Installer to use; rust is an alias for cargo
    #[arg(short = 't', long, default_value_t = InstallTool::Auto)]
    pub(crate) tool: InstallTool,

    /// Do not add @latest to a remote Go import path
    #[arg(long)]
    pub(crate) no_latest: bool,

    /// Repository name, path, or remote; remote Go sources may end in @<query>
    #[arg(value_name = "DTR_REPOSPEC")]
    pub(crate) repospec: OsString,

    /// Native installer arguments, following --
    #[arg(last = true, value_name = "INSTALL_ARG", num_args = 0..)]
    pub(crate) install_args: Vec<OsString>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum InstallTool {
    Go,
    #[value(alias("rust"))]
    Cargo,
    Uv,
    Pipx,
    Npm,
    Auto,
}

impl std::fmt::Display for InstallTool {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Go => "go",
            Self::Cargo => "cargo",
            Self::Uv => "uv",
            Self::Pipx => "pipx",
            Self::Npm => "npm",
            Self::Auto => "auto",
        })
    }
}

#[derive(Debug, Args)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// List configured values
    List(ConfigListArgs),

    /// Set a configuration value
    Set(ConfigSetArgs),

    /// Print a configuration value
    Get(ConfigKeyArgs),

    /// Remove a configuration value
    Unset(ConfigKeyArgs),
}

#[derive(Debug, Args)]
pub(crate) struct ConfigListArgs {
    /// Show configured key names without values
    #[arg(long)]
    pub(crate) name_only: bool,
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
    use clap::CommandFactory;

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
        let cli =
            Cli::try_parse_from(["dtr", "i", "--tool", "go", "owner/repo"]).expect("valid command");
        assert!(matches!(cli.command, DtrCommand::Install(_)));
    }

    #[test]
    fn cargo_and_rust_value_alias_select_the_same_installer() {
        for value in ["cargo", "rust"] {
            let cli = Cli::try_parse_from(["dtr", "install", "--tool", value, "owner/repo"])
                .expect("valid Cargo install command");
            let DtrCommand::Install(args) = cli.command else {
                panic!("expected install command");
            };
            assert_eq!(args.tool, InstallTool::Cargo);
        }
    }

    #[test]
    fn python_installers_are_selectable() {
        for value in ["uv", "pipx"] {
            let cli = Cli::try_parse_from(["dtr", "install", "-t", value, "owner/repo"])
                .expect("valid Python install command");
            let DtrCommand::Install(args) = cli.command else {
                panic!("expected install command");
            };
            assert_eq!(
                args.tool,
                if value == "uv" {
                    InstallTool::Uv
                } else {
                    InstallTool::Pipx
                }
            );
        }
    }

    #[test]
    fn auto_is_the_default_and_npm_is_selectable() {
        let cli = Cli::try_parse_from(["dtr", "install", "owner/repo"])
            .expect("valid automatic install command");
        let DtrCommand::Install(args) = cli.command else {
            panic!("expected install command");
        };
        assert_eq!(args.tool, InstallTool::Auto);

        let cli = Cli::try_parse_from(["dtr", "install", "--tool=npm", "owner/repo"])
            .expect("valid npm install command");
        let DtrCommand::Install(args) = cli.command else {
            panic!("expected install command");
        };
        assert_eq!(args.tool, InstallTool::Npm);
    }

    #[test]
    fn legacy_selectors_are_rejected_and_tool_cannot_repeat() {
        for selector in ["--go", "--rust", "--cargo", "--uv", "--pipx", "--npm"] {
            assert!(
                Cli::try_parse_from(["dtr", "install", selector, "owner/repo"]).is_err(),
                "{selector}"
            );
        }
        assert!(
            Cli::try_parse_from([
                "dtr",
                "install",
                "--tool",
                "go",
                "--tool",
                "cargo",
                "owner/repo",
            ])
            .is_err()
        );
    }

    #[test]
    fn cargo_arguments_require_and_follow_the_separator() {
        assert!(
            Cli::try_parse_from([
                "dtr",
                "install",
                "--tool",
                "cargo",
                "owner/repo",
                "--locked",
            ])
            .is_err()
        );

        let cli = Cli::try_parse_from([
            "dtr",
            "install",
            "--tool",
            "cargo",
            "owner/repo",
            "--",
            "--locked",
            "--bin",
            "tool",
        ])
        .expect("valid Cargo install command");
        let DtrCommand::Install(args) = cli.command else {
            panic!("expected install command");
        };
        assert_eq!(
            args.install_args,
            ["--locked", "--bin", "tool"].map(OsString::from).to_vec()
        );
    }

    #[cfg(unix)]
    #[test]
    fn installer_arguments_preserve_non_utf8_values() {
        use std::os::unix::ffi::OsStringExt;

        let native_argument = OsString::from_vec(b"feature-\xff".to_vec());
        let cli = Cli::try_parse_from([
            OsString::from("dtr"),
            OsString::from("install"),
            OsString::from("--tool"),
            OsString::from("uv"),
            OsString::from("owner/repo"),
            OsString::from("--"),
            native_argument.clone(),
        ])
        .expect("valid non-UTF-8 Cargo argument");
        let DtrCommand::Install(args) = cli.command else {
            panic!("expected install command");
        };
        assert_eq!(args.install_args, [native_argument]);
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

    #[test]
    fn config_long_help_lists_every_known_key() {
        let mut command = Cli::command();
        let config = command
            .find_subcommand_mut("config")
            .expect("config subcommand");
        let help = config.render_long_help().to_string();
        for key in crate::config::CONFIG_KEYS {
            assert!(help.contains(key), "missing {key:?} from config long help");
        }
    }
}
