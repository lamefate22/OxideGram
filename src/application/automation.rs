//! Automation use-case ports.

/// User input required while configuring a script before its event loop starts.
pub trait BotConsole: Send + Sync {
    fn ask_input(&self, prompt: &str, default: Option<&str>) -> Result<String, String>;
}
