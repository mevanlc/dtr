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

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct InstallSource {
    pub(crate) spec: RepoSpec,
    pub(crate) go_query: Option<String>,
}

impl InstallSource {
    pub(crate) fn parse(value: &OsStr) -> Result<Self, DtrError> {
        let path = Path::new(value);
        if path.is_absolute() || is_explicit_relative(value) {
            return Ok(Self {
                spec: RepoSpec::parse(value)?,
                go_query: None,
            });
        }

        let text = value.to_str().ok_or_else(|| {
            DtrError::new(
                "non-UTF-8 repository references must be local paths beginning /, ./, or ../",
            )
        })?;
        let Some((base, query)) = split_install_query(text)? else {
            return Ok(Self {
                spec: RepoSpec::parse(value)?,
                go_query: None,
            });
        };

        Ok(Self {
            spec: RepoSpec::parse(OsStr::new(&base))?,
            go_query: Some(query),
        })
    }
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

        if let Some(spec) = parse_github_scp_like(text)? {
            return Ok(spec);
        }

        if let Some((host, path)) = parse_scp_like(text) {
            return Ok(Self::ScpLike {
                remote: value.to_os_string(),
                host,
                path,
            });
        }

        let forge_shorthand = text.split_once('#').map_or(text, |(base, _)| base);
        let parts = forge_shorthand.split('/').collect::<Vec<_>>();
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

        let forge = if url.port().is_none() {
            match (url.scheme(), host.as_str()) {
                ("http" | "https", "github.com") => Some(Forge::GitHub),
                ("http" | "https", "gitlab.com") => Some(Forge::GitLab),
                ("ssh", "github.com") if url.username() == "git" => Some(Forge::GitHub),
                _ => None,
            }
        } else {
            None
        };

        if let Some(forge) = forge {
            if url.query().is_some() {
                return Err(DtrError::new(
                    "forge repository URLs must not contain a query string",
                ));
            }
            match url.scheme() {
                "http" | "https" if !url.username().is_empty() || url.password().is_some() => {
                    return Err(DtrError::new(
                        "forge repository URLs must not contain embedded credentials",
                    ));
                }
                "ssh" if url.password().is_some() => {
                    return Err(DtrError::new(
                        "GitHub SSH repository URLs must not contain a password",
                    ));
                }
                _ => {}
            }

            let mut segments = normalized_segments(url.path())?;
            if forge == Forge::GitHub {
                if segments.len() != 2 {
                    return Err(DtrError::new(
                        "GitHub references must identify a repository root such as https://github.com/owner/repo",
                    ));
                }
            } else {
                if segments.len() < 2 || segments.iter().any(|segment| segment == "-") {
                    return Err(DtrError::new(
                        "GitLab references must be repository-root URLs like https://gitlab.com/group/repo",
                    ));
                }
            }

            let repo = strip_dot_git(&segments.pop().expect("at least two segments")).to_owned();
            if repo.is_empty() {
                return Err(DtrError::new("repository name is empty"));
            }
            return Ok(Self::Forge {
                forge,
                host,
                namespace: segments,
                repo,
                remote: Some(OsString::from(
                    text.split_once('#')
                        .map_or(text, |(base, _)| base)
                        .trim_end_matches('/'),
                )),
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

    pub(crate) fn install_git_remote(
        &self,
        github_owner: Option<&str>,
    ) -> Result<OsString, DtrError> {
        match self {
            Self::Local { .. } => Err(DtrError::new(
                "local repositories use an installer path source, not a Git remote",
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

    pub(crate) fn inspection_git_remote(
        &self,
        github_owner: Option<&str>,
    ) -> Result<OsString, DtrError> {
        match self {
            Self::Local { .. } => Err(DtrError::new(
                "local repositories are inspected directly, not through Git",
            )),
            Self::Forge { .. } => self.install_git_remote(github_owner),
            Self::GitUrl { .. } => self.git_remote(),
            Self::ScpLike { remote, .. } => Ok(remote.clone()),
            Self::GithubMine { repo } => {
                let owner = github_owner.ok_or_else(|| {
                    DtrError::new("the authenticated GitHub owner has not been resolved")
                })?;
                Ok(format!("https://github.com/{owner}/{repo}.git").into())
            }
        }
    }

    pub(crate) fn python_package_source(
        &self,
        github_owner: Option<&str>,
    ) -> Result<OsString, DtrError> {
        if let Self::Local { path } = self {
            return Ok(path.as_os_str().to_os_string());
        }
        let remote = self.install_git_remote(github_owner)?;
        let remote = remote
            .to_str()
            .expect("non-local Git repository references are UTF-8");
        Ok(format!("git+{remote}").into())
    }

    pub(crate) fn npm_package_source(
        &self,
        github_owner: Option<&str>,
    ) -> Result<OsString, DtrError> {
        if let Self::Local { path } = self {
            return Ok(path.as_os_str().to_os_string());
        }
        let remote = self.install_git_remote(github_owner)?;
        let remote_text = remote
            .to_str()
            .expect("non-local Git repository references are UTF-8");
        if remote_text.starts_with("git://") {
            Ok(remote)
        } else {
            Ok(format!("git+{remote_text}").into())
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

fn split_install_query(text: &str) -> Result<Option<(String, String)>, DtrError> {
    if has_supported_url_scheme(text) {
        let mut url = Url::parse(text)
            .map_err(|error| DtrError::new(format!("invalid repository URL: {error}")))?;
        let Some((base_path, query)) = url.path().split_once('@') else {
            return Ok(None);
        };
        let base_path = base_path.to_owned();
        let query = validate_go_query(query)?;
        url.set_path(&base_path);
        return Ok(Some((url.into(), query)));
    }

    if let Some((prefix, path)) = text.split_once(':') {
        if let Some((base_path, query)) = path.split_once('@') {
            let query = validate_go_query(query)?;
            return Ok(Some((format!("{prefix}:{base_path}"), query)));
        }
        return Ok(None);
    }

    let Some((base, query)) = text.split_once('@') else {
        return Ok(None);
    };
    Ok(Some((base.to_owned(), validate_go_query(query)?)))
}

fn validate_go_query(query: &str) -> Result<String, DtrError> {
    if query.is_empty() {
        return Err(DtrError::new("Go version query after @ must not be empty"));
    }
    if query.contains('@') {
        return Err(DtrError::new("Go version query must not contain another @"));
    }
    if query.contains('%') {
        return Err(DtrError::new(
            "Go version query must not use percent-encoded characters",
        ));
    }
    if query.chars().any(char::is_whitespace) {
        return Err(DtrError::new(
            "Go version query must not contain whitespace",
        ));
    }
    Ok(query.to_owned())
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

fn parse_github_scp_like(text: &str) -> Result<Option<RepoSpec>, DtrError> {
    let remote = text.split_once('#').map_or(text, |(base, _)| base);
    let Some((left, path)) = remote.split_once(':') else {
        return Ok(None);
    };
    if !left.eq_ignore_ascii_case("git@github.com") {
        return Ok(None);
    }

    let segments = path.trim_matches('/').split('/').collect::<Vec<_>>();
    if segments.len() != 2 || !segments.iter().all(|part| valid_github_component(part)) {
        return Err(DtrError::new(
            "GitHub SSH references must identify a repository root such as git@github.com:owner/repo",
        ));
    }
    let repo = strip_dot_git(segments[1]);
    if repo.is_empty() {
        return Err(DtrError::new("repository name is empty"));
    }

    Ok(Some(RepoSpec::Forge {
        forge: Forge::GitHub,
        host: "github.com".to_owned(),
        namespace: vec![segments[0].to_owned()],
        repo: repo.to_owned(),
        remote: Some(OsString::from(remote.trim_end_matches('/'))),
    }))
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

    fn parse_install(value: &str) -> InstallSource {
        InstallSource::parse(OsStr::new(value)).expect("install source should parse")
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
    fn strips_fragments_from_recognized_forge_references() {
        for (value, slug, remote) in [
            (
                "https://github.com/owner/tool.git/#installation",
                "owner/tool",
                "https://github.com/owner/tool.git",
            ),
            (
                "https://gitlab.com/group/tool#readme",
                "group/tool",
                "https://gitlab.com/group/tool",
            ),
            (
                "git@github.com:owner/tool.git#readme",
                "owner/tool",
                "git@github.com:owner/tool.git",
            ),
            (
                "ssh://git@github.com/owner/tool.git#readme",
                "owner/tool",
                "ssh://git@github.com/owner/tool.git",
            ),
        ] {
            let spec = parse(value);
            assert_eq!(spec.forge_slug().as_deref(), Some(slug), "{value}");
            assert_eq!(
                spec.git_remote().unwrap(),
                OsString::from(remote),
                "{value}"
            );
        }

        let shorthand = parse("owner/tool#readme");
        assert_eq!(shorthand.forge_slug().as_deref(), Some("owner/tool"));
        assert_eq!(
            shorthand.git_remote().unwrap(),
            OsString::from("https://github.com/owner/tool.git")
        );
    }

    #[test]
    fn recognizes_gcl_github_ssh_forms_as_forge_repositories() {
        for value in [
            "git@github.com:owner/tool.git",
            "ssh://git@github.com/owner/tool.git",
        ] {
            let spec = parse(value);
            assert!(
                matches!(
                    spec,
                    RepoSpec::Forge {
                        forge: Forge::GitHub,
                        ..
                    }
                ),
                "{value}"
            );
            assert_eq!(spec.forge_slug().as_deref(), Some("owner/tool"), "{value}");
            assert_eq!(spec.git_remote().unwrap(), OsString::from(value), "{value}");
        }

        assert!(matches!(
            parse("ssh://owner@github.com/owner/tool.git"),
            RepoSpec::GitUrl { .. }
        ));
    }

    #[test]
    fn install_sources_separate_go_queries_from_remote_repositories() {
        for (value, expected_remote, expected_query) in [
            (
                "https://github.com/yuser/reepo@some-go-stuff",
                "https://github.com/yuser/reepo.git",
                "some-go-stuff",
            ),
            (
                "owner/reepo@feature/branch",
                "https://github.com/owner/reepo.git",
                "feature/branch",
            ),
            (
                "git@example.com:owner/reepo.git@deadbeef",
                "ssh://git@example.com/~/owner/reepo.git",
                "deadbeef",
            ),
        ] {
            let source = parse_install(value);
            assert_eq!(source.go_query.as_deref(), Some(expected_query), "{value}");
            assert_eq!(
                source.spec.install_git_remote(None).unwrap(),
                OsString::from(expected_remote),
                "{value}"
            );
        }
    }

    #[test]
    fn install_query_parsing_preserves_local_at_signs_and_ssh_usernames() {
        let local = parse_install("./reepo@some-go-stuff");
        assert!(matches!(local.spec, RepoSpec::Local { .. }));
        assert_eq!(local.go_query, None);

        let ssh = parse_install("git@example.com:owner/reepo.git");
        assert!(matches!(ssh.spec, RepoSpec::ScpLike { .. }));
        assert_eq!(ssh.go_query, None);
    }

    #[test]
    fn install_queries_reject_empty_repeated_and_percent_encoded_values() {
        for value in [
            "owner/reepo@",
            "owner/reepo@one@two",
            "https://github.com/owner/reepo@feature%2Fbranch",
        ] {
            assert!(InstallSource::parse(OsStr::new(value)).is_err(), "{value}");
        }
    }

    #[test]
    fn ordinary_repospec_parsing_keeps_clone_at_sign_semantics_unchanged() {
        let spec = parse("https://github.com/yuser/reepo@some-go-stuff");
        assert_eq!(
            spec.forge_slug().as_deref(),
            Some("yuser/reepo@some-go-stuff")
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
    fn rejects_forge_browser_pages_and_queries() {
        for value in [
            "https://github.com/o/r/tree/main",
            "https://github.com/o/r?tab=readme",
            "https://github.com/o/r/tree/main#readme",
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

        let fragmented = "https://example.com/path/tool.git#readme";
        assert_eq!(
            parse(fragmented).git_remote().unwrap(),
            OsString::from(fragmented)
        );
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
    fn install_remote_normalizes_forges_and_bare_github_names() {
        assert_eq!(
            parse("owner/tool")
                .install_git_remote(None)
                .unwrap()
                .to_str(),
            Some("https://github.com/owner/tool.git")
        );
        assert_eq!(
            parse("http://gitlab.com/group/subgroup/tool")
                .install_git_remote(None)
                .unwrap()
                .to_str(),
            Some("https://gitlab.com/group/subgroup/tool.git")
        );
        assert_eq!(
            parse("tool")
                .install_git_remote(Some("mevanlc"))
                .unwrap()
                .to_str(),
            Some("https://github.com/mevanlc/tool.git")
        );
    }

    #[test]
    fn install_remote_preserves_generic_urls_and_converts_scp_like_paths() {
        let generic = "ssh://git@example.com/srv/tool.git";
        assert_eq!(
            parse(generic).install_git_remote(None).unwrap().to_str(),
            Some(generic)
        );
        assert_eq!(
            parse("git@example.com:owner/tool.git")
                .install_git_remote(None)
                .unwrap()
                .to_str(),
            Some("ssh://git@example.com/~/owner/tool.git")
        );
        assert_eq!(
            parse("git@example.com:/srv/git/tool.git")
                .install_git_remote(None)
                .unwrap()
                .to_str(),
            Some("ssh://git@example.com/srv/git/tool.git")
        );
    }

    #[test]
    fn inspection_remote_normalizes_forges_but_preserves_generic_git_syntax() {
        assert_eq!(
            parse("http://github.com/owner/tool")
                .inspection_git_remote(None)
                .unwrap(),
            OsString::from("https://github.com/owner/tool.git")
        );
        assert_eq!(
            parse("git@example.com:owner/tool.git")
                .inspection_git_remote(None)
                .unwrap(),
            OsString::from("git@example.com:owner/tool.git")
        );
        assert_eq!(
            parse("tool")
                .inspection_git_remote(Some("mevanlc"))
                .unwrap(),
            OsString::from("https://github.com/mevanlc/tool.git")
        );
    }

    #[test]
    fn python_sources_preserve_paths_and_prefix_normalized_git_remotes() {
        assert_eq!(
            parse("./local tool").python_package_source(None).unwrap(),
            OsString::from("./local tool")
        );
        assert_eq!(
            parse("owner/tool")
                .python_package_source(None)
                .unwrap()
                .to_str(),
            Some("git+https://github.com/owner/tool.git")
        );
        assert_eq!(
            parse("git@example.com:owner/tool.git")
                .python_package_source(None)
                .unwrap()
                .to_str(),
            Some("git+ssh://git@example.com/~/owner/tool.git")
        );
        assert_eq!(
            parse("tool")
                .python_package_source(Some("mevanlc"))
                .unwrap()
                .to_str(),
            Some("git+https://github.com/mevanlc/tool.git")
        );
        assert_eq!(
            parse("https://example.com/tool.git?ref=main")
                .python_package_source(None)
                .unwrap()
                .to_str(),
            Some("git+https://example.com/tool.git?ref=main")
        );
    }

    #[cfg(unix)]
    #[test]
    fn package_sources_preserve_a_non_utf8_local_path() {
        use std::os::unix::ffi::{OsStrExt, OsStringExt};

        let path = OsString::from_vec(b"./tool-\xff".to_vec());
        let spec = RepoSpec::parse(&path).unwrap();
        assert_eq!(spec.python_package_source(None).unwrap(), path);
        assert_eq!(spec.npm_package_source(None).unwrap(), path);
        assert_eq!(
            spec.local_path().unwrap().as_os_str(),
            OsStr::from_bytes(b"./tool-\xff")
        );
    }

    #[test]
    fn npm_sources_use_supported_git_protocol_spellings() {
        assert_eq!(
            parse("owner/tool").npm_package_source(None).unwrap(),
            OsString::from("git+https://github.com/owner/tool.git")
        );
        assert_eq!(
            parse("https://example.com/tool.git")
                .npm_package_source(None)
                .unwrap(),
            OsString::from("git+https://example.com/tool.git")
        );
        assert_eq!(
            parse("git://example.com/owner/tool.git")
                .npm_package_source(None)
                .unwrap(),
            OsString::from("git://example.com/owner/tool.git")
        );
        assert_eq!(
            parse("git://example.com/owner/tool.git")
                .python_package_source(None)
                .unwrap(),
            OsString::from("git+git://example.com/owner/tool.git")
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
