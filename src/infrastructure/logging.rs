//! Dual-layer logging infrastructure for OxideGram.
//!
//! Configures two independent tracing layers:
//! 1. File layer: Detailed rolling file output to `data/logs/oxidegram.YYYY-MM-DD.log`
//! 2. Console layer: Clean, compact, colorized output to `stdout` with MTProto noise suppression

use crate::errors::{FileSystemError, LogError, OxideError};
use std::{env, fs};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Controls terminal console log verbosity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogVerbosity {
    #[default]
    Normal,
    Verbose,
    Quiet,
}

/// Initializes dual-layer logging: rolling file appender on disk and compact filtered console output.
///
/// Returns a `WorkerGuard` which flushes logs on drop.
pub fn initialize_logger(verbosity: LogVerbosity) -> Result<WorkerGuard, OxideError> {
    let working_path = env::current_dir().map_err(FileSystemError::CurrentDirectoryError)?;
    let logs_dir = working_path.join("data").join("logs");

    if !logs_dir.exists() {
        fs::create_dir_all(&logs_dir).map_err(FileSystemError::FolderCreationError)?;
    }

    // 1. File layer: Detailed technical diagnostics
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("oxidegram")
        .filename_suffix("log")
        .max_log_files(7)
        .build(logs_dir)
        .map_err(LogError::AppenderCreationError)?;

    let (non_blocking, guard) = NonBlockingBuilder::default().lossy(false).finish(appender);

    let file_filter = EnvFilter::try_from_env("OXIDEGRAM_FILE_LOG")
        .unwrap_or_else(|_| EnvFilter::new("oxidegram=debug,lua=debug,grammers=info,warn,error"));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .with_thread_ids(true)
        .with_filter(file_filter);

    // 2. Console layer: Filtered, compact, and formatted for humans
    let console_directive = match verbosity {
        LogVerbosity::Verbose => "oxidegram=debug,lua=debug,grammers=info,warn,error",
        LogVerbosity::Quiet => "oxidegram=warn,lua=warn,grammers=error,error",
        LogVerbosity::Normal => {
            // Mute MTProto/grammers internal pings and transport noise
            "oxidegram=info,lua=info,grammers_mtsender=warn,grammers_mtproto=warn,grammers_client=warn,warn,error"
        }
    };

    let console_filter = EnvFilter::try_from_env("OXIDEGRAM_LOG")
        .unwrap_or_else(|_| EnvFilter::new(console_directive));

    let console_layer = fmt::layer()
        .compact()
        .without_time()
        .with_target(false)
        .with_file(false)
        .with_line_number(false)
        .with_filter(console_filter);

    tracing_subscriber::registry()
        .with(file_layer)
        .with(console_layer)
        .init();

    Ok(guard)
}
