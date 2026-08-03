//! OxideGram: High-performance, secure, and extensible Telegram automation client in Rust.
//!
//! Entry point for the interactive terminal interface.

mod application;
mod domain;
mod errors;
mod infrastructure;
mod presentation;

use errors::OxideError;
use std::error::Error;
use std::process::ExitCode;
use std::sync::Arc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> ExitCode {
    use infrastructure::logging::initialize_logger;

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
    use application::authentication::LoginService;
    use infrastructure::lua_runtime::LuaBotRunner;
    use infrastructure::script_catalog::FileSystemScriptCatalog;
    use infrastructure::session_repository::OxideConfig;
    use infrastructure::telegram_auth::GrammersAuthGateway;
    use presentation::console::OxideConsole;

    let console = Arc::new(OxideConsole::new());
    console.print_header();
    let mut config = OxideConfig::default();

    let telegram = GrammersAuthGateway;
    let mut login = LoginService::new(&mut config, console.as_ref(), &telegram);
    let authenticated = login.login().await?;

    let loader = FileSystemScriptCatalog::default();
    let bots = loader.search_bots().await?;

    if bots.is_empty() {
        console.print("[empty] No scripts found in data/bots/. Add a .lua file and retry.");
        return Ok(());
    }

    let bot_names: Vec<String> = bots.iter().map(|b| b.name.clone()).collect();
    let selected_name = match console.ask_select("Select a bot to run:", bot_names) {
        Ok(name) => name,
        Err(_) => {
            console.print("Session canceled.");
            return Ok(());
        }
    };

    if let Some(selected_bot) = bots.into_iter().find(|b| b.name == selected_name) {
        let mut runner =
            LuaBotRunner::new(authenticated.client, authenticated.updates, console.clone())?;
        runner.load_script(&selected_bot.path).await?;
        console.print(&format!(
            "[running] {}  |  press Ctrl+C to stop",
            selected_bot.name
        ));
        runner.run_event_loop().await?;
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
