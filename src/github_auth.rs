use std::env;
use std::ffi::{OsStr, OsString};
use std::process::Command;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::command::SecretEnvironment;
use crate::config::Config;
use crate::error::DtrError;

pub(crate) struct GithubAuthSelection {
    pub(crate) account: String,
    pub(crate) token: String,
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
        token: token.to_owned(),
    }))
}

pub(crate) fn cargo_git_environment(
    token: &str,
) -> Result<(Vec<SecretEnvironment>, Vec<OsString>), DtrError> {
    github_git_environment(
        token,
        vec![SecretEnvironment::new(
            "CARGO_NET_GIT_FETCH_WITH_CLI",
            "true",
        )],
    )
}

pub(crate) fn python_git_environment(
    token: &str,
) -> Result<(Vec<SecretEnvironment>, Vec<OsString>), DtrError> {
    github_git_environment(
        token,
        vec![SecretEnvironment::new("UV_NO_GITHUB_FAST_PATH", "true")],
    )
}

fn github_git_environment(
    token: &str,
    mut environment: Vec<SecretEnvironment>,
) -> Result<(Vec<SecretEnvironment>, Vec<OsString>), DtrError> {
    let count_value = env::var_os("GIT_CONFIG_COUNT");
    let count = git_config_count_from(count_value.as_deref())?;
    let next_count = count
        .checked_add(2)
        .ok_or_else(|| DtrError::new("GIT_CONFIG_COUNT is too large to extend safely"))?;
    let header_key = "http.https://github.com/.extraHeader";
    environment.extend([
        SecretEnvironment::new("GIT_CONFIG_COUNT", next_count.to_string()),
        SecretEnvironment::new(format!("GIT_CONFIG_KEY_{count}"), header_key),
        SecretEnvironment::new(format!("GIT_CONFIG_VALUE_{count}"), ""),
        SecretEnvironment::new(format!("GIT_CONFIG_KEY_{}", count + 1), header_key),
        SecretEnvironment::new(
            format!("GIT_CONFIG_VALUE_{}", count + 1),
            authorization_header(token),
        ),
    ]);
    Ok((
        environment,
        ["GH_TOKEN", "GITHUB_TOKEN"].map(OsString::from).to_vec(),
    ))
}

fn git_config_count_from(value: Option<&OsStr>) -> Result<usize, DtrError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(0);
    };
    value
        .to_str()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| DtrError::new("GIT_CONFIG_COUNT must be a non-negative integer"))
}

fn authorization_header(token: &str) -> String {
    let credential = format!("x-access-token:{token}");
    format!("Authorization: Basic {}", STANDARD.encode(credential))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_missing_empty_and_numeric_git_config_counts() {
        assert_eq!(git_config_count_from(None).unwrap(), 0);
        assert_eq!(git_config_count_from(Some(OsStr::new(""))).unwrap(), 0);
        assert_eq!(git_config_count_from(Some(OsStr::new("7"))).unwrap(), 7);
        assert!(git_config_count_from(Some(OsStr::new("-1"))).is_err());
        assert!(git_config_count_from(Some(OsStr::new("nope"))).is_err());
    }

    #[test]
    fn constructs_a_basic_header_for_the_selected_token() {
        let header = authorization_header("synthetic-token");
        let encoded = header
            .strip_prefix("Authorization: Basic ")
            .expect("basic authorization prefix");
        assert_eq!(
            STANDARD.decode(encoded).unwrap(),
            b"x-access-token:synthetic-token"
        );
    }
}
