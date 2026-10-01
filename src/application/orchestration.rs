//! Orchestration module managing multi-session and multi-bot cluster execution.

use crate::errors::OxideError;
use crate::infrastructure::lua_runtime::LuaBotRunner;
use std::path::PathBuf;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

/// Configuration specification for a bot instance running in the cluster.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BotInstanceConfig {
    /// Phone number identifying the Telegram account session.
    pub session_phone: String,
    /// Human-readable bot name.
    pub bot_name: String,
    /// Path to the `.lua` bot script.
    pub script_path: PathBuf,
}

/// Cluster coordinator managing concurrent execution of multiple bot scripts across multiple sessions.
pub struct MultiBotCluster;

/// Internal worker task representation for a cluster bot instance.
struct ClusterTask {
    phone: String,
    bot_name: String,
    handle: JoinHandle<Result<(), String>>,
}

impl MultiBotCluster {
    /// Spawns and manages a collection of initialized `LuaBotRunner` instances concurrently.
    ///
    /// Each bot runs in an isolated `tokio::spawn` task.
    /// If one bot fails or encounters an error, other instances continue operating.
    /// Pressing Ctrl+C initiates a graceful shutdown across all active sessions.
    pub async fn run_instances(
        runners: Vec<(String, String, LuaBotRunner)>,
    ) -> Result<(), OxideError> {
        let total = runners.len();
        info!(total, "Starting multi-session bot cluster instances");

        let mut tasks: Vec<ClusterTask> = Vec::new();

        for (phone, bot_name, mut runner) in runners {
            let handle = tokio::spawn(async move {
                runner
                    .run_event_loop()
                    .await
                    .map_err(|e| format_error_chain(&e))
            });
            tasks.push(ClusterTask {
                phone,
                bot_name,
                handle,
            });
        }

        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        tokio::select! {
            signal = &mut ctrl_c => {
                match signal {
                    Ok(()) => info!("Multi-session cluster received Ctrl+C; shutting down all bots..."),
                    Err(e) => warn!(error = %e, "Failed to capture Ctrl+C signal"),
                }
            }
        }

        for task in tasks {
            match task.handle.await {
                Ok(Ok(())) => {
                    info!(phone = %task.phone, bot = %task.bot_name, "Bot instance terminated gracefully");
                }
                Ok(Err(err)) => {
                    error!(phone = %task.phone, bot = %task.bot_name, error = %err, "Bot instance stopped with error");
                }
                Err(join_err) => {
                    error!(phone = %task.phone, bot = %task.bot_name, error = %join_err, "Bot instance task panicked or aborted");
                }
            }
        }

        info!("All multi-session bot cluster instances stopped");
        Ok(())
    }
}

fn format_error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(err) = source {
        let detail = err.to_string();
        if !message.contains(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        source = err.source();
    }
    message
}
