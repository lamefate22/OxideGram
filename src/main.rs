//! OxideGram: High-performance, secure, and extensible Telegram automation client in Rust.
//!
//! Entry point for the interactive terminal interface.

mod core;
mod errors;
mod infrastructure;
mod scripting;
mod ui;

use errors::OxideError;
use std::error::Error;
use std::process::ExitCode;
use tracing::{error, info};

#[tokio::main]
async fn main() -> ExitCode {
    use core::log::initialize_logger;

    let _guard = match initialize_logger() {
        Ok(guard) => guard,
        Err(error) => {
            eprintln!(
                "Failed to initialize logging: {}",
                format_error_chain(&error)
            );
            return ExitCode::FAILURE;
        }
    };

    info!(version = env!("CARGO_PKG_VERSION"), "OxideGram started");
    match run().await {
        Ok(()) => {
            info!("OxideGram stopped normally");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let error_chain = format_error_chain(&error);
            error!(error = %error_chain, "OxideGram stopped with an error");
            eprintln!("Error: {error_chain}");
            eprintln!(
                "Details were written to data/logs/. Set OXIDEGRAM_LOG=debug for verbose logs."
            );
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), OxideError> {
    use core::config::OxideConfig;
    use infrastructure::auth::OxideAuth;
    use infrastructure::loader::BotLoader;
    use scripting::engine::LuaBotRunner;
    use ui::console::OxideConsole;

    let console = OxideConsole::new();
    let mut config = OxideConfig::default();

    let mut auth = OxideAuth::new(&mut config, &console);
    let authenticated = auth.login().await?;

    let loader = BotLoader::default();
    let bots = loader.search_bots().await?;

    if bots.is_empty() {
        console.print("No bot scripts found in 'data/bots/'. Place a .lua file there and retry.");
        return Ok(());
    }

    let bot_names: Vec<String> = bots.iter().map(|b| b.name.clone()).collect();
    let selected_name = match console.ask_select("Select a bot to run:", bot_names) {
        Ok(name) => name,
        Err(_) => {
            console.print("Goodbye!");
            return Ok(());
        }
    };

    if let Some(selected_bot) = bots.into_iter().find(|b| b.name == selected_name) {
        let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        let mut runner = LuaBotRunner::new(authenticated.client, authenticated.updates)?;
        runner.load_script(&selected_bot.path).await?;
        console.print(&format!("Bot '{}' is running...", selected_bot.name));
        runner.run_event_loop(cancel_rx).await?;
    }

    Ok(())
}

fn format_error_chain(error: &(dyn Error + 'static)) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(error) = source {
        let detail = error.to_string();
        if !message.contains(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        source = error.source();
    }
    message
}
