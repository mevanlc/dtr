use std::fmt;

#[derive(Debug)]
pub struct DtrError(pub(crate) String);

impl DtrError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for DtrError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for DtrError {}

impl From<std::io::Error> for DtrError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}
