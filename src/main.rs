//! OxideGram: High-performance, secure, and extensible Telegram automation client in Rust.
//!
//! Entry point for the interactive terminal interface.

mod application;
mod domain;
mod errors;
mod infrastructure;
mod presentation;

use errors::{OxideError, format_error_chain};
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
    use infrastructure::session_repository::OxideConfig;
    use presentation::console::OxideConsole;

    let console = Arc::new(OxideConsole::new());
    console.print_header();
    let mut config = OxideConfig::default();
    config.load().await?;

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "check" {
        return run_check(console.as_ref(), &config).await;
    }
    if args.len() > 1 && (args[1] == "test-bot" || args[1] == "sim") {
        let script_arg = args.get(2).map(|s| s.as_str());
        return run_simulator(console.clone(), script_arg).await;
    }

    let menu_options = vec![
        "1. Run Single Bot".to_string(),
        "2. Run Multi-Session Cluster".to_string(),
        "3. Test Bot in Simulator (Offline Dry-Run)".to_string(),
        "4. Create Bot Script Template".to_string(),
        "5. Exit".to_string(),
    ];

    let choice = match console.ask_select("Choose action:", menu_options) {
        Ok(c) => c,
        Err(_) => {
            console.print("Operation canceled.");
            return Ok(());
        }
    };

    if choice.starts_with('1') {
        run_single_bot(console, &mut config).await?;
    } else if choice.starts_with('2') {
        run_multi_cluster(console, &mut config).await?;
    } else if choice.starts_with('3') {
        run_simulator(console, None).await?;
    } else if choice.starts_with('4') {
        create_template_flow(console.as_ref()).await?;
    } else {
        console.print("Goodbye!");
    }

    Ok(())
}

async fn run_single_bot(
    console: Arc<presentation::console::OxideConsole>,
    config: &mut infrastructure::session_repository::OxideConfig,
) -> Result<(), OxideError> {
    use application::authentication::LoginService;
    use infrastructure::lua::LuaBotRunner;
    use infrastructure::script_catalog::FileSystemScriptCatalog;
    use infrastructure::telegram_auth::GrammersAuthGateway;

    let telegram = GrammersAuthGateway;
    let mut login = LoginService::new(config, console.as_ref(), &telegram);
    let authenticated = login.login().await?;

    let loader = FileSystemScriptCatalog::default();
    let mut bots = loader.search_bots().await?;

    if bots.is_empty() {
        console.print("[empty] No scripts found in data/bots/.");
        let create_now = console
            .ask_confirm("Would you like to generate a bot template now?")
            .unwrap_or(false);
        if create_now {
            create_template_flow(console.as_ref()).await?;
            bots = loader.search_bots().await?;
        } else {
            return Ok(());
        }
    }

    if bots.is_empty() {
        console.print("No bot script selected. Aborting.");
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

    let Some(selected_bot) = bots.into_iter().find(|b| b.name == selected_name) else {
        return Ok(());
    };

    let infrastructure::telegram_auth::AuthenticatedClient {
        client,
        updates,
        phone,
        session_password,
    } = authenticated;

    let mut runner = LuaBotRunner::new(client, updates, console.clone())?;
    runner.load_script(&selected_bot.path).await?;
    console.print(&format!(
        "[running] {}  |  press Ctrl+C to stop (Hot-Reload enabled)",
        selected_bot.name
    ));
    let run_res = runner.run_event_loop().await;

    if let Some(password) = session_password
        && let Err(error) = telegram.persist_and_secure(&phone, &password, config).await
    {
        tracing::warn!(error = %error, "Failed to persist and secure encrypted session");
    }

    run_res?;
    Ok(())
}

async fn run_multi_cluster(
    console: Arc<presentation::console::OxideConsole>,
    config: &mut infrastructure::session_repository::OxideConfig,
) -> Result<(), OxideError> {
    use application::authentication::SessionRepository;
    use application::authentication::SessionRestorer;
    use application::orchestration::MultiBotCluster;
    use infrastructure::lua::LuaBotRunner;
    use infrastructure::script_catalog::FileSystemScriptCatalog;
    use infrastructure::telegram_auth::GrammersAuthGateway;

    let saved_phones = config.session_phones();
    if saved_phones.is_empty() {
        console.print("[warning] No saved sessions found in configuration.");
        console.print(
            "Please run option 1 ('Run Single Bot') first to authorize at least one session.",
        );
        return Ok(());
    }

    let loader = FileSystemScriptCatalog::default();
    let bots = loader.search_bots().await?;
    if bots.is_empty() {
        console.print("[empty] No scripts found in data/bots/. Create a script first.");
        return Ok(());
    }

    console.print(&format!(
        "[cluster setup] Found {} saved sessions and {} bot scripts.",
        saved_phones.len(),
        bots.len()
    ));

    let mut runners: Vec<(String, String, LuaBotRunner)> = Vec::new();
    let mut session_passwords: Vec<(String, String)> = Vec::new();
    let telegram = GrammersAuthGateway;

    for phone in &saved_phones {
        let run_for_this = console
            .ask_confirm(&format!("Include session {phone} in cluster?"))
            .unwrap_or(false);
        if !run_for_this {
            continue;
        }

        let bot_names: Vec<String> = bots.iter().map(|b| b.name.clone()).collect();
        let selected_bot_name =
            match console.ask_select(&format!("Select bot script for {phone}:"), bot_names) {
                Ok(name) => name,
                Err(_) => continue,
            };

        let Some(bot_script) = bots.iter().find(|b| b.name == selected_bot_name) else {
            continue;
        };

        let password =
            match console.ask_password(&format!("Enter decryption password for {phone}:")) {
                Ok(p) => p,
                Err(_) => continue,
            };

        let Some(saved_session) = config.session(phone) else {
            console.print(&format!("[error] Session not found for {phone}"));
            continue;
        };

        console.print(&format!("[connecting] Restoring session {phone}..."));
        let authenticated = match telegram.restore(phone, &password, saved_session).await {
            Ok(client) => client,
            Err(e) => {
                console.print(&format!("[error] Failed to restore session {phone}: {e}"));
                continue;
            }
        };

        let mut runner =
            LuaBotRunner::new(authenticated.client, authenticated.updates, console.clone())?;

        if let Err(e) = runner.load_script(&bot_script.path).await {
            console.print(&format!(
                "[error] Failed to load script '{}' for {phone}: {e}",
                bot_script.name
            ));
            continue;
        }

        session_passwords.push((phone.clone(), password));
        runners.push((phone.clone(), bot_script.name.clone(), runner));
    }

    if runners.is_empty() {
        console.print("[warning] No bot instances configured for the cluster.");
        return Ok(());
    }

    console.print(&format!(
        "\n[cluster active] Running {} bot instances across {} sessions | press Ctrl+C to stop",
        runners.len(),
        runners.len()
    ));

    let run_res = MultiBotCluster::run_instances(runners).await;

    // Securely update session state on disk
    for (phone, password) in &session_passwords {
        if let Err(e) = telegram.persist_and_secure(phone, password, config).await {
            tracing::warn!(phone = %phone, error = %e, "Failed to persist session after cluster shutdown");
        }
    }

    run_res?;
    Ok(())
}

async fn create_template_flow(
    console: &presentation::console::OxideConsole,
) -> Result<(), OxideError> {
    use infrastructure::script_catalog::FileSystemScriptCatalog;

    let bot_name = match console.ask_text("Enter new bot script name (e.g. echo_helper):") {
        Ok(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => {
            console.print("Canceled bot template creation.");
            return Ok(());
        }
    };

    let template_choices = vec![
        "1. echo - Basic start/ping commands & text echo".to_string(),
        "2. buttons - Interactive buttons parsing & click simulator".to_string(),
        "3. full - Full showcase (Regex captures, timers, rich text, flood protection)".to_string(),
        "4. flow - Step-by-step Dialog Flow (FSM), string methods & ox.storage".to_string(),
    ];

    let choice = match console.ask_select("Select template architecture:", template_choices) {
        Ok(c) => c,
        Err(_) => {
            console.print("Canceled bot template creation.");
            return Ok(());
        }
    };

    let template_kind = if choice.starts_with('2') {
        "buttons"
    } else if choice.starts_with('3') {
        "full"
    } else if choice.starts_with('4') {
        "flow"
    } else {
        "echo"
    };

    let loader = FileSystemScriptCatalog::default();
    match loader.create_bot_template(&bot_name, template_kind).await {
        Ok(path) => {
            console.print(&format!(
                "[success] Bot script template created: {}",
                path.display()
            ));
            console.print(
                "You can now customize it. Hot-Reload will automatically apply edits when running!",
            );
        }
        Err(e) => {
            console.print(&format!("[error] Failed to create bot template: {e}"));
        }
    }

    Ok(())
}

async fn run_check(
    console: &presentation::console::OxideConsole,
    config: &infrastructure::session_repository::OxideConfig,
) -> Result<(), OxideError> {
    use application::authentication::SessionRepository;
    use infrastructure::script_catalog::FileSystemScriptCatalog;

    console.print("[check] Starting OxideGram diagnostics...\n");

    // 1. Check sessions
    let sessions = config.session_phones();
    console.print(&format!(
        "[check] Config: data/config.oxide (OK, {} saved sessions)",
        sessions.len()
    ));

    // 2. Check scripts
    let loader = FileSystemScriptCatalog::default();
    let bots = loader.search_bots().await?;
    console.print(&format!(
        "[check] Bots directory: {} ({} scripts found)\n",
        loader.bots_dir.display(),
        bots.len()
    ));

    let lua = mlua::Lua::new();
    let mut syntax_errors = 0;

    for bot in &bots {
        match tokio::fs::read_to_string(&bot.path).await {
            Ok(content) => match lua.load(&content).into_function() {
                Ok(_) => {
                    console.print(&format!("  [OK]     {} (syntax valid)", bot.name));
                }
                Err(err) => {
                    syntax_errors += 1;
                    console.print(&format!("  [FAILED] {} (syntax error: {})", bot.name, err));
                }
            },
            Err(err) => {
                syntax_errors += 1;
                console.print(&format!("  [FAILED] {} (I/O error: {})", bot.name, err));
            }
        }
    }

    console.print("");
    if syntax_errors > 0 {
        console.print(&format!(
            "[check failed] Found {} error(s) across scripts.",
            syntax_errors
        ));
        Err(
            errors::ScriptError::Runtime(format!("{syntax_errors} scripts have syntax errors"))
                .into(),
        )
    } else {
        console.print("[check passed] All configuration and bot scripts are valid!");
        Ok(())
    }
}

async fn run_simulator(
    console: Arc<presentation::console::OxideConsole>,
    script_path_arg: Option<&str>,
) -> Result<(), OxideError> {
    use infrastructure::lua::BotSimulator;
    use infrastructure::script_catalog::FileSystemScriptCatalog;

    let target_path = if let Some(path_str) = script_path_arg {
        let p = std::path::PathBuf::from(path_str);
        if !p.exists() {
            console.print(&format!("[error] Script file not found: {}", p.display()));
            return Ok(());
        }
        p
    } else {
        let loader = FileSystemScriptCatalog::default();
        let bots = loader.search_bots().await?;
        if bots.is_empty() {
            console.print("[empty] No bot scripts found in data/bots/ to simulate.");
            return Ok(());
        }

        let names: Vec<String> = bots.iter().map(|b| b.name.clone()).collect();
        let selected_name = match console.ask_select("Select a bot script to simulate:", names) {
            Ok(n) => n,
            Err(_) => return Ok(()),
        };

        let Some(bot) = bots.into_iter().find(|b| b.name == selected_name) else {
            return Ok(());
        };
        bot.path
    };

    let mut sim = BotSimulator::new(&target_path, console.clone()).await?;
    sim.load_script().await?;
    sim.run_loop().await?;

    Ok(())
}
