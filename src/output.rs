use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::process::{Command, Stdio};

use crate::config::Config;
use crate::error::DtrError;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Verbosity(i8);

impl Verbosity {
    pub(crate) fn new(base: i8, adjustment: i32) -> Self {
        Self((i32::from(base) + adjustment).clamp(-2, 2) as i8)
    }

    pub(crate) fn load(adjustment: i32) -> Result<Self, DtrError> {
        Ok(Self::new(
            Config::load_for_runtime()?.verbosity(),
            adjustment,
        ))
    }

    pub(crate) fn results(self) -> bool {
        self.0 >= -1
    }
    pub(crate) fn progress(self) -> bool {
        self.0 >= 0
    }
    pub(crate) fn verbose(self) -> bool {
        self.0 >= 1
    }
    pub(crate) fn debug(self, message: std::fmt::Arguments<'_>) {
        if self.0 >= 2 {
            eprintln!("dtr: {message}");
        }
    }
}

/// Spool quiet child output to a temporary file, avoiding pipe deadlocks and
/// unbounded memory use. Replay failures to stderr so stdout stays usable data.
pub(crate) struct CapturedOutput(File);

impl CapturedOutput {
    pub(crate) fn new() -> io::Result<Self> {
        tempfile::tempfile().map(Self)
    }

    pub(crate) fn redirect(&self, command: &mut Command) -> io::Result<()> {
        command.stdout(self.stdio()?).stderr(self.stdio()?);
        Ok(())
    }

    pub(crate) fn stdio(&self) -> io::Result<Stdio> {
        Ok(Stdio::from(self.0.try_clone()?))
    }

    pub(crate) fn append(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.0.write_all(bytes)
    }

    pub(crate) fn replay(&mut self) -> io::Result<()> {
        self.0.seek(SeekFrom::Start(0))?;
        io::copy(&mut self.0, &mut io::stderr().lock())?;
        Ok(())
    }
}
