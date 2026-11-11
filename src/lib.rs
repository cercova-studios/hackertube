//! Agent-first video catalog. The binary is a thin wrapper around [`cli::run`].

pub mod areas;
pub mod cli;
pub mod commands;
pub mod db;
pub mod embed;
pub mod error;
pub mod ingest;
pub mod insights;
pub mod output;
pub mod paths;
pub mod state;
pub mod vecstore;

pub use error::{AppError, UsageError};
