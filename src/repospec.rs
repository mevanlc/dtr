use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use url::Url;

use crate::error::DtrError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Forge {
    GitHub,
    GitLab,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RepoSpec {
    Local {
        path: PathBuf,
    },
    Forge {
        forge: Forge,
        host: String,
        namespace: Vec<String>,
        repo: String,
        remote: Option<OsString>,
    },
    GitUrl {
        remote: OsString,
        authority: String,
        path: String,
        has_credentials: bool,
        has_query_or_fragment: bool,
        go_path_is_ambiguous: bool,
    },
    ScpLike {
        remote: OsString,
        host: String,
        path: String,
    },
    GithubMine {
        repo: String,
    },
}

impl RepoSpec {
    pub(crate) fn parse(value: &OsStr) -> Result<Self, DtrError> {
        let path = Path::new(value);
        if path.is_absolute() || is_explicit_relative(value) {
            return Ok(Self::Local {
                path: path.to_path_buf(),
            });
        }

        let text = value.to_str().ok_or_else(|| {
            DtrError::new(
                "non-UTF-8 repository references must be local paths beginning /, ./, or ../",
            )
        })?;

        if text.starts_with("scp://") || text.starts_with("sftp://") {
            return Err(DtrError::new(
                "scp:// and sftp:// repository staging is planned for a later milestone",
            ));
        }

        if has_supported_url_scheme(text) {
            return Self::parse_url(text, value);
        }

        if let Some((host, path)) = parse_scp_like(text) {
            return Ok(Self::ScpLike {
                remote: value.to_os_string(),
                host,
                path,
            });
        }

        let parts = text.split('/').collect::<Vec<_>>();
        if parts.len() == 2 && parts.iter().all(|part| valid_github_component(part)) {
            let repo = strip_dot_git(parts[1]);
            if repo.is_empty() {
                return Err(DtrError::new("repository name is empty"));
            }
            return Ok(Self::Forge {
                forge: Forge::GitHub,
                host: "github.com".to_owned(),
                namespace: vec![parts[0].to_owned()],
                repo: repo.to_owned(),
                remote: None,
            });
        }

        if valid_github_component(text) {
            let repo = strip_dot_git(text);
            if !repo.is_empty() {
                return Ok(Self::GithubMine {
                    repo: repo.to_owned(),
                });
            }
        }

        Err(DtrError::new(format!(
            "unsupported repository reference: {}",
            display_os(value)
        )))
    }

    fn parse_url(text: &str, original: &OsStr) -> Result<Self, DtrError> {
        let url = Url::parse(text)
            .map_err(|error| DtrError::new(format!("invalid repository URL: {error}")))?;
        let host = url
            .host_str()
            .ok_or_else(|| DtrError::new("repository URL is missing a hostname"))?
            .to_ascii_lowercase();

        if matches!(url.scheme(), "http" | "https")
            && url.port().is_none()
            && matches!(host.as_str(), "github.com" | "gitlab.com")
        {
            if url.query().is_some() || url.fragment().is_some() {
                return Err(DtrError::new(
                    "forge repository URLs must not contain a query string or fragment",
                ));
            }
            if !url.username().is_empty() || url.password().is_some() {
                return Err(DtrError::new(
                    "forge repository URLs must not contain embedded credentials",
                ));
            }

            let mut segments = normalized_segments(url.path())?;
            let forge = if host == "github.com" {
                if segments.len() != 2 {
                    return Err(DtrError::new(
                        "GitHub references must be repository-root URLs like https://github.com/owner/repo",
                    ));
                }
                Forge::GitHub
            } else {
                if segments.len() < 2 || segments.iter().any(|segment| segment == "-") {
                    return Err(DtrError::new(
                        "GitLab references must be repository-root URLs like https://gitlab.com/group/repo",
                    ));
                }
                Forge::GitLab
            };

            let repo = strip_dot_git(&segments.pop().expect("at least two segments")).to_owned();
            if repo.is_empty() {
                return Err(DtrError::new("repository name is empty"));
            }
            return Ok(Self::Forge {
                forge,
                host,
                namespace: segments,
                repo,
                remote: Some(OsString::from(text.trim_end_matches('/'))),
            });
        }

        let authority = match url.port() {
            Some(port) => format!("{host}:{port}"),
            None => host,
        };
        let path = generic_git_path(url.path())?;
        Ok(Self::GitUrl {
            remote: original.to_os_string(),
            authority,
            go_path_is_ambiguous: path.contains('%'),
            path,
            has_credentials: url.password().is_some()
                || (matches!(url.scheme(), "http" | "https") && !url.username().is_empty()),
            has_query_or_fragment: url.query().is_some() || url.fragment().is_some(),
        })
    }

    pub(crate) fn description(&self) -> String {
        match self {
            Self::Local { path } => format!("local repository {}", display_os(path.as_os_str())),
            Self::Forge {
                forge,
                namespace,
                repo,
                ..
            } => format!(
                "{} repository {}/{}",
                forge.label(),
                namespace.join("/"),
                repo
            ),
            Self::GitUrl { remote, .. } => {
                format!("Git URL {}", display_os(remote.as_os_str()))
            }
            Self::ScpLike { remote, .. } => {
                format!("SSH repository {}", display_os(remote.as_os_str()))
            }
            Self::GithubMine { repo } => format!("your GitHub repository {repo}"),
        }
    }

    pub(crate) fn forge_parts(&self) -> Option<(Forge, &[String], &str)> {
        match self {
            Self::Forge {
                forge,
                namespace,
                repo,
                ..
            } => Some((*forge, namespace, repo)),
            _ => None,
        }
    }

    pub(crate) fn forge_slug(&self) -> Option<String> {
        let (_, namespace, repo) = self.forge_parts()?;
        Some(format!("{}/{repo}", namespace.join("/")))
    }

    pub(crate) fn default_target(&self) -> Result<PathBuf, DtrError> {
        let name = match self {
            Self::Local { path } => path_repo_name(path.as_os_str())?,
            Self::Forge { repo, .. } | Self::GithubMine { repo } => OsString::from(repo),
            Self::GitUrl { path, .. } | Self::ScpLike { path, .. } => {
                OsString::from(last_path_component(path)?)
            }
        };
        Ok(PathBuf::from(strip_dot_git_os(name)))
    }

    pub(crate) fn git_remote(&self) -> Result<OsString, DtrError> {
        match self {
            Self::Local { path } => Ok(path.as_os_str().to_os_string()),
            Self::Forge {
                forge,
                host,
                remote,
                ..
            } => {
                if let Some(remote) = remote {
                    Ok(remote.clone())
                } else if *forge == Forge::GitHub {
                    Ok(format!(
                        "https://{host}/{}.git",
                        self.forge_slug().expect("forge has slug")
                    )
                    .into())
                } else {
                    Err(DtrError::new("cannot construct GitLab remote"))
                }
            }
            Self::GitUrl { remote, .. } | Self::ScpLike { remote, .. } => Ok(remote.clone()),
            Self::GithubMine { .. } => Err(DtrError::new(
                "a bare GitHub repository name requires the gh CLI",
            )),
        }
    }

    pub(crate) fn go_import_path(&self, github_owner: Option<&str>) -> Result<String, DtrError> {
        match self {
            Self::Local { .. } => Err(DtrError::new(
                "local repositories do not have a remote Go import path",
            )),
            Self::Forge {
                host,
                namespace,
                repo,
                ..
            } => Ok(format!("{host}/{}/{repo}", namespace.join("/"))),
            Self::GitUrl {
                authority,
                path,
                has_credentials,
                has_query_or_fragment,
                go_path_is_ambiguous,
                ..
            } => {
                if *has_credentials || *has_query_or_fragment || *go_path_is_ambiguous {
                    return Err(DtrError::new(
                        "cannot derive a Go import path from a URL with credentials or an encoded, queried, or fragmented path",
                    ));
                }
                Ok(format!("{authority}/{path}"))
            }
            Self::ScpLike { host, path, .. } => {
                if path.contains(['%', '?', '#']) {
                    return Err(DtrError::new(
                        "cannot derive a Go import path from an encoded, queried, or fragmented SSH path",
                    ));
                }
                Ok(format!("{host}/{path}"))
            }
            Self::GithubMine { repo } => {
                let owner = github_owner.ok_or_else(|| {
                    DtrError::new("the authenticated GitHub owner has not been resolved")
                })?;
                Ok(format!("github.com/{owner}/{repo}"))
            }
        }
    }

    pub(crate) fn cargo_git_remote(
        &self,
        github_owner: Option<&str>,
    ) -> Result<OsString, DtrError> {
        match self {
            Self::Local { .. } => Err(DtrError::new(
                "local repositories use cargo install --path, not --git",
            )),
            Self::Forge {
                host,
                namespace,
                repo,
                ..
            } => Ok(format!("https://{host}/{}/{}.git", namespace.join("/"), repo).into()),
            Self::GitUrl { remote, .. } => Ok(remote.clone()),
            Self::ScpLike { remote, .. } => cargo_ssh_url(remote),
            Self::GithubMine { repo } => {
                let owner = github_owner.ok_or_else(|| {
                    DtrError::new("the authenticated GitHub owner has not been resolved")
                })?;
                Ok(format!("https://github.com/{owner}/{repo}.git").into())
            }
        }
    }

    pub(crate) fn is_local(&self) -> bool {
        matches!(self, Self::Local { .. })
    }

    pub(crate) fn local_path(&self) -> Option<&Path> {
        match self {
            Self::Local { path } => Some(path),
            _ => None,
        }
    }
}

fn cargo_ssh_url(remote: &OsStr) -> Result<OsString, DtrError> {
    let text = remote
        .to_str()
        .ok_or_else(|| DtrError::new("SCP-like repository references must be UTF-8"))?;
    let (authority, path) = text
        .split_once(':')
        .expect("validated SCP-like repository has a colon");
    if let Some(absolute) = path.strip_prefix('/') {
        Ok(format!("ssh://{authority}/{absolute}").into())
    } else {
        Ok(format!("ssh://{authority}/~/{path}").into())
    }
}

impl Forge {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::GitHub => "GitHub",
            Self::GitLab => "GitLab",
        }
    }
}

fn has_supported_url_scheme(text: &str) -> bool {
    ["http://", "https://", "ssh://", "git://"]
        .iter()
        .any(|scheme| text.starts_with(scheme))
}

fn normalized_segments(path: &str) -> Result<Vec<String>, DtrError> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Err(DtrError::new("repository URL path is empty"));
    }
    let segments = trimmed.split('/').map(str::to_owned).collect::<Vec<_>>();
    if segments.iter().any(|segment| {
        segment.is_empty() || segment == "." || segment == ".." || segment.contains('%')
    }) {
        return Err(DtrError::new(
            "repository URL contains an empty, relative, or percent-encoded path segment",
        ));
    }
    Ok(segments)
}

fn generic_git_path(path: &str) -> Result<String, DtrError> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Err(DtrError::new("repository URL path is empty"));
    }
    Ok(strip_dot_git(trimmed).to_owned())
}

fn parse_scp_like(text: &str) -> Option<(String, String)> {
    if text.contains(char::is_whitespace) {
        return None;
    }
    let (left, right) = text.split_once(':')?;
    if left.is_empty() || right.is_empty() || left.contains('/') {
        return None;
    }
    let host = left.rsplit_once('@').map_or(left, |(_, host)| host);
    if host.is_empty() {
        return None;
    }
    let path = right.trim_matches('/');
    if path.is_empty() {
        return None;
    }
    Some((host.to_owned(), strip_dot_git(path).to_owned()))
}

fn valid_github_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn strip_dot_git(value: &str) -> &str {
    value.strip_suffix(".git").unwrap_or(value)
}

fn strip_dot_git_os(value: OsString) -> OsString {
    if let Some(text) = value.to_str()
        && let Some(stripped) = text.strip_suffix(".git")
    {
        return OsString::from(stripped);
    }
    value
}

fn path_repo_name(value: &OsStr) -> Result<OsString, DtrError> {
    let path = Path::new(value);
    if let Some(name) = path.file_name() {
        return Ok(name.to_os_string());
    }

    let canonical = path.canonicalize().map_err(|error| {
        DtrError::new(format!(
            "cannot derive a clone directory from {}: {error}",
            display_os(value)
        ))
    })?;
    canonical
        .file_name()
        .map(OsStr::to_os_string)
        .ok_or_else(|| DtrError::new("repository path has no final component"))
}

fn last_path_component(path: &str) -> Result<&str, DtrError> {
    path.rsplit('/')
        .next()
        .filter(|part| !part.is_empty())
        .map(strip_dot_git)
        .ok_or_else(|| DtrError::new("repository path has no final component"))
}

#[cfg(unix)]
fn is_explicit_relative(value: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let bytes = value.as_bytes();
    bytes.starts_with(b"./") || bytes.starts_with(b"../")
}

#[cfg(not(unix))]
fn is_explicit_relative(value: &OsStr) -> bool {
    value
        .to_str()
        .is_some_and(|text| text.starts_with("./") || text.starts_with("../"))
}

fn display_os(value: &OsStr) -> String {
    value.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(value: &str) -> RepoSpec {
        RepoSpec::parse(OsStr::new(value)).expect("repospec should parse")
    }

    #[test]
    fn classifies_explicit_local_paths_before_shorthand() {
        assert!(matches!(parse("./owner/repo"), RepoSpec::Local { .. }));
        assert!(matches!(parse("../owner/repo"), RepoSpec::Local { .. }));
        assert!(matches!(parse("/owner/repo"), RepoSpec::Local { .. }));
        assert!(matches!(
            parse("owner/repo"),
            RepoSpec::Forge {
                forge: Forge::GitHub,
                ..
            }
        ));
    }

    #[test]
    fn parses_bare_github_repo() {
        assert_eq!(
            parse("gittop"),
            RepoSpec::GithubMine {
                repo: "gittop".to_owned()
            }
        );
    }

    #[test]
    fn parses_github_url_and_normalizes_name() {
        let spec = parse("https://github.com/hjr265/gittop.git/");
        assert_eq!(spec.forge_slug().as_deref(), Some("hjr265/gittop"));
        assert_eq!(spec.default_target().unwrap(), PathBuf::from("gittop"));
        assert_eq!(
            spec.go_import_path(None).unwrap(),
            "github.com/hjr265/gittop"
        );
    }

    #[test]
    fn parses_nested_gitlab_namespace() {
        let spec = parse("https://gitlab.com/group/subgroup/tool.git");
        assert_eq!(spec.forge_slug().as_deref(), Some("group/subgroup/tool"));
        assert_eq!(
            spec.go_import_path(None).unwrap(),
            "gitlab.com/group/subgroup/tool"
        );
    }

    #[test]
    fn rejects_forge_browser_pages_queries_and_fragments() {
        for value in [
            "https://github.com/o/r/tree/main",
            "https://github.com/o/r?tab=readme",
            "https://github.com/o/r#readme",
            "https://gitlab.com/o/r/-/blob/main/README.md",
        ] {
            assert!(RepoSpec::parse(OsStr::new(value)).is_err(), "{value}");
        }
    }

    #[test]
    fn parses_generic_urls() {
        for value in [
            "https://example.com/path/tool.git",
            "ssh://example.com/path/tool",
            "git://example.com/path/tool",
        ] {
            assert!(matches!(parse(value), RepoSpec::GitUrl { .. }), "{value}");
        }
    }

    #[test]
    fn parses_scp_like_git_remote() {
        let spec = parse("git@example.com:owner/tool.git");
        assert_eq!(spec.go_import_path(None).unwrap(), "example.com/owner/tool");

        let trailing = parse("git@example.com:owner/tool.git/");
        assert_eq!(
            trailing.go_import_path(None).unwrap(),
            "example.com/owner/tool"
        );
    }

    #[test]
    fn cargo_remote_normalizes_forges_and_bare_github_names() {
        assert_eq!(
            parse("owner/tool").cargo_git_remote(None).unwrap().to_str(),
            Some("https://github.com/owner/tool.git")
        );
        assert_eq!(
            parse("http://gitlab.com/group/subgroup/tool")
                .cargo_git_remote(None)
                .unwrap()
                .to_str(),
            Some("https://gitlab.com/group/subgroup/tool.git")
        );
        assert_eq!(
            parse("tool")
                .cargo_git_remote(Some("mevanlc"))
                .unwrap()
                .to_str(),
            Some("https://github.com/mevanlc/tool.git")
        );
    }

    #[test]
    fn cargo_remote_preserves_generic_urls_and_converts_scp_like_paths() {
        let generic = "ssh://git@example.com/srv/tool.git";
        assert_eq!(
            parse(generic).cargo_git_remote(None).unwrap().to_str(),
            Some(generic)
        );
        assert_eq!(
            parse("git@example.com:owner/tool.git")
                .cargo_git_remote(None)
                .unwrap()
                .to_str(),
            Some("ssh://git@example.com/~/owner/tool.git")
        );
        assert_eq!(
            parse("git@example.com:/srv/git/tool.git")
                .cargo_git_remote(None)
                .unwrap()
                .to_str(),
            Some("ssh://git@example.com/srv/git/tool.git")
        );
    }

    #[test]
    fn parks_literal_scp_and_sftp_urls() {
        for value in ["scp://example.com/path", "sftp://example.com/path"] {
            let error = RepoSpec::parse(OsStr::new(value)).unwrap_err();
            assert!(error.to_string().contains("later milestone"));
        }
    }

    #[test]
    fn generic_url_with_query_is_cloneable_but_not_go_installable() {
        let spec = parse("https://example.com/path/tool.git?ref=main");
        assert!(spec.go_import_path(None).is_err());
    }

    #[test]
    fn encoded_generic_url_is_cloneable_but_not_go_installable() {
        let spec = parse("https://example.com/path/tool%20name.git");
        assert!(matches!(spec, RepoSpec::GitUrl { .. }));
        assert!(spec.go_import_path(None).is_err());
    }

    #[test]
    fn ssh_transport_username_is_not_a_go_import_credential() {
        let spec = parse("ssh://git@example.com/owner/tool.git");
        assert_eq!(spec.go_import_path(None).unwrap(), "example.com/owner/tool");
    }

    #[cfg(unix)]
    #[test]
    fn accepts_non_utf8_local_path() {
        use std::os::unix::ffi::OsStrExt;

        let path = OsStr::from_bytes(b"./repo-\xff");
        assert!(matches!(RepoSpec::parse(path), Ok(RepoSpec::Local { .. })));
    }
}
