use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::str::FromStr;

use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

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

    /// Print dtr's execution narration
    #[arg(long, global = true, conflicts_with = "no_narration")]
    pub(crate) narration: bool,

    /// Suppress dtr's execution narration (warnings remain enabled)
    #[arg(long, global = true, conflicts_with = "narration")]
    pub(crate) no_narration: bool,

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

    /// Manage the configured installation kit
    #[command(visible_alias = "k")]
    Kit(KitArgs),

    /// Read or change dtr configuration
    #[command(
        long_about = "Read or change dtr configuration.\n\nAvailable configuration keys:\n  github.auth.auto_switch\n      Comma-separated GitHub CLI account names eligible for process-scoped\n      authentication when an explicit repository owner matches.\n  narration\n      Whether dtr prints command, clone-path, and install-success narration.\n  uv.install.force\n      Whether uv installs receive --force.\n  uv.install.editable\n      Whether uv installs receive --editable.\n  uv.install.reinstall\n      Whether uv installs receive --reinstall."
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
    /// Add this install to kit.toml after it succeeds
    #[arg(short = 'a', long)]
    pub(crate) add: bool,

    /// Installer to use; rust is an alias for cargo
    #[arg(short = 't', long, default_value_t = InstallTool::Auto)]
    pub(crate) tool: InstallTool,

    /// Do not add @latest to a remote Go import path
    #[arg(long)]
    pub(crate) no_latest: bool,

    /// Repository name, path, or remote; remote Go sources may end in @<query>
    #[arg(value_name = "DTR_REPOSPEC", default_value = ".")]
    pub(crate) repospec: OsString,

    /// Native installer arguments, following --
    #[arg(last = true, value_name = "INSTALL_ARG", num_args = 0..)]
    pub(crate) install_args: Vec<OsString>,
}

#[derive(Debug, Args)]
pub(crate) struct KitArgs {
    #[command(subcommand)]
    pub(crate) command: KitCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum KitCommand {
    /// Install every repository in the kit
    #[command(visible_alias = "i")]
    Install(KitInstallArgs),

    /// List kit entries as reusable dtr install commands
    #[command(visible_alias = "ls")]
    List(KitFileArgs),

    /// Edit the kit configuration
    Edit(KitFileArgs),
}

#[derive(Debug, Args)]
pub(crate) struct KitInstallArgs {
    /// Use an alternate kit TOML file
    #[arg(long, value_name = "FILE")]
    pub(crate) file: Option<PathBuf>,

    /// Maximum concurrent installs; auto uses ceil(available CPU cores / 2)
    #[arg(short = 'j', long, value_name = "n|auto", default_value_t = Jobs::Auto)]
    pub(crate) jobs: Jobs,

    /// Suppress installer stdout; repeat for stderr, then dtr narration
    #[arg(short = 'q', long, action = ArgAction::Count)]
    pub(crate) quiet: u8,
}

#[derive(Debug, Args)]
pub(crate) struct KitFileArgs {
    /// Use an alternate kit TOML file
    #[arg(long, value_name = "FILE")]
    pub(crate) file: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Jobs {
    #[default]
    Auto,
    Count(NonZeroUsize),
}

impl Jobs {
    pub(crate) fn resolve(self) -> usize {
        match self {
            Self::Auto => {
                automatic_jobs(std::thread::available_parallelism().map_or(1, NonZeroUsize::get))
            }
            Self::Count(jobs) => jobs.get(),
        }
    }
}

impl std::fmt::Display for Jobs {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => formatter.write_str("auto"),
            Self::Count(jobs) => jobs.fmt(formatter),
        }
    }
}

impl FromStr for Jobs {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "auto" {
            return Ok(Self::Auto);
        }
        value.parse::<NonZeroUsize>().map(Self::Count).map_err(|_| {
            format!("invalid jobs value {value:?}; expected 'auto' or a positive integer")
        })
    }
}

fn automatic_jobs(cores: usize) -> usize {
    cores.div_ceil(2).max(1)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InstallTool {
    Go,
    #[value(alias("rust"))]
    #[serde(alias = "rust")]
    Cargo,
    Uv,
    Pipx,
    Npm,
    #[default]
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
    fn kit_aliases_are_accepted() {
        let install = Cli::try_parse_from(["dtr", "k", "i"]).expect("valid kit install alias");
        let DtrCommand::Kit(args) = install.command else {
            panic!("expected kit command");
        };
        assert!(matches!(args.command, KitCommand::Install(_)));

        let list = Cli::try_parse_from(["dtr", "k", "ls"]).expect("valid kit list alias");
        let DtrCommand::Kit(args) = list.command else {
            panic!("expected kit command");
        };
        assert!(matches!(args.command, KitCommand::List(_)));
    }

    #[test]
    fn kit_install_defaults_to_automatic_jobs() {
        let cli =
            Cli::try_parse_from(["dtr", "kit", "install"]).expect("valid kit install command");
        let DtrCommand::Kit(args) = cli.command else {
            panic!("expected kit command");
        };
        let KitCommand::Install(args) = args.command else {
            panic!("expected kit install command");
        };
        assert_eq!(args.jobs, Jobs::Auto);
        assert_eq!(args.file, None);
        assert_eq!(args.quiet, 0);
    }

    #[test]
    fn install_add_and_kit_file_options_are_accepted() {
        let install = Cli::try_parse_from(["dtr", "install", "--add", "./tool"])
            .expect("valid tracked install");
        let DtrCommand::Install(args) = install.command else {
            panic!("expected install command");
        };
        assert!(args.add);

        for subcommand in ["list", "edit"] {
            let cli = Cli::try_parse_from(["dtr", "kit", subcommand, "--file", "other.toml"])
                .expect("valid kit management command");
            let DtrCommand::Kit(args) = cli.command else {
                panic!("expected kit command");
            };
            let file = match args.command {
                KitCommand::List(args) | KitCommand::Edit(args) => args.file,
                KitCommand::Install(_) => panic!("expected kit management command"),
            };
            assert_eq!(file, Some(PathBuf::from("other.toml")));
        }
    }

    #[test]
    fn kit_jobs_accepts_positive_counts_and_auto() {
        for (value, expected) in [
            ("auto", Jobs::Auto),
            ("1", Jobs::Count(NonZeroUsize::new(1).unwrap())),
            ("17", Jobs::Count(NonZeroUsize::new(17).unwrap())),
        ] {
            let cli = Cli::try_parse_from(["dtr", "kit", "install", "--jobs", value])
                .expect("valid jobs value");
            let DtrCommand::Kit(args) = cli.command else {
                panic!("expected kit command");
            };
            let KitCommand::Install(args) = args.command else {
                panic!("expected kit install command");
            };
            assert_eq!(args.jobs, expected);
        }

        for value in ["0", "-1", "all", "1.5"] {
            assert!(
                Cli::try_parse_from(["dtr", "kit", "install", "-j", value]).is_err(),
                "{value:?}"
            );
        }
    }

    #[test]
    fn kit_quiet_counts_short_and_long_occurrences() {
        for (arguments, expected) in [
            (vec!["dtr", "kit", "install", "-q"], 1),
            (vec!["dtr", "kit", "install", "-qq"], 2),
            (vec!["dtr", "kit", "install", "-qqq"], 3),
            (vec!["dtr", "kit", "install", "--quiet", "--quiet"], 2),
        ] {
            let cli = Cli::try_parse_from(arguments).expect("valid quiet level");
            let DtrCommand::Kit(args) = cli.command else {
                panic!("expected kit command");
            };
            let KitCommand::Install(args) = args.command else {
                panic!("expected kit install command");
            };
            assert_eq!(args.quiet, expected);
        }
    }

    #[test]
    fn automatic_jobs_is_half_the_core_count_rounded_up() {
        for (cores, jobs) in [(0, 1), (1, 1), (2, 1), (3, 2), (4, 2), (5, 3), (32, 16)] {
            assert_eq!(automatic_jobs(cores), jobs, "{cores} cores");
        }
    }

    #[test]
    fn narration_overrides_are_global_and_mutually_exclusive() {
        let before = Cli::try_parse_from(["dtr", "--no-narration", "clone", "owner/repo"])
            .expect("valid narration opt-out");
        assert!(before.no_narration);

        let after = Cli::try_parse_from([
            "dtr",
            "install",
            "--narration",
            "--tool",
            "go",
            "owner/repo",
        ])
        .expect("valid narration opt-in");
        assert!(after.narration);

        assert!(
            Cli::try_parse_from([
                "dtr",
                "--narration",
                "--no-narration",
                "install",
                "owner/repo",
            ])
            .is_err()
        );
    }

    #[test]
    fn install_defaults_to_the_current_directory() {
        let cli = Cli::try_parse_from(["dtr", "i"]).expect("valid current-directory install");
        let DtrCommand::Install(args) = cli.command else {
            panic!("expected install command");
        };
        assert_eq!(args.repospec, ".");
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
