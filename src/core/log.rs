//! Logging configuration for OxideGram.
//!
//! Configures rolling file output to `data/logs/` using `tracing-subscriber` and `tracing-appender`.
//! Console output to `stdout` is explicitly disabled as requested.

use crate::errors::{FileSystemError, LogError, OxideError};
use std::{env, fs};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt};

/// Initializes daily rolling file appender logging.
///
/// Returns a `WorkerGuard` which flushes logs on drop.
pub fn initialize_logger() -> Result<WorkerGuard, OxideError> {
    let working_path = env::current_dir().map_err(FileSystemError::CurrentDirectoryError)?;
    let logs_dir = working_path.join("data").join("logs");

    if !logs_dir.exists() {
        fs::create_dir_all(&logs_dir).map_err(FileSystemError::FolderCreationError)?;
    }

    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("oxidegram")
        .filename_suffix("log")
        .max_log_files(7)
        .build(logs_dir)
        .map_err(LogError::AppenderCreationError)?;

    let (non_blocking, guard) = NonBlockingBuilder::default().lossy(false).finish(appender);

    let filter = EnvFilter::try_from_env("OXIDEGRAM_LOG")
        .unwrap_or_else(|_| EnvFilter::new(format!("{}=info,warn", env!("CARGO_CRATE_NAME"))));

    fmt()
        .with_env_filter(filter)
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .with_thread_ids(true)
        .init();

    Ok(guard)
}
