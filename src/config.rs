use std::collections::HashSet;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::cli::{ConfigArgs, ConfigCommand};
use crate::error::DtrError;

pub(crate) const GITHUB_AUTO_SWITCH_KEY: &str = "github.auth.auto_switch";
pub(crate) const CONFIG_KEYS: &[&str] = &[GITHUB_AUTO_SWITCH_KEY];

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    github: Option<GithubConfig>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct GithubConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    auth: Option<GithubAuthConfig>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct GithubAuthConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    auto_switch: Option<Vec<String>>,
}

impl Config {
    pub(crate) fn load() -> Result<Self, DtrError> {
        Self::load_at(&config_file_path()?)
    }

    fn load_at(path: &Path) -> Result<Self, DtrError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(DtrError::new(format!(
                    "could not read configuration {}: {error}",
                    path.display()
                )));
            }
        };
        let config: Self = toml::from_str(&text).map_err(|error| {
            DtrError::new(format!(
                "could not parse configuration {}: {error}",
                path.display()
            ))
        })?;
        config.validate()?;
        Ok(config)
    }

    fn save_at(&self, path: &Path) -> Result<(), DtrError> {
        let parent = path.parent().ok_or_else(|| {
            DtrError::new(format!(
                "configuration path {} has no parent directory",
                path.display()
            ))
        })?;
        fs::create_dir_all(parent).map_err(|error| {
            DtrError::new(format!(
                "could not create configuration directory {}: {error}",
                parent.display()
            ))
        })?;

        let text = toml::to_string_pretty(self).map_err(|error| {
            DtrError::new(format!("could not serialize configuration: {error}"))
        })?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|error| {
            DtrError::new(format!(
                "could not create a temporary configuration file in {}: {error}",
                parent.display()
            ))
        })?;
        temporary.write_all(text.as_bytes()).map_err(|error| {
            DtrError::new(format!(
                "could not write temporary configuration file: {error}"
            ))
        })?;
        temporary.as_file_mut().sync_all().map_err(|error| {
            DtrError::new(format!(
                "could not flush temporary configuration file: {error}"
            ))
        })?;
        temporary.persist(path).map_err(|error| {
            DtrError::new(format!(
                "could not replace configuration {}: {}",
                path.display(),
                error.error
            ))
        })?;
        Ok(())
    }

    pub(crate) fn auto_switch_account(&self, owner: &str) -> Option<&str> {
        self.github
            .as_ref()?
            .auth
            .as_ref()?
            .auto_switch
            .as_ref()?
            .iter()
            .find(|account| account.eq_ignore_ascii_case(owner))
            .map(String::as_str)
    }

    fn auto_switch_accounts(&self) -> Option<&[String]> {
        self.github.as_ref()?.auth.as_ref()?.auto_switch.as_deref()
    }

    fn set_auto_switch_accounts(&mut self, accounts: Vec<String>) {
        self.github
            .get_or_insert_with(GithubConfig::default)
            .auth
            .get_or_insert_with(GithubAuthConfig::default)
            .auto_switch = Some(accounts);
    }

    fn unset_auto_switch_accounts(&mut self) {
        let Some(github) = &mut self.github else {
            return;
        };
        let Some(auth) = &mut github.auth else {
            return;
        };
        auth.auto_switch = None;
        if auth.auto_switch.is_none() {
            github.auth = None;
        }
        if github.auth.is_none() {
            self.github = None;
        }
    }

    fn validate(&self) -> Result<(), DtrError> {
        let Some(accounts) = self.auto_switch_accounts() else {
            return Ok(());
        };
        if accounts.is_empty() {
            return Err(DtrError::new(format!(
                "{GITHUB_AUTO_SWITCH_KEY} must contain at least one account"
            )));
        }

        let mut seen = HashSet::new();
        for account in accounts {
            if account != account.trim() || !is_valid_account(account) {
                return Err(DtrError::new(format!(
                    "invalid GitHub account name {account:?} in {GITHUB_AUTO_SWITCH_KEY}"
                )));
            }
            if !seen.insert(account.to_ascii_lowercase()) {
                return Err(DtrError::new(format!(
                    "duplicate GitHub account name {account:?} in {GITHUB_AUTO_SWITCH_KEY}"
                )));
            }
        }
        Ok(())
    }
}

pub(crate) fn run(args: ConfigArgs) -> Result<i32, DtrError> {
    match args.command {
        ConfigCommand::List(args) => {
            let config = Config::load()?;
            if let Some(accounts) = config.auto_switch_accounts() {
                if args.name_only {
                    println!("{GITHUB_AUTO_SWITCH_KEY}");
                } else {
                    println!("{GITHUB_AUTO_SWITCH_KEY}={}", accounts.join(","));
                }
            }
        }
        ConfigCommand::Set(args) => {
            require_known_key(&args.key)?;
            let accounts = parse_auto_switch_accounts(&args.value)?;
            let path = config_file_path()?;
            let mut config = Config::load_at(&path)?;
            config.set_auto_switch_accounts(accounts);
            config.save_at(&path)?;
        }
        ConfigCommand::Get(args) => {
            require_known_key(&args.key)?;
            let config = Config::load()?;
            let accounts = config.auto_switch_accounts().ok_or_else(|| {
                DtrError::new(format!(
                    "configuration key {GITHUB_AUTO_SWITCH_KEY} is not set"
                ))
            })?;
            println!("{}", accounts.join(","));
        }
        ConfigCommand::Unset(args) => {
            require_known_key(&args.key)?;
            let path = config_file_path()?;
            if path.exists() {
                let mut config = Config::load_at(&path)?;
                config.unset_auto_switch_accounts();
                config.save_at(&path)?;
            }
        }
    }
    Ok(0)
}

fn require_known_key(key: &str) -> Result<(), DtrError> {
    if CONFIG_KEYS.contains(&key) {
        Ok(())
    } else {
        Err(DtrError::new(format!(
            "unknown configuration key {key:?}; available keys: {}",
            CONFIG_KEYS.join(", ")
        )))
    }
}

fn parse_auto_switch_accounts(value: &str) -> Result<Vec<String>, DtrError> {
    let mut seen = HashSet::new();
    let mut accounts = Vec::new();
    for member in value.split(',') {
        let account = member.trim();
        if account.is_empty() {
            return Err(DtrError::new(format!(
                "{GITHUB_AUTO_SWITCH_KEY} contains an empty account name"
            )));
        }
        if !is_valid_account(account) {
            return Err(DtrError::new(format!(
                "invalid GitHub account name {account:?} in {GITHUB_AUTO_SWITCH_KEY}"
            )));
        }
        if seen.insert(account.to_ascii_lowercase()) {
            accounts.push(account.to_owned());
        }
    }
    Ok(accounts)
}

fn is_valid_account(account: &str) -> bool {
    !account.is_empty()
        && account
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn config_file_path() -> Result<PathBuf, DtrError> {
    let dtr_config_dir = env::var_os("DTR_CONFIG_DIR");
    let xdg_config_home = env::var_os("XDG_CONFIG_HOME");
    let home = env::var_os("HOME");
    config_file_path_from(
        dtr_config_dir.as_deref(),
        xdg_config_home.as_deref(),
        home.as_deref(),
    )
}

fn config_file_path_from(
    dtr_config_dir: Option<&OsStr>,
    xdg_config_home: Option<&OsStr>,
    home: Option<&OsStr>,
) -> Result<PathBuf, DtrError> {
    if let Some(directory) = dtr_config_dir {
        if directory.is_empty() {
            return Err(DtrError::new("DTR_CONFIG_DIR must not be empty"));
        }
        return Ok(Path::new(directory).join("config.toml"));
    }
    if let Some(directory) = xdg_config_home.filter(|directory| !directory.is_empty()) {
        return Ok(Path::new(directory).join("dtr/config.toml"));
    }
    if let Some(directory) = home.filter(|directory| !directory.is_empty()) {
        return Ok(Path::new(directory).join(".config/dtr/config.toml"));
    }
    Err(DtrError::new(
        "could not locate dtr configuration: HOME, XDG_CONFIG_HOME, and DTR_CONFIG_DIR are unset",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trims_and_case_insensitively_deduplicates_accounts() {
        assert_eq!(
            parse_auto_switch_accounts(" mevanlc,MIKE-clark-8192,MeVaNlC ").unwrap(),
            ["mevanlc", "MIKE-clark-8192"]
        );
    }

    #[test]
    fn rejects_empty_and_invalid_accounts() {
        for value in ["", "mevanlc,", ",mevanlc", "me vanlc"] {
            assert!(parse_auto_switch_accounts(value).is_err(), "{value:?}");
        }
    }

    #[test]
    fn typed_toml_rejects_unknown_auth_fields() {
        let error = toml::from_str::<Config>(
            "[github.auth]\nauto_switch = [\"mevanlc\"]\nsurprise = true\n",
        )
        .err()
        .expect("unknown field should fail");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn typed_toml_validation_rejects_empty_invalid_and_duplicate_lists() {
        for text in [
            "[github.auth]\nauto_switch = []\n",
            "[github.auth]\nauto_switch = [\"not an account\"]\n",
            "[github.auth]\nauto_switch = [\"mevanlc\", \"MeVaNlC\"]\n",
        ] {
            let config: Config = toml::from_str(text).unwrap();
            assert!(config.validate().is_err(), "{text:?}");
        }
    }

    #[test]
    fn typed_toml_round_trips_and_matches_owner_case_insensitively() {
        let mut config = Config::default();
        config.set_auto_switch_accounts(vec!["MeVaNlC".to_owned()]);
        let encoded = toml::to_string_pretty(&config).unwrap();
        let decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.auto_switch_account("mevanlc"), Some("MeVaNlC"));
    }

    #[test]
    fn config_path_precedence_is_deterministic() {
        assert_eq!(
            config_file_path_from(
                Some(OsStr::new("/dtr")),
                Some(OsStr::new("/xdg")),
                Some(OsStr::new("/home")),
            )
            .unwrap(),
            PathBuf::from("/dtr/config.toml")
        );
        assert_eq!(
            config_file_path_from(None, Some(OsStr::new("/xdg")), Some(OsStr::new("/home")),)
                .unwrap(),
            PathBuf::from("/xdg/dtr/config.toml")
        );
        assert_eq!(
            config_file_path_from(None, None, Some(OsStr::new("/home"))).unwrap(),
            PathBuf::from("/home/.config/dtr/config.toml")
        );
    }
}
