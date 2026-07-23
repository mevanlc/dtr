use std::ffi::OsString;

use clap::{ArgGroup, Args, Parser, Subcommand};

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
#[command(group(
    ArgGroup::new("installer")
        .required(true)
        .multiple(false)
        .args(["go", "rust", "uv", "pipx"])
))]
pub(crate) struct InstallArgs {
    /// Install a Go command from the repository
    #[arg(long)]
    pub(crate) go: bool,

    /// Install a Rust binary from the repository
    #[arg(long, visible_alias = "cargo")]
    pub(crate) rust: bool,

    /// Install a Python tool with uv
    #[arg(long)]
    pub(crate) uv: bool,

    /// Install a Python tool with pipx
    #[arg(long)]
    pub(crate) pipx: bool,

    /// Do not add @latest to a remote Go import path
    #[arg(long)]
    pub(crate) no_latest: bool,

    /// Repository name, path, or remote
    #[arg(value_name = "DTR_REPOSPEC")]
    pub(crate) repospec: OsString,

    /// Native installer arguments, following --
    #[arg(last = true, value_name = "INSTALL_ARG", num_args = 0..)]
    pub(crate) install_args: Vec<OsString>,
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
    fn rust_and_cargo_select_the_same_installer() {
        for selector in ["--rust", "--cargo"] {
            let cli = Cli::try_parse_from(["dtr", "install", selector, "owner/repo"])
                .expect("valid Rust install command");
            let DtrCommand::Install(args) = cli.command else {
                panic!("expected install command");
            };
            assert!(args.rust);
            assert!(!args.go);
            assert!(!args.uv);
            assert!(!args.pipx);
        }
    }

    #[test]
    fn python_installers_are_selectable() {
        for selector in ["--uv", "--pipx"] {
            let cli = Cli::try_parse_from(["dtr", "install", selector, "owner/repo"])
                .expect("valid Python install command");
            let DtrCommand::Install(args) = cli.command else {
                panic!("expected install command");
            };
            assert_eq!(args.uv, selector == "--uv");
            assert_eq!(args.pipx, selector == "--pipx");
        }
    }

    #[test]
    fn installer_is_required_and_selectors_conflict() {
        assert!(Cli::try_parse_from(["dtr", "install", "owner/repo"]).is_err());
        assert!(Cli::try_parse_from(["dtr", "install", "--go", "--rust", "owner/repo",]).is_err());
        assert!(Cli::try_parse_from(["dtr", "install", "--uv", "--pipx", "owner/repo",]).is_err());
    }

    #[test]
    fn cargo_arguments_require_and_follow_the_separator() {
        assert!(
            Cli::try_parse_from(["dtr", "install", "--rust", "owner/repo", "--locked",]).is_err()
        );

        let cli = Cli::try_parse_from([
            "dtr",
            "install",
            "--rust",
            "owner/repo",
            "--",
            "--locked",
            "--bin",
            "tool",
        ])
        .expect("valid Rust install command");
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
            OsString::from("--uv"),
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
}
