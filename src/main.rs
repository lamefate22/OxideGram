//! OxideGram: High-performance, secure, and extensible Telegram automation client in Rust.
//!
//! Entry point for CLI commands and interactive terminal interface.

mod application;
mod domain;
mod errors;
mod infrastructure;
mod presentation;

use application::authentication::{SessionRepository, SessionRestorer};
use application::master_key::MasterKeyProvider;
use clap::Parser;
use errors::{OxideError, format_error_chain};
use infrastructure::crypto::HardwareMasterKeyProvider;
use infrastructure::logging::{LogVerbosity, initialize_logger};
use infrastructure::session_repository::OxideConfig;
use presentation::cli::{Cli, Commands, SessionAction, TemplateAction};
use presentation::console::OxideConsole;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use tracing::{error, info};

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(c) => c,
        Err(e) => {
            e.exit();
        }
    };

    let verbosity = if cli.verbose {
        LogVerbosity::Verbose
    } else if cli.quiet {
        LogVerbosity::Quiet
    } else {
        LogVerbosity::Normal
    };

    let _guard = match initialize_logger(verbosity) {
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
    match run(cli).await {
        Ok(()) => {
            info!("OxideGram stopped normally");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let error_chain = format_error_chain(&error);
            error!(error = %error_chain, "OxideGram stopped with an error");
            eprintln!("Error: {error_chain}");
            eprintln!(
                "Details were written to data/logs/. Set --verbose or OXIDEGRAM_LOG=debug for verbose logs."
            );
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), OxideError> {
    let console = Arc::new(OxideConsole::new());
    let mut config = OxideConfig::default();
    config.load().await?;

    let key_provider = Arc::new(HardwareMasterKeyProvider::new(
        console.clone(),
        cli.password.clone(),
    ));

    match cli.command {
        Some(Commands::Run { bot, session }) => {
            run_direct_bot(
                &console,
                &mut config,
                key_provider.as_ref(),
                &bot,
                session.as_deref(),
            )
            .await
        }
        Some(Commands::Sim { bot }) => {
            let script_path = bot.as_deref().and_then(|p| p.to_str());
            run_simulator(console, script_path).await
        }
        Some(Commands::Check { bot }) => run_check(console.as_ref(), &config, bot.as_deref()).await,
        Some(Commands::Cluster { bots }) => {
            run_multi_cluster(console, &mut config, key_provider.as_ref(), bots).await
        }
        Some(Commands::Session { action }) => {
            run_session_command(console.as_ref(), &mut config, key_provider.as_ref(), action).await
        }
        Some(Commands::Template { action }) => run_template_command(console.as_ref(), action).await,
        None => run_interactive_menu(console, &mut config, key_provider.as_ref()).await,
    }
}

async fn run_interactive_menu(
    console: Arc<OxideConsole>,
    config: &mut OxideConfig,
    key_provider: &HardwareMasterKeyProvider,
) -> Result<(), OxideError> {
    console.print_header();

    let menu_options = vec![
        "1. Run Single Bot".to_string(),
        "2. Run Multi-Session Cluster".to_string(),
        "3. Test Bot in Simulator (Offline Dry-Run)".to_string(),
        "4. Create Bot Script Template".to_string(),
        "5. Manage Sessions & Device Vault".to_string(),
        "6. Exit".to_string(),
    ];

    let choice = match console.ask_select("Choose action:", menu_options) {
        Ok(c) => c,
        Err(_) => {
            console.print("Operation canceled.");
            return Ok(());
        }
    };

    if choice.starts_with('1') {
        run_single_bot(console, config, key_provider).await?;
    } else if choice.starts_with('2') {
        run_multi_cluster(console, config, key_provider, None).await?;
    } else if choice.starts_with('3') {
        run_simulator(console, None).await?;
    } else if choice.starts_with('4') {
        create_template_flow(console.as_ref()).await?;
    } else if choice.starts_with('5') {
        run_session_menu(console.as_ref(), config, key_provider).await?;
    } else {
        console.print("Goodbye!");
    }

    Ok(())
}

async fn run_single_bot(
    console: Arc<OxideConsole>,
    config: &mut OxideConfig,
    key_provider: &HardwareMasterKeyProvider,
) -> Result<(), OxideError> {
    use application::authentication::LoginService;
    use infrastructure::lua::LuaBotRunner;
    use infrastructure::script_catalog::FileSystemScriptCatalog;
    use infrastructure::telegram_auth::GrammersAuthGateway;

    let telegram = GrammersAuthGateway;
    let saved_phones = config.session_phones();

    let authenticated = if saved_phones.len() == 1 {
        // Smart auto-skip: Exactly one session exists, skip prompt questions entirely!
        let phone = saved_phones[0].clone();
        let session = config
            .session(&phone)
            .ok_or_else(|| errors::AuthError::SessionNotFound(phone.clone()))?;
        let password = key_provider.resolve_master_key().await?;
        console.print(&format!("[auto-login] Restoring single session {phone}..."));
        telegram.restore(&phone, &password, session).await?
    } else {
        let mut login =
            LoginService::new(config, console.as_ref(), &telegram).with_key_provider(key_provider);
        login.login().await?
    };

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

async fn run_direct_bot(
    console: &Arc<OxideConsole>,
    config: &mut OxideConfig,
    key_provider: &HardwareMasterKeyProvider,
    bot_query: &str,
    session_arg: Option<&str>,
) -> Result<(), OxideError> {
    use infrastructure::lua::LuaBotRunner;
    use infrastructure::script_catalog::FileSystemScriptCatalog;
    use infrastructure::telegram_auth::GrammersAuthGateway;

    let telegram = GrammersAuthGateway;

    // 1. Locate the bot script
    let bot_path = if Path::new(bot_query).exists() {
        PathBuf::from(bot_query)
    } else {
        let loader = FileSystemScriptCatalog::default();
        let bots = loader.search_bots().await?;
        let matching = bots
            .into_iter()
            .find(|b| b.name.eq_ignore_ascii_case(bot_query));
        match matching {
            Some(b) => b.path,
            None => {
                console.print(&format!(
                    "[error] Bot '{bot_query}' not found in data/bots/"
                ));
                return Err(
                    errors::ScriptError::Runtime(format!("Bot '{bot_query}' not found")).into(),
                );
            }
        }
    };

    // 2. Resolve the Telegram session
    let saved_phones = config.session_phones();
    if saved_phones.is_empty() {
        console
            .print("[error] No saved sessions found in configuration. Authorize an account first.");
        return Err(errors::AuthError::SessionNotFound("None".into()).into());
    }

    let phone = if let Some(p) = session_arg {
        p.to_string()
    } else if saved_phones.len() == 1 {
        saved_phones[0].clone()
    } else {
        match console.ask_autocomplete("Select session to bind:", saved_phones) {
            Ok(p) => p,
            Err(_) => return Ok(()),
        }
    };

    let session = config
        .session(&phone)
        .ok_or_else(|| errors::AuthError::SessionNotFound(phone.clone()))?;

    // 3. Unlock session credentials
    let password = key_provider.resolve_master_key().await?;
    console.print(&format!("[connecting] Restoring session {phone}..."));
    let authenticated = telegram.restore(&phone, &password, session).await?;

    let mut runner =
        LuaBotRunner::new(authenticated.client, authenticated.updates, console.clone())?;
    runner.load_script(&bot_path).await?;
    console.print(&format!(
        "[running] {} on {phone} | press Ctrl+C to stop (Hot-Reload enabled)",
        bot_path.display()
    ));

    let run_res = runner.run_event_loop().await;

    if let Some(ref password) = authenticated.session_password
        && let Err(error) = telegram.persist_and_secure(&phone, password, config).await
    {
        tracing::warn!(error = %error, "Failed to persist and secure encrypted session");
    }

    run_res?;
    Ok(())
}

async fn run_multi_cluster(
    console: Arc<OxideConsole>,
    config: &mut OxideConfig,
    key_provider: &HardwareMasterKeyProvider,
    bots_filter: Option<Vec<String>>,
) -> Result<(), OxideError> {
    use application::authentication::SessionRestorer;
    use application::orchestration::MultiBotCluster;
    use infrastructure::lua::LuaBotRunner;
    use infrastructure::script_catalog::FileSystemScriptCatalog;
    use infrastructure::telegram_auth::GrammersAuthGateway;

    let saved_phones = config.session_phones();
    if saved_phones.is_empty() {
        console.print("[warning] No saved sessions found in configuration.");
        return Ok(());
    }

    let loader = FileSystemScriptCatalog::default();
    let bots = loader.search_bots().await?;
    if bots.is_empty() {
        console.print("[empty] No scripts found in data/bots/. Create a script first.");
        return Ok(());
    }

    // Resolve master key ONCE for the entire cluster run
    let master_password = key_provider.resolve_master_key().await?;

    let mut runners: Vec<(String, String, LuaBotRunner)> = Vec::new();
    let mut session_passwords: Vec<(String, String)> = Vec::new();
    let telegram = GrammersAuthGateway;

    for (idx, phone) in saved_phones.iter().enumerate() {
        let bot_script = if let Some(ref filter_list) = bots_filter {
            if let Some(bot_name) = filter_list.get(idx) {
                bots.iter().find(|b| b.name.eq_ignore_ascii_case(bot_name))
            } else {
                bots.first()
            }
        } else {
            let bot_names: Vec<String> = bots.iter().map(|b| b.name.clone()).collect();
            let selected =
                match console.ask_select(&format!("Select bot script for {phone}:"), bot_names) {
                    Ok(name) => name,
                    Err(_) => continue,
                };
            bots.iter().find(|b| b.name == selected)
        };

        let Some(bot_script) = bot_script else {
            continue;
        };

        let Some(saved_session) = config.session(phone) else {
            continue;
        };

        console.print(&format!("[connecting] Restoring session {phone}..."));
        let authenticated = match telegram
            .restore(phone, &master_password, saved_session)
            .await
        {
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

        session_passwords.push((phone.clone(), master_password.clone()));
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

    // Persist and secure sessions
    for (phone, password) in &session_passwords {
        if let Err(e) = telegram.persist_and_secure(phone, password, config).await {
            tracing::warn!(phone = %phone, error = %e, "Failed to persist session after cluster shutdown");
        }
    }

    run_res?;
    Ok(())
}

async fn run_session_command(
    console: &OxideConsole,
    config: &mut OxideConfig,
    key_provider: &HardwareMasterKeyProvider,
    action: SessionAction,
) -> Result<(), OxideError> {
    match action {
        SessionAction::List => {
            let phones = config.session_phones();
            if phones.is_empty() {
                console.print("[empty] No authorized sessions found in data/config.oxide");
            } else {
                console.print(&format!("Saved Authorized Sessions ({}) :", phones.len()));
                for (i, p) in phones.iter().enumerate() {
                    console.print(&format!("  {}. {p}", i + 1));
                }
            }
        }
        SessionAction::Add => {
            use application::authentication::LoginService;
            use infrastructure::telegram_auth::GrammersAuthGateway;

            let telegram = GrammersAuthGateway;
            let mut login =
                LoginService::new(config, console, &telegram).with_key_provider(key_provider);
            login.login().await?;
            console.print("[success] New Telegram session successfully authorized and saved!");
        }
        SessionAction::Remove { phone } => {
            if config.remove_session(&phone).await? {
                console.print(&format!("[success] Session {phone} successfully removed."));
            } else {
                console.print(&format!("[warning] Session {phone} not found."));
            }
        }
        SessionAction::Lock => {
            key_provider.clear_device_key().await?;
            console.print("[locked] Device-bound vault cleared. Master password will be requested on next launch.");
        }
        SessionAction::Unlock => {
            if key_provider.has_device_key() {
                match key_provider.resolve_master_key().await {
                    Ok(_) => console.print(
                        "[unlocked] Device vault unlocked successfully via hardware fingerprint!",
                    ),
                    Err(e) => console.print(&format!("[error] Failed to unlock device vault: {e}")),
                }
            } else {
                console.print("[info] No device vault is currently enrolled. Launch OxideGram to remember your password.");
            }
        }
    }
    Ok(())
}

async fn run_session_menu(
    console: &OxideConsole,
    config: &mut OxideConfig,
    key_provider: &HardwareMasterKeyProvider,
) -> Result<(), OxideError> {
    let options = vec![
        "1. List Sessions".to_string(),
        "2. Add New Session".to_string(),
        "3. Remove Session".to_string(),
        "4. Lock Device Vault (Clear Saved Password)".to_string(),
        "5. Test Device Vault Unlock".to_string(),
        "6. Back to Main Menu".to_string(),
    ];

    let choice = match console.ask_select("Session Management:", options) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };

    if choice.starts_with('1') {
        run_session_command(console, config, key_provider, SessionAction::List).await?;
    } else if choice.starts_with('2') {
        run_session_command(console, config, key_provider, SessionAction::Add).await?;
    } else if choice.starts_with('3') {
        let phones = config.session_phones();
        if phones.is_empty() {
            console.print("[empty] No sessions to remove.");
            return Ok(());
        }
        let selected = match console.ask_select("Select session to remove:", phones) {
            Ok(p) => p,
            Err(_) => return Ok(()),
        };
        let confirm = console
            .ask_confirm(&format!(
                "Are you sure you want to delete session {selected}?"
            ))
            .unwrap_or(false);
        if confirm {
            run_session_command(
                console,
                config,
                key_provider,
                SessionAction::Remove { phone: selected },
            )
            .await?;
        }
    } else if choice.starts_with('4') {
        run_session_command(console, config, key_provider, SessionAction::Lock).await?;
    } else if choice.starts_with('5') {
        run_session_command(console, config, key_provider, SessionAction::Unlock).await?;
    }

    Ok(())
}

async fn run_template_command(
    console: &OxideConsole,
    action: TemplateAction,
) -> Result<(), OxideError> {
    use infrastructure::script_catalog::FileSystemScriptCatalog;

    match action {
        TemplateAction::New { name, kind } => {
            let loader = FileSystemScriptCatalog::default();
            let path = loader.create_bot_template(&name, &kind).await?;
            console.print(&format!(
                "[success] Created bot script template at {}",
                path.display()
            ));
        }
    }
    Ok(())
}

async fn create_template_flow(console: &OxideConsole) -> Result<(), OxideError> {
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
    console: &OxideConsole,
    config: &OxideConfig,
    specific_bot: Option<&Path>,
) -> Result<(), OxideError> {
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
    let bots = if let Some(target) = specific_bot {
        vec![crate::domain::automation::BotScript {
            name: target
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "custom".to_string()),
            path: target.to_path_buf(),
        }]
    } else {
        loader.search_bots().await?
    };

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
    console: Arc<OxideConsole>,
    script_path_arg: Option<&str>,
) -> Result<(), OxideError> {
    use infrastructure::lua::BotSimulator;
    use infrastructure::script_catalog::FileSystemScriptCatalog;

    let target_path = if let Some(path_str) = script_path_arg {
        let p = PathBuf::from(path_str);
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
