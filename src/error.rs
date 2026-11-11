use std::fmt;

#[derive(Debug)]
pub struct AppError {
    pub code: String,
    pub detail: String,
    pub extra: Vec<(String, String)>,
}

impl AppError {
    pub fn new(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            detail: detail.into(),
            extra: Vec::new(),
        }
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.push((key.into(), value.into()));
        self
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for AppError {}

#[derive(Debug)]
pub struct UsageError {
    pub detail: String,
    pub flag: Option<String>,
}

impl UsageError {
    pub fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
            flag: None,
        }
    }

    pub fn flag(mut self, flag: impl Into<String>) -> Self {
        self.flag = Some(flag.into());
        self
    }
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.detail)
    }
}

impl std::error::Error for UsageError {}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::new("sqlite", err.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::new("io", err.to_string())
    }
}
