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
pub(crate) const NARRATION_KEY: &str = "narration";
pub(crate) const UV_INSTALL_FORCE_KEY: &str = "uv.install.force";
pub(crate) const UV_INSTALL_EDITABLE_KEY: &str = "uv.install.editable";
pub(crate) const UV_INSTALL_REINSTALL_KEY: &str = "uv.install.reinstall";
pub(crate) const CONFIG_KEYS: &[&str] = &[
    GITHUB_AUTO_SWITCH_KEY,
    NARRATION_KEY,
    UV_INSTALL_FORCE_KEY,
    UV_INSTALL_EDITABLE_KEY,
    UV_INSTALL_REINSTALL_KEY,
];

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    narration: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    github: Option<GithubConfig>,

    #[serde(skip_serializing_if = "Option::is_none")]
    uv: Option<UvConfig>,
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

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct UvConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    install: Option<UvInstallConfig>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct UvInstallConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    force: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    editable: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    reinstall: Option<bool>,
}

/// A `uv tool install` option dtr can enable persistently.
#[derive(Clone, Copy)]
enum UvInstallFlag {
    Force,
    Editable,
    Reinstall,
}

impl UvInstallFlag {
    const ALL: [Self; 3] = [Self::Force, Self::Editable, Self::Reinstall];

    fn from_key(key: &str) -> Option<Self> {
        match key {
            UV_INSTALL_FORCE_KEY => Some(Self::Force),
            UV_INSTALL_EDITABLE_KEY => Some(Self::Editable),
            UV_INSTALL_REINSTALL_KEY => Some(Self::Reinstall),
            _ => None,
        }
    }

    fn option(self) -> &'static str {
        match self {
            Self::Force => "--force",
            Self::Editable => "--editable",
            Self::Reinstall => "--reinstall",
        }
    }
}

impl UvInstallConfig {
    fn value(&self, flag: UvInstallFlag) -> Option<bool> {
        match flag {
            UvInstallFlag::Force => self.force,
            UvInstallFlag::Editable => self.editable,
            UvInstallFlag::Reinstall => self.reinstall,
        }
    }

    fn value_mut(&mut self, flag: UvInstallFlag) -> &mut Option<bool> {
        match flag {
            UvInstallFlag::Force => &mut self.force,
            UvInstallFlag::Editable => &mut self.editable,
            UvInstallFlag::Reinstall => &mut self.reinstall,
        }
    }

    fn is_empty(&self) -> bool {
        UvInstallFlag::ALL
            .into_iter()
            .all(|flag| self.value(flag).is_none())
    }
}

impl Config {
    pub(crate) fn load() -> Result<Self, DtrError> {
        Self::load_at(&config_file_path()?)
    }

    pub(crate) fn load_for_runtime() -> Result<Self, DtrError> {
        let path = discover_config_file_path()?;
        Self::load_optional_at(path.as_deref())
    }

    fn load_optional_at(path: Option<&Path>) -> Result<Self, DtrError> {
        match path {
            Some(path) => Self::load_at(path),
            None => Ok(Self::default()),
        }
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

    pub(crate) fn narration(&self) -> bool {
        self.narration.unwrap_or(true)
    }

    /// The `uv tool install` options enabled by configuration, in key order.
    pub(crate) fn uv_install_options(&self) -> Vec<&'static str> {
        UvInstallFlag::ALL
            .into_iter()
            .filter(|flag| self.uv_install_flag(*flag).unwrap_or(false))
            .map(UvInstallFlag::option)
            .collect()
    }

    fn value(&self, key: &str) -> Option<String> {
        match key {
            GITHUB_AUTO_SWITCH_KEY => Some(self.auto_switch_accounts()?.join(",")),
            NARRATION_KEY => Some(self.narration?.to_string()),
            key => Some(
                self.uv_install_flag(UvInstallFlag::from_key(key)?)?
                    .to_string(),
            ),
        }
    }

    fn auto_switch_accounts(&self) -> Option<&[String]> {
        self.github.as_ref()?.auth.as_ref()?.auto_switch.as_deref()
    }

    fn uv_install_flag(&self, flag: UvInstallFlag) -> Option<bool> {
        self.uv.as_ref()?.install.as_ref()?.value(flag)
    }

    fn set_uv_install_flag(&mut self, flag: UvInstallFlag, value: bool) {
        *self
            .uv
            .get_or_insert_with(UvConfig::default)
            .install
            .get_or_insert_with(UvInstallConfig::default)
            .value_mut(flag) = Some(value);
    }

    fn unset_uv_install_flag(&mut self, flag: UvInstallFlag) {
        let Some(uv) = &mut self.uv else {
            return;
        };
        let Some(install) = &mut uv.install else {
            return;
        };
        *install.value_mut(flag) = None;
        if install.is_empty() {
            uv.install = None;
        }
        if uv.install.is_none() {
            self.uv = None;
        }
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
            for key in CONFIG_KEYS {
                let Some(value) = config.value(key) else {
                    continue;
                };
                if args.name_only {
                    println!("{key}");
                } else {
                    println!("{key}={value}");
                }
            }
        }
        ConfigCommand::Set(args) => {
            require_known_key(&args.key)?;
            let path = config_file_path()?;
            let mut config = Config::load_at(&path)?;
            match args.key.as_str() {
                GITHUB_AUTO_SWITCH_KEY => {
                    config.set_auto_switch_accounts(parse_auto_switch_accounts(&args.value)?);
                }
                NARRATION_KEY => config.narration = Some(parse_bool(NARRATION_KEY, &args.value)?),
                key => {
                    config.set_uv_install_flag(uv_install_flag(key), parse_bool(key, &args.value)?)
                }
            }
            config.save_at(&path)?;
        }
        ConfigCommand::Get(args) => {
            require_known_key(&args.key)?;
            let value = Config::load()?.value(&args.key).ok_or_else(|| {
                DtrError::new(format!("configuration key {} is not set", args.key))
            })?;
            println!("{value}");
        }
        ConfigCommand::Unset(args) => {
            require_known_key(&args.key)?;
            let path = config_file_path()?;
            if path.exists() {
                let mut config = Config::load_at(&path)?;
                match args.key.as_str() {
                    GITHUB_AUTO_SWITCH_KEY => config.unset_auto_switch_accounts(),
                    NARRATION_KEY => config.narration = None,
                    key => config.unset_uv_install_flag(uv_install_flag(key)),
                }
                config.save_at(&path)?;
            }
        }
    }
    Ok(0)
}

fn parse_bool(key: &str, value: &str) -> Result<bool, DtrError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(DtrError::new(format!("{key} must be true or false"))),
    }
}

fn uv_install_flag(key: &str) -> UvInstallFlag {
    UvInstallFlag::from_key(key).expect("key was checked against the known configuration keys")
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
    discover_config_file_path()?
        .ok_or_else(|| DtrError::new("could not locate the user home directory"))
}

pub(crate) fn kit_file_path() -> Result<PathBuf, DtrError> {
    Ok(config_file_path()?.with_file_name("kit.toml"))
}

pub(crate) fn migrate_legacy_kit_file() -> Result<PathBuf, DtrError> {
    let kit = kit_file_path()?;
    if path_entry_exists(&kit, "kit configuration")? {
        return Ok(kit);
    }

    let legacy = kit.with_file_name("install-all.toml");
    if !path_entry_exists(&legacy, "legacy kit configuration")? {
        return Ok(kit);
    }

    fs::rename(&legacy, &kit).map_err(|error| {
        DtrError::new(format!(
            "could not rename legacy kit configuration {} to {}: {error}",
            legacy.display(),
            kit.display()
        ))
    })?;
    Ok(kit)
}

fn path_entry_exists(path: &Path, description: &str) -> Result<bool, DtrError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(DtrError::new(format!(
            "could not inspect {description} {}: {error}",
            path.display()
        ))),
    }
}

fn discover_config_file_path() -> Result<Option<PathBuf>, DtrError> {
    let dtr_config_dir = env::var_os("DTR_CONFIG_DIR");
    let home = home::home_dir();
    config_file_path_from(dtr_config_dir.as_deref(), home.as_deref())
}

fn config_file_path_from(
    dtr_config_dir: Option<&OsStr>,
    home: Option<&Path>,
) -> Result<Option<PathBuf>, DtrError> {
    if let Some(directory) = dtr_config_dir {
        if directory.is_empty() {
            return Err(DtrError::new("DTR_CONFIG_DIR must not be empty"));
        }
        return Ok(Some(Path::new(directory).join("config.toml")));
    }
    if let Some(directory) = home {
        return Ok(Some(
            directory.join(".config").join("dtr").join("config.toml"),
        ));
    }
    Ok(None)
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
    fn narration_defaults_on_and_accepts_only_boolean_values() {
        assert!(Config::default().narration());
        assert!(parse_bool(NARRATION_KEY, "true").unwrap());
        assert!(!parse_bool(NARRATION_KEY, "false").unwrap());
        for value in ["", "yes", "False", "0"] {
            let error = parse_bool(NARRATION_KEY, value).expect_err(value);
            assert_eq!(error.to_string(), "narration must be true or false");
        }
    }

    #[test]
    fn uv_install_options_follow_key_order_and_omit_disabled_flags() {
        let mut config = Config::default();
        assert!(config.uv_install_options().is_empty());

        for key in [
            UV_INSTALL_REINSTALL_KEY,
            UV_INSTALL_FORCE_KEY,
            UV_INSTALL_EDITABLE_KEY,
        ] {
            config.set_uv_install_flag(uv_install_flag(key), true);
        }
        assert_eq!(
            config.uv_install_options(),
            ["--force", "--editable", "--reinstall"]
        );

        config.set_uv_install_flag(uv_install_flag(UV_INSTALL_EDITABLE_KEY), false);
        assert_eq!(config.uv_install_options(), ["--force", "--reinstall"]);
        assert_eq!(
            config.value(UV_INSTALL_EDITABLE_KEY).as_deref(),
            Some("false")
        );
    }

    #[test]
    fn uv_install_flags_round_trip_and_unsetting_the_last_one_drops_the_table() {
        let mut config = Config::default();
        config.set_uv_install_flag(uv_install_flag(UV_INSTALL_FORCE_KEY), true);
        let encoded = toml::to_string_pretty(&config).unwrap();
        assert_eq!(encoded, "[uv.install]\nforce = true\n");

        let mut decoded: Config = toml::from_str(&encoded).unwrap();
        assert_eq!(decoded.value(UV_INSTALL_FORCE_KEY).as_deref(), Some("true"));
        assert_eq!(decoded.value(UV_INSTALL_REINSTALL_KEY), None);

        decoded.unset_uv_install_flag(uv_install_flag(UV_INSTALL_REINSTALL_KEY));
        assert_eq!(decoded.value(UV_INSTALL_FORCE_KEY).as_deref(), Some("true"));
        decoded.unset_uv_install_flag(uv_install_flag(UV_INSTALL_FORCE_KEY));
        assert_eq!(toml::to_string_pretty(&decoded).unwrap(), "");
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
    fn config_path_prefers_override_then_uses_user_home() {
        assert_eq!(
            config_file_path_from(Some(OsStr::new("/dtr")), Some(Path::new("/home"))).unwrap(),
            Some(PathBuf::from("/dtr/config.toml"))
        );
        assert_eq!(
            config_file_path_from(None, Some(Path::new("/home"))).unwrap(),
            Some(PathBuf::from("/home/.config/dtr/config.toml"))
        );
        assert_eq!(config_file_path_from(None, None).unwrap(), None);
        assert!(config_file_path_from(Some(OsStr::new("")), None).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn default_windows_config_path_uses_native_separators() {
        let path = config_file_path_from(None, Some(Path::new(r"C:\Users\mclark")))
            .unwrap()
            .unwrap();
        assert_eq!(
            path.to_str(),
            Some(r"C:\Users\mclark\.config\dtr\config.toml")
        );
    }

    #[test]
    fn runtime_load_only_defaults_when_config_location_is_undiscoverable() {
        let config = Config::load_optional_at(None).unwrap();
        assert_eq!(config.auto_switch_account("mevanlc"), None);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("config.toml");
        fs::write(&path, "not valid toml = [").unwrap();
        let error = Config::load_optional_at(Some(&path)).err().unwrap();
        assert!(error.to_string().contains("could not parse configuration"));
    }
}
