use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde::Deserialize;

use crate::cli::InstallTool;
use crate::command::{apply_environment, command_exists};
use crate::error::DtrError;
use crate::github_auth::{self, GithubAuthSelection};
use crate::repospec::{Forge, RepoSpec};

const EXPLICIT_TOOL_SUGGESTION: &str =
    "use --tool <go|cargo|uv|pipx|npm> to select one explicitly (rust is an alias for cargo)";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Ecosystem {
    Go,
    Cargo,
    Python,
    Npm,
}

const MARKERS: [(Ecosystem, &str); 6] = [
    (Ecosystem::Go, "go.mod"),
    (Ecosystem::Cargo, "Cargo.toml"),
    (Ecosystem::Python, "pyproject.toml"),
    (Ecosystem::Python, "setup.py"),
    (Ecosystem::Python, "setup.cfg"),
    (Ecosystem::Npm, "package.json"),
];

struct RootEntry {
    name: Vec<u8>,
    file_like: bool,
}

impl RootEntry {
    fn file(name: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            file_like: true,
        }
    }

    fn directory(name: impl Into<Vec<u8>>) -> Self {
        Self {
            name: name.into(),
            file_like: false,
        }
    }
}

pub(crate) fn detect_tool(
    spec: &RepoSpec,
    github_owner: Option<&str>,
    github_selection: Option<&GithubAuthSelection>,
) -> Result<InstallTool, DtrError> {
    if let Some(path) = spec.local_path() {
        return detect_local_tool(path);
    }
    let entries = inspect_root(spec, github_owner, github_selection)?;
    infer_tool(&entries, command_exists)
}

fn detect_local_tool(path: &Path) -> Result<InstallTool, DtrError> {
    let entries = inspect_local_root(path)?;
    if has_supported_manifest(&entries) {
        return infer_tool(&entries, command_exists);
    }
    if has_non_test_go_source(&entries)
        && git_worktree_root(path).is_some_and(|root| has_ancestor_go_module(path, &root))
    {
        return Ok(InstallTool::Go);
    }
    infer_tool(&entries, command_exists)
}

fn has_supported_manifest(entries: &[RootEntry]) -> bool {
    entries.iter().any(|entry| {
        entry.file_like
            && MARKERS
                .iter()
                .any(|(_, manifest)| entry.name == manifest.as_bytes())
    })
}

fn has_non_test_go_source(entries: &[RootEntry]) -> bool {
    entries.iter().any(|entry| {
        entry.file_like && entry.name.ends_with(b".go") && !entry.name.ends_with(b"_test.go")
    })
}

fn git_worktree_root(path: &Path) -> Option<PathBuf> {
    if !command_exists("git") {
        return None;
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    git_path_output(output.stdout)
}

fn git_path_output(mut output: Vec<u8>) -> Option<PathBuf> {
    while matches!(output.last(), Some(b'\n' | b'\r')) {
        output.pop();
    }
    if output.is_empty() {
        return None;
    }

    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;

        Some(PathBuf::from(OsString::from_vec(output)))
    }
    #[cfg(not(unix))]
    {
        String::from_utf8(output).ok().map(PathBuf::from)
    }
}

fn has_ancestor_go_module(path: &Path, worktree_root: &Path) -> bool {
    let Ok(path) = path.canonicalize() else {
        return false;
    };
    let Ok(worktree_root) = worktree_root.canonicalize() else {
        return false;
    };
    if !path.starts_with(&worktree_root) {
        return false;
    }

    let mut directory = path.parent();
    while let Some(ancestor) = directory {
        if ancestor.join("go.mod").is_file() {
            return true;
        }
        if ancestor == worktree_root {
            break;
        }
        directory = ancestor.parent();
    }
    false
}

fn infer_tool(
    entries: &[RootEntry],
    command_available: impl Fn(&str) -> bool,
) -> Result<InstallTool, DtrError> {
    let evidence = MARKERS
        .iter()
        .filter(|(_, marker)| {
            entries
                .iter()
                .any(|entry| entry.file_like && entry.name == marker.as_bytes())
        })
        .copied()
        .collect::<Vec<_>>();

    let mut ecosystems = Vec::new();
    for (ecosystem, _) in &evidence {
        if !ecosystems.contains(ecosystem) {
            ecosystems.push(*ecosystem);
        }
    }

    if ecosystems.is_empty() {
        return Err(DtrError::new(format!(
            "could not determine an install tool from the repository root; no supported manifest was found; {EXPLICIT_TOOL_SUGGESTION}"
        )));
    }
    if ecosystems.len() > 1 {
        let markers = evidence
            .iter()
            .map(|(ecosystem, marker)| format!("{marker} ({})", ecosystem.label()))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(DtrError::new(format!(
            "could not determine one install tool from the repository root; found {markers}; {EXPLICIT_TOOL_SUGGESTION}"
        )));
    }

    match ecosystems[0] {
        Ecosystem::Go => Ok(InstallTool::Go),
        Ecosystem::Cargo => Ok(InstallTool::Cargo),
        Ecosystem::Npm => Ok(InstallTool::Npm),
        Ecosystem::Python if command_available("uv") => Ok(InstallTool::Uv),
        Ecosystem::Python if command_available("pipx") => Ok(InstallTool::Pipx),
        Ecosystem::Python => Err(DtrError::new(
            "the repository root identifies a Python project, but neither uv nor pipx was found on PATH; install one or use --tool <uv|pipx> after making it available",
        )),
    }
}

impl Ecosystem {
    fn label(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::Cargo => "cargo",
            Self::Python => "uv/pipx",
            Self::Npm => "npm",
        }
    }
}

fn inspect_root(
    spec: &RepoSpec,
    github_owner: Option<&str>,
    github_selection: Option<&GithubAuthSelection>,
) -> Result<Vec<RootEntry>, DtrError> {
    let mut failed_attempts = Vec::new();
    if let Some((owner, repo)) = github_repository(spec, github_owner)
        && command_exists("gh")
    {
        match inspect_github_root(owner, repo, github_selection) {
            Ok(entries) => return Ok(entries),
            Err(error) => failed_attempts.push(error),
        }
    }
    if let Some(project) = gitlab_project(spec)
        && command_exists("glab")
    {
        match inspect_gitlab_root(&project) {
            Ok(entries) => return Ok(entries),
            Err(error) => failed_attempts.push(error),
        }
    }

    if command_exists("git") {
        match inspect_git_root(spec, github_owner, github_selection) {
            Ok(entries) => return Ok(entries),
            Err(error) => failed_attempts.push(error),
        }
    } else {
        failed_attempts.push("git was not found on PATH for fallback inspection".to_owned());
    }

    Err(DtrError::new(format!(
        "could not inspect the repository root automatically: {}; {EXPLICIT_TOOL_SUGGESTION}",
        failed_attempts.join("; ")
    )))
}

fn inspect_local_root(path: &Path) -> Result<Vec<RootEntry>, DtrError> {
    let directory = fs::read_dir(path).map_err(|error| {
        DtrError::new(format!(
            "could not inspect local repository root {}: {error}",
            path.display()
        ))
    })?;
    let mut entries = Vec::new();
    for entry in directory {
        let entry = entry.map_err(|error| {
            DtrError::new(format!(
                "could not inspect local repository root {}: {error}",
                path.display()
            ))
        })?;
        let file_type = entry.file_type().map_err(|error| {
            DtrError::new(format!(
                "could not inspect local repository entry {}: {error}",
                entry.path().display()
            ))
        })?;
        entries.push(RootEntry {
            name: entry.file_name().as_encoded_bytes().to_vec(),
            file_like: !file_type.is_dir(),
        });
    }
    Ok(entries)
}

fn github_repository<'a>(
    spec: &'a RepoSpec,
    github_owner: Option<&'a str>,
) -> Option<(&'a str, &'a str)> {
    match spec {
        RepoSpec::Forge {
            forge: Forge::GitHub,
            namespace,
            repo,
            ..
        } => Some((&namespace[0], repo)),
        RepoSpec::GithubMine { repo } => github_owner.map(|owner| (owner, repo.as_str())),
        _ => None,
    }
}

#[derive(Deserialize)]
struct GithubTree {
    truncated: bool,
    tree: Vec<GithubTreeEntry>,
}

#[derive(Deserialize)]
struct GithubTreeEntry {
    path: String,
    #[serde(rename = "type")]
    kind: String,
}

fn inspect_github_root(
    owner: &str,
    repo: &str,
    selection: Option<&GithubAuthSelection>,
) -> Result<Vec<RootEntry>, String> {
    let endpoint = format!("repos/{owner}/{repo}/git/trees/HEAD");
    let mut command = Command::new("gh");
    command.args(["api", &endpoint, "--hostname", "github.com"]);
    if let Some(selection) = selection {
        command
            .env("GH_TOKEN", &selection.token)
            .env_remove("GITHUB_TOKEN");
    }
    let output = command
        .output()
        .map_err(|error| format!("could not start gh for GitHub root inspection: {error}"))?;
    if !output.status.success() {
        return Err(output_error("GitHub API root inspection", &output));
    }
    let response: GithubTree = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("GitHub API returned invalid root-tree JSON: {error}"))?;
    if response.truncated {
        return Err("GitHub API returned a truncated root tree".to_owned());
    }
    Ok(response
        .tree
        .into_iter()
        .map(|entry| RootEntry {
            name: entry.path.into_bytes(),
            file_like: entry.kind == "blob",
        })
        .collect())
}

fn gitlab_project(spec: &RepoSpec) -> Option<String> {
    let RepoSpec::Forge {
        forge: Forge::GitLab,
        namespace,
        repo,
        ..
    } = spec
    else {
        return None;
    };
    Some(format!("{}/{repo}", namespace.join("/")))
}

#[derive(Deserialize)]
struct GitlabTreeEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
}

fn inspect_gitlab_root(project: &str) -> Result<Vec<RootEntry>, String> {
    let encoded_project =
        url::form_urlencoded::byte_serialize(project.as_bytes()).collect::<String>();
    let endpoint =
        format!("projects/{encoded_project}/repository/tree?pagination=keyset&per_page=100");
    let output = Command::new("glab")
        .args(["api", &endpoint, "--hostname", "gitlab.com", "--paginate"])
        .output()
        .map_err(|error| format!("could not start glab for GitLab root inspection: {error}"))?;
    if !output.status.success() {
        return Err(output_error("GitLab API root inspection", &output));
    }

    let mut entries = Vec::new();
    let mut pages = 0;
    for page in
        serde_json::Deserializer::from_slice(&output.stdout).into_iter::<Vec<GitlabTreeEntry>>()
    {
        let page =
            page.map_err(|error| format!("GitLab API returned invalid tree JSON: {error}"))?;
        pages += 1;
        entries.extend(page.into_iter().map(|entry| RootEntry {
            name: entry.name.into_bytes(),
            file_like: entry.kind == "blob",
        }));
    }
    if pages == 0 {
        return Err("GitLab API returned no tree JSON".to_owned());
    }
    Ok(entries)
}

fn inspect_git_root(
    spec: &RepoSpec,
    github_owner: Option<&str>,
    github_selection: Option<&GithubAuthSelection>,
) -> Result<Vec<RootEntry>, String> {
    let remote = spec
        .inspection_git_remote(github_owner)
        .map_err(|error| error.to_string())?;
    let temporary = tempfile::tempdir()
        .map_err(|error| format!("could not create a temporary inspection directory: {error}"))?;
    let repository = temporary.path().join("repository");
    let (environment, removed_environment) = match github_selection {
        Some(selection) => github_auth::git_environment(&selection.token).map_err(|error| {
            format!("could not prepare GitHub inspection authentication: {error}")
        })?,
        None => (Vec::new(), Vec::new()),
    };

    let mut clone = Command::new("git");
    clone.args([
        OsStr::new("clone"),
        OsStr::new("--quiet"),
        OsStr::new("--depth=1"),
        OsStr::new("--single-branch"),
        OsStr::new("--no-tags"),
        OsStr::new("--filter=blob:none"),
        OsStr::new("--no-checkout"),
        OsStr::new("--"),
    ]);
    clone.arg(&remote).arg(&repository);
    apply_environment(&mut clone, &environment, &removed_environment);
    let output = clone
        .output()
        .map_err(|error| format!("could not start git for repository root inspection: {error}"))?;
    if !output.status.success() {
        return Err(output_error("filtered Git root inspection", &output));
    }

    let mut list = Command::new("git");
    list.arg("-C")
        .arg(&repository)
        .args(["ls-tree", "-z", "HEAD"]);
    apply_environment(&mut list, &environment, &removed_environment);
    let output = list
        .output()
        .map_err(|error| format!("could not list the temporary Git root tree: {error}"))?;
    if !output.status.success() {
        return Err(output_error("temporary Git root-tree listing", &output));
    }
    parse_git_tree(&output.stdout)
}

fn parse_git_tree(output: &[u8]) -> Result<Vec<RootEntry>, String> {
    let mut entries = Vec::new();
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let tab = record
            .iter()
            .position(|byte| *byte == b'\t')
            .ok_or_else(|| "git ls-tree returned an entry without a path".to_owned())?;
        let mut fields = record[..tab].split(|byte| *byte == b' ');
        let _mode = fields.next();
        let kind = fields
            .next()
            .ok_or_else(|| "git ls-tree returned an entry without an object type".to_owned())?;
        entries.push(if kind == b"blob" {
            RootEntry::file(record[tab + 1..].to_vec())
        } else {
            RootEntry::directory(record[tab + 1..].to_vec())
        });
    }
    Ok(entries)
}

fn output_error(context: &str, output: &Output) -> String {
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    let diagnostic = diagnostic.trim();
    if diagnostic.is_empty() {
        format!("{context} exited with {}", output.status)
    } else {
        format!("{context} failed: {diagnostic}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(names: &[&str]) -> Vec<RootEntry> {
        names
            .iter()
            .map(|name| RootEntry::file(name.as_bytes().to_vec()))
            .collect()
    }

    #[test]
    fn recognizes_each_supported_ecosystem() {
        assert_eq!(
            infer_tool(&files(&["go.mod"]), |_| true).unwrap(),
            InstallTool::Go
        );
        assert_eq!(
            infer_tool(&files(&["Cargo.toml"]), |_| true).unwrap(),
            InstallTool::Cargo
        );
        assert_eq!(
            infer_tool(&files(&["package.json"]), |_| true).unwrap(),
            InstallTool::Npm
        );
    }

    #[test]
    fn python_prefers_uv_then_pipx() {
        let entries = files(&["pyproject.toml", "setup.cfg"]);
        assert_eq!(
            infer_tool(&entries, |program| matches!(program, "uv" | "pipx")).unwrap(),
            InstallTool::Uv
        );
        assert_eq!(
            infer_tool(&entries, |program| program == "pipx").unwrap(),
            InstallTool::Pipx
        );
        assert!(infer_tool(&entries, |_| false).is_err());
    }

    #[test]
    fn mixed_or_missing_ecosystems_decline() {
        let mixed = infer_tool(&files(&["Cargo.toml", "package.json"]), |_| true)
            .unwrap_err()
            .to_string();
        assert!(mixed.contains("Cargo.toml (cargo)"));
        assert!(mixed.contains("package.json (npm)"));
        assert!(infer_tool(&files(&["README.md"]), |_| true).is_err());
    }

    #[test]
    fn markers_are_exact_file_names() {
        let entries = vec![
            RootEntry::directory(b"Cargo.toml".to_vec()),
            RootEntry::file(b"cargo.toml".to_vec()),
            RootEntry::file(b"uv.lock".to_vec()),
        ];
        assert!(infer_tool(&entries, |_| true).is_err());
    }

    #[test]
    fn go_source_trigger_accepts_any_non_test_go_filename() {
        assert!(has_non_test_go_source(&files(&["entrypoint.go"])));
        assert!(has_non_test_go_source(&files(&["main.go", "README.md"])));
        assert!(!has_non_test_go_source(&files(&["main_test.go"])));
        assert!(!has_non_test_go_source(&files(&["MAIN.GO"])));
        assert!(!has_non_test_go_source(&[RootEntry::directory(b"main.go")]));
    }

    #[test]
    fn ancestor_go_module_search_stops_at_the_worktree_root() {
        let temporary = tempfile::tempdir().unwrap();
        let outer = temporary.path();
        let worktree = outer.join("repo");
        let command = worktree.join("cmd/tool");
        fs::create_dir_all(&command).unwrap();

        fs::write(outer.join("go.mod"), "module outside\n").unwrap();
        assert!(!has_ancestor_go_module(&command, &worktree));

        fs::write(worktree.join("go.mod"), "module example.com/tool\n").unwrap();
        assert!(has_ancestor_go_module(&command, &worktree));
    }

    #[test]
    fn git_path_output_removes_only_trailing_line_endings() {
        assert_eq!(
            git_path_output(b"/tmp/line\nbreak\r\n".to_vec()).unwrap(),
            PathBuf::from("/tmp/line\nbreak")
        );
        assert_eq!(git_path_output(b"\r\n".to_vec()), None);
    }

    #[test]
    fn parses_git_tree_records_without_line_based_path_assumptions() {
        let output = b"100644 blob abc\tCargo.toml\0\
100644 blob def\todd\nname\0\
040000 tree ghi\tpackage.json\0";
        let entries = parse_git_tree(output).unwrap();
        assert_eq!(infer_tool(&entries, |_| true).unwrap(), InstallTool::Cargo);
    }
}
