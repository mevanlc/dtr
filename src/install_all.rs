use std::ffi::OsString;
use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::cli::{InstallArgs, InstallTool};
use crate::config::{self, Config};
use crate::error::DtrError;
use crate::resolve::plan_install;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallAllConfig {
    install: Vec<InstallEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstallEntry {
    repospec: String,

    #[serde(default)]
    tool: InstallTool,

    #[serde(default)]
    no_latest: bool,

    #[serde(default)]
    args: Vec<String>,
}

impl InstallAllConfig {
    fn load() -> Result<Self, DtrError> {
        let path = config::install_all_file_path()?;
        let text = fs::read_to_string(&path).map_err(|error| {
            DtrError::new(format!(
                "could not read install-all configuration {}: {error}",
                path.display()
            ))
        })?;
        let config: Self = toml::from_str(&text).map_err(|error| {
            DtrError::new(format!(
                "could not parse install-all configuration {}: {error}",
                path.display()
            ))
        })?;
        if config.install.is_empty() {
            return Err(DtrError::new(format!(
                "install-all configuration {} must contain at least one [[install]] entry",
                path.display()
            )));
        }
        Ok(config)
    }
}

impl InstallEntry {
    fn into_args(self, home: Option<&Path>) -> Result<InstallArgs, DtrError> {
        if self.repospec.is_empty() {
            return Err(DtrError::new("repospec must not be empty"));
        }

        Ok(InstallArgs {
            tool: self.tool,
            no_latest: self.no_latest,
            repospec: expand_home(self.repospec, home)?,
            install_args: self.args.into_iter().map(OsString::from).collect(),
        })
    }
}

pub(crate) fn run(explain: bool, narration_override: Option<bool>) -> Result<i32, DtrError> {
    let config = InstallAllConfig::load()?;
    let narration = if explain {
        false
    } else {
        match narration_override {
            Some(narration) => narration,
            None => Config::load_for_runtime()?.narration(),
        }
    };
    let home = home::home_dir();
    let mut failed = false;

    for (offset, entry) in config.install.into_iter().enumerate() {
        let number = offset + 1;
        let repospec = entry.repospec.clone();
        let plan = entry.into_args(home.as_deref()).and_then(plan_install);
        let plan = match plan {
            Ok(plan) => plan,
            Err(error) => {
                warn_entry(number, &repospec, &error);
                failed = true;
                continue;
            }
        };

        if explain {
            if offset > 0 {
                println!();
            }
            println!("install-all entry {number}:");
            plan.explain();
            continue;
        }

        match plan.execute(narration) {
            Ok(0) => {}
            Ok(code) => {
                eprintln!(
                    "dtr: warning: install-all entry {number} ({repospec:?}) exited with status {code}"
                );
                failed = true;
            }
            Err(error) => {
                warn_entry(number, &repospec, &error);
                failed = true;
            }
        }
    }

    Ok(i32::from(failed))
}

fn expand_home(repospec: String, home: Option<&Path>) -> Result<OsString, DtrError> {
    let suffix = if repospec == "~" {
        Some("")
    } else {
        repospec.strip_prefix("~/")
    };
    let Some(suffix) = suffix else {
        return Ok(repospec.into());
    };
    let home = home.ok_or_else(|| {
        DtrError::new(format!(
            "could not expand {repospec:?} because the user home directory could not be located"
        ))
    })?;
    Ok(home.join(suffix).into_os_string())
}

fn warn_entry(number: usize, repospec: &str, error: &DtrError) {
    eprintln!("dtr: warning: install-all entry {number} ({repospec:?}): {error}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn config_accepts_native_args_and_rust_alias() {
        let config: InstallAllConfig = toml::from_str(
            r#"
                [[install]]
                repospec = "~/p/my/ripgrep"
                tool = "rust"
                args = ["--force", "--features", "pcre2"]
            "#,
        )
        .unwrap();
        let entry = config.install.into_iter().next().unwrap();
        assert_eq!(entry.tool, InstallTool::Cargo);
        assert_eq!(entry.args, ["--force", "--features", "pcre2"]);
    }

    #[test]
    fn config_rejects_unknown_fields() {
        let error = toml::from_str::<InstallAllConfig>(
            r#"
                [[install]]
                repospec = "./tool"
                surprise = true
            "#,
        )
        .err()
        .expect("unknown field should fail");
        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn leading_tilde_expands_only_for_a_home_relative_path() {
        let home = Path::new("/home/user");
        assert_eq!(
            PathBuf::from(expand_home("~/p/my/tool".to_owned(), Some(home)).unwrap()),
            PathBuf::from("/home/user/p/my/tool")
        );
        assert_eq!(
            expand_home("~someone/tool".to_owned(), Some(home)).unwrap(),
            OsString::from("~someone/tool")
        );
        assert!(expand_home("~/tool".to_owned(), None).is_err());
    }
}
