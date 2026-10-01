//! Command-line interface definitions and parser using `clap`.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// OxideGram: High-performance, secure Telegram automation client in Rust.
#[derive(Debug, Parser)]
#[command(
    name = "oxidegram",
    author,
    version,
    about = "High-performance, secure, and extensible Telegram automation client in Rust",
    propagate_version = true
)]
pub struct Cli {
    /// Enable verbose diagnostic logs in the terminal
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress all non-error output in the terminal
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Master decryption password (or set via OXIDEGRAM_PASSWORD environment variable)
    #[arg(long, global = true, env = "OXIDEGRAM_PASSWORD")]
    pub password: Option<String>,

    /// Subcommand to execute (if omitted, launches interactive TUI menu)
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Available CLI subcommands.
#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Run a specific bot script
    Run {
        /// Name or path of the bot script to execute (e.g. hello, data/bots/echo.lua)
        bot: String,

        /// Phone number of the Telegram session to bind
        #[arg(short, long)]
        session: Option<String>,
    },

    /// Test a bot script locally in offline Dry-Run simulator
    #[command(alias = "test-bot")]
    Sim {
        /// Path to the bot script to test
        bot: Option<PathBuf>,
    },

    /// Validate syntax of bot scripts and integrity of saved sessions
    Check {
        /// Optional specific bot script to check
        bot: Option<PathBuf>,
    },

    /// Launch a concurrent multi-session bot cluster
    Cluster {
        /// Comma-separated list of bot names or paths to run
        #[arg(short, long, value_delimiter = ',')]
        bots: Option<Vec<String>>,
    },

    /// Manage Telegram sessions and hardware-bound encryption vault
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },

    /// Generate ready-to-run bot script templates
    Template {
        #[command(subcommand)]
        action: TemplateAction,
    },
}

/// Actions available under `oxidegram session`.
#[derive(Debug, Subcommand)]
pub enum SessionAction {
    /// List all authorized sessions
    List,
    /// Authorize and add a new Telegram session
    Add,
    /// Remove an authorized session
    Remove {
        /// Phone number of the session to delete
        phone: String,
    },
    /// Clear the device-bound encryption vault (locks on next launch)
    Lock,
    /// Test unlocking the device-bound encryption vault
    Unlock,
}

/// Actions available under `oxidegram template`.
#[derive(Debug, Subcommand)]
pub enum TemplateAction {
    /// Create a new bot script template
    New {
        /// Name for the new script
        name: String,
        /// Template kind: echo, buttons, flow, full
        #[arg(short, long, default_value = "flow")]
        kind: String,
    },
}
