use std::ffi::OsString;
use std::process::Command;

use crate::config::Config;
use crate::error::DtrError;

pub(crate) struct GithubAuthSelection {
    pub(crate) account: String,
    pub(crate) token: OsString,
}

pub(crate) fn select_for_owner(owner: &str) -> Result<Option<GithubAuthSelection>, DtrError> {
    let config = Config::load()?;
    let Some(account) = config.auto_switch_account(owner) else {
        return Ok(None);
    };
    let account = account.to_owned();
    let output = Command::new("gh")
        .args([
            "auth",
            "token",
            "--hostname",
            "github.com",
            "--user",
            &account,
        ])
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .output()
        .map_err(|error| {
            DtrError::new(format!(
                "could not start gh while selecting GitHub account {account}: {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(DtrError::new(format!(
            "could not retrieve the stored gh token for auto-switch account {account}"
        )));
    }
    let token = String::from_utf8(output.stdout).map_err(|_| {
        DtrError::new(format!(
            "gh returned a non-UTF-8 token for auto-switch account {account}"
        ))
    })?;
    let token = token.trim();
    if token.is_empty() || token.chars().any(char::is_whitespace) {
        return Err(DtrError::new(format!(
            "gh returned an empty or invalid token for auto-switch account {account}"
        )));
    }
    Ok(Some(GithubAuthSelection {
        account,
        token: token.into(),
    }))
}
