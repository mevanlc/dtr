use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::process::Command;

use crate::error::DtrError;
use crate::repospec::RepoSpec;

pub(crate) const HELP: &str = r#"Usage: dtr [--explain|-n] clone [options] <dtr-repospec> [dir]

Clone a local or remote repository using git, gh, or glab.

dtr options:
  -O, --name-owner  if [dir] is omitted, use namespace--repo
  -D                if [dir] is omitted, use namespace/repo
  -h, --help        print help

Recognized git clone options may appear in normal git-clone positions.
Use `dtr -n clone ...` to explain; `dtr clone -n ...` means git --no-checkout.
"#;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NameMode {
    Default,
    NameOwner,
    OwnerDirectory,
}

#[derive(Debug)]
pub(crate) struct CloneRequest {
    pub(crate) spec: RepoSpec,
    pub(crate) directory: Option<OsString>,
    pub(crate) git_options: Vec<OsString>,
    pub(crate) name_mode: NameMode,
}

pub(crate) enum ParsedClone {
    Help,
    Request(CloneRequest),
}

#[derive(Debug)]
struct GitOptionTable {
    arity: HashMap<String, bool>,
}

pub(crate) fn parse_clone_args(argv: Vec<OsString>) -> Result<ParsedClone, DtrError> {
    if help_requested(&argv) {
        return Ok(ParsedClone::Help);
    }
    if argv.is_empty() {
        return Err(DtrError::new(
            "missing <dtr-repospec>\n\nTry 'dtr clone --help' for more information.",
        ));
    }

    let table = GitOptionTable::discover()?;
    let mut git_options = Vec::new();
    let mut positionals = Vec::new();
    let mut name_mode = NameMode::Default;
    let mut after_double_dash = false;
    let mut index = 0;

    while index < argv.len() {
        let argument = &argv[index];
        if after_double_dash {
            positionals.push(argument.clone());
            index += 1;
            continue;
        }

        if argument == OsStr::new("--") {
            after_double_dash = true;
            index += 1;
            continue;
        }

        match argument.to_str() {
            Some("-O" | "--name-owner") => {
                if name_mode == NameMode::OwnerDirectory {
                    return Err(DtrError::new(
                        "-O/--name-owner and -D are mutually exclusive",
                    ));
                }
                name_mode = NameMode::NameOwner;
                index += 1;
            }
            Some("-D") => {
                if name_mode == NameMode::NameOwner {
                    return Err(DtrError::new(
                        "-O/--name-owner and -D are mutually exclusive",
                    ));
                }
                name_mode = NameMode::OwnerDirectory;
                index += 1;
            }
            Some(text) if text.starts_with('-') && text != "-" => {
                let consumed = table.parse_option(&argv, index, &mut git_options)?;
                index += consumed;
            }
            _ => {
                positionals.push(argument.clone());
                index += 1;
            }
        }
    }

    if positionals.is_empty() {
        return Err(DtrError::new("missing <dtr-repospec>"));
    }
    if positionals.len() > 2 {
        return Err(DtrError::new(format!(
            "too many positional arguments (expected <dtr-repospec> [dir], found {})",
            positionals.len()
        )));
    }

    let spec = RepoSpec::parse(&positionals[0])?;
    let directory = positionals.get(1).cloned();
    Ok(ParsedClone::Request(CloneRequest {
        spec,
        directory,
        git_options,
        name_mode,
    }))
}

fn help_requested(argv: &[OsString]) -> bool {
    for argument in argv {
        if argument == OsStr::new("--") {
            return false;
        }
        if argument == OsStr::new("-h") || argument == OsStr::new("--help") {
            return true;
        }
    }
    false
}

impl GitOptionTable {
    fn discover() -> Result<Self, DtrError> {
        let output = Command::new("git")
            .args(["clone", "-h"])
            .env("LC_ALL", "C")
            .env("COLUMNS", "999")
            .output()
            .map_err(|error| {
                DtrError::new(format!(
                    "cannot inspect 'git clone -h'; is git installed and on PATH? ({error})"
                ))
            })?;

        let mut help = String::from_utf8_lossy(&output.stdout).into_owned();
        help.push_str(&String::from_utf8_lossy(&output.stderr));
        let table = Self::from_help(&help);
        if table.arity.is_empty() {
            return Err(DtrError::new(
                "could not infer the git clone option surface from 'git clone -h'",
            ));
        }
        Ok(table)
    }

    fn from_help(help: &str) -> Self {
        let mut arity = HashMap::new();
        for line in help.lines() {
            let trimmed = line.trim_start();
            if !trimmed.starts_with('-') {
                continue;
            }
            let spec = split_option_spec(trimmed);
            let takes_value = spec.contains(" <") && !spec.contains("[=<");
            let base = spec.split(" <").next().unwrap_or(spec);

            for raw_part in base.split(',') {
                let part = raw_part.trim();
                if let Some(rest) = part.strip_prefix("--[no-]") {
                    let name = rest.split('[').next().unwrap_or(rest);
                    if !name.is_empty() {
                        arity.insert(format!("--{name}"), takes_value);
                        arity.insert(format!("--no-{name}"), false);
                    }
                } else if part.starts_with('-') {
                    let name = part.split('[').next().unwrap_or(part);
                    arity.insert(name.to_owned(), takes_value && !name.starts_with("--no-"));
                }
            }
        }
        Self { arity }
    }

    fn parse_option(
        &self,
        argv: &[OsString],
        index: usize,
        output: &mut Vec<OsString>,
    ) -> Result<usize, DtrError> {
        let argument = argv[index]
            .to_str()
            .ok_or_else(|| DtrError::new("git option names must be valid UTF-8"))?;

        if let Some((key, _)) = argument.split_once('=')
            && argument.starts_with("--")
            && self.arity.contains_key(key)
        {
            output.push(argv[index].clone());
            return Ok(1);
        }

        if let Some(takes_value) = self.arity.get(argument) {
            output.push(argv[index].clone());
            if *takes_value {
                let value = argv.get(index + 1).ok_or_else(|| {
                    DtrError::new(format!("git clone option requires a value: {argument}"))
                })?;
                if value == OsStr::new("--") {
                    return Err(DtrError::new(format!(
                        "git clone option requires a value: {argument}"
                    )));
                }
                output.push(value.clone());
                return Ok(2);
            }
            return Ok(1);
        }

        if argument.starts_with('-') && !argument.starts_with("--") && argument.len() > 2 {
            let short = &argument[..2];
            if self.arity.get(short) == Some(&true) {
                output.push(argv[index].clone());
                return Ok(1);
            }
            if argument[1..]
                .chars()
                .all(|character| self.arity.get(&format!("-{character}")) == Some(&false))
            {
                output.push(argv[index].clone());
                return Ok(1);
            }
        }

        Err(DtrError::new(format!(
            "unknown option (not recognized by dtr or 'git clone -h'): {argument}"
        )))
    }
}

fn split_option_spec(line: &str) -> &str {
    let bytes = line.as_bytes();
    for index in 0..bytes.len().saturating_sub(1) {
        if bytes[index].is_ascii_whitespace() && bytes[index + 1].is_ascii_whitespace() {
            return line[..index].trim_end();
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELP_FIXTURE: &str = "\
usage: git clone [<options>] [--] <repo> [<dir>]\n\
    -v, --[no-]verbose    be more verbose\n\
    -q, --[no-]quiet      be more quiet\n\
    -n, --no-checkout     don't create a checkout\n\
    -j, --[no-]jobs <n>   number of jobs\n\
    --[no-]depth <depth>  create a shallow clone\n\
    --[no-]recursive[=<pathspec>]\n";

    fn table() -> GitOptionTable {
        GitOptionTable::from_help(HELP_FIXTURE)
    }

    #[test]
    fn infers_positive_negative_and_value_arities() {
        let table = table();
        assert_eq!(table.arity.get("-j"), Some(&true));
        assert_eq!(table.arity.get("--jobs"), Some(&true));
        assert_eq!(table.arity.get("--no-jobs"), Some(&false));
        assert_eq!(table.arity.get("--recursive"), Some(&false));
    }

    #[test]
    fn parses_long_value_and_short_cluster() {
        let table = table();
        let argv = ["--depth", "1", "-vq"].map(OsString::from).to_vec();
        let mut output = Vec::new();
        assert_eq!(table.parse_option(&argv, 0, &mut output).unwrap(), 2);
        assert_eq!(table.parse_option(&argv, 2, &mut output).unwrap(), 1);
        assert_eq!(output, argv);
    }

    #[test]
    fn parses_attached_short_value_and_long_equals() {
        let table = table();
        for value in ["-j4", "--depth=1"] {
            let argv = vec![OsString::from(value)];
            let mut output = Vec::new();
            assert_eq!(table.parse_option(&argv, 0, &mut output).unwrap(), 1);
            assert_eq!(output, argv);
        }
    }

    #[test]
    fn rejects_unknown_options_and_missing_values() {
        let table = table();
        for argv in [
            vec![OsString::from("--wat")],
            vec![OsString::from("--depth")],
        ] {
            assert!(table.parse_option(&argv, 0, &mut Vec::new()).is_err());
        }
    }
}
