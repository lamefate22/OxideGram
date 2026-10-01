//! Orchestration module managing multi-session and multi-bot cluster execution.

use crate::errors::{OxideError, format_error_chain};
use crate::infrastructure::lua::LuaBotRunner;
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

/// Specification for an initialized bot instance running in the cluster.
pub struct BotInstance {
    /// Phone number identifying the Telegram account session.
    pub session_phone: String,
    /// Human-readable bot name.
    pub bot_name: String,
    /// Initialized Lua runtime runner.
    pub runner: LuaBotRunner,
}

impl BotInstance {
    #[allow(dead_code)]
    pub fn new(
        session_phone: impl Into<String>,
        bot_name: impl Into<String>,
        runner: LuaBotRunner,
    ) -> Self {
        Self {
            session_phone: session_phone.into(),
            bot_name: bot_name.into(),
            runner,
        }
    }
}

impl From<(String, String, LuaBotRunner)> for BotInstance {
    fn from((session_phone, bot_name, runner): (String, String, LuaBotRunner)) -> Self {
        Self {
            session_phone,
            bot_name,
            runner,
        }
    }
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
    pub async fn run_instances<I: Into<BotInstance>>(runners: Vec<I>) -> Result<(), OxideError> {
        let instances: Vec<BotInstance> = runners.into_iter().map(Into::into).collect();
        let total = instances.len();
        info!(total, "Starting multi-session bot cluster instances");

        let mut tasks: Vec<ClusterTask> = Vec::new();

        for instance in instances {
            let phone = instance.session_phone;
            let bot_name = instance.bot_name;
            let mut runner = instance.runner;

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
