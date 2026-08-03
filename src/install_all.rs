use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use serde::Deserialize;

use crate::cli::{InstallAllArgs, InstallArgs, InstallTool, Jobs};
use crate::command::CommandPlan;
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

struct InstallJob {
    number: usize,
    repospec: String,
    plan: CommandPlan,
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

pub(crate) fn run(
    args: InstallAllArgs,
    explain: bool,
    narration_override: Option<bool>,
) -> Result<i32, DtrError> {
    let config = InstallAllConfig::load()?;
    let requested_jobs = args.jobs;
    let job_count = requested_jobs.resolve();
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
    let mut jobs = Vec::new();

    if explain {
        match requested_jobs {
            Jobs::Auto => println!("jobs: {job_count} (auto)"),
            Jobs::Count(_) => println!("jobs: {job_count}"),
        }
    }

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
            println!();
            println!("install-all entry {number}:");
            plan.explain();
            continue;
        }

        jobs.push(InstallJob {
            number,
            repospec,
            plan,
        });
    }

    if !explain && execute_jobs(jobs, job_count, narration) {
        failed = true;
    }

    Ok(i32::from(failed))
}

fn execute_jobs(jobs: Vec<InstallJob>, job_count: usize, narration: bool) -> bool {
    let worker_count = job_count.min(jobs.len());
    let queue = Mutex::new(VecDeque::from(jobs));
    let failed = AtomicBool::new(false);

    thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                loop {
                    let job = queue
                        .lock()
                        .expect("install-all work queue should not be poisoned")
                        .pop_front();
                    let Some(job) = job else {
                        break;
                    };
                    if !execute_job(&job, narration) {
                        failed.store(true, Ordering::Relaxed);
                    }
                }
            });
        }
    });

    failed.load(Ordering::Relaxed)
}

fn execute_job(job: &InstallJob, narration: bool) -> bool {
    match job.plan.execute(narration) {
        Ok(0) => true,
        Ok(code) => {
            eprintln!(
                "dtr: warning: install-all entry {} ({:?}) exited with status {code}",
                job.number, job.repospec
            );
            false
        }
        Err(error) => {
            warn_entry(job.number, &job.repospec, &error);
            false
        }
    }
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
