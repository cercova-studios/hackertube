use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};

pub fn user_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn data_home() -> PathBuf {
    if let Some(value) = std::env::var_os("HACKERTUBE_HOME") {
        return PathBuf::from(value);
    }
    user_home().join(".hackertube")
}

pub fn display_path(path: &Path) -> String {
    let home = user_home();
    if let Ok(rest) = path.strip_prefix(&home) {
        if rest.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", rest.to_string_lossy().replace('\\', "/"));
    }
    path.to_string_lossy().to_string()
}

pub fn bin_path() -> String {
    std::env::current_exe()
        .map(|path| display_path(&path))
        .unwrap_or_else(|_| "hackertube".to_string())
}

pub fn today() -> chrono::NaiveDate {
    if let Ok(value) = std::env::var("HACKERTUBE_TODAY") {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d") {
            return date;
        }
    }
    Utc::now().date_naive()
}

pub fn now_iso() -> String {
    if std::env::var("HACKERTUBE_TODAY").is_ok() {
        return format!("{}T00:00:00Z", today().format("%Y-%m-%d"));
    }
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

pub fn parse_day(value: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").ok()
}

pub fn utc_now() -> DateTime<Utc> {
    Utc::now()
}
