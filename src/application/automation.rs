//! Automation use-case ports.

/// User input required while configuring a script before its event loop starts.
pub trait BotConsole: Send + Sync {
    /// Prompts the user to enter arbitrary text.
    fn ask_input(&self, prompt: &str, default: Option<&str>) -> Result<String, String>;

    /// Prompts the user to pick an option from a predefined list of choices.
    fn ask_select(
        &self,
        prompt: &str,
        choices: Vec<String>,
        default: Option<&str>,
    ) -> Result<String, String> {
        let _ = default;
        if choices.is_empty() {
            return Err("No choices provided".into());
        }
        let formatted = format!("{prompt} [{}]", choices.join("/"));
        self.ask_input(&formatted, default)
    }

    /// Prompts the user for a boolean confirmation (yes/no).
    fn ask_confirm(&self, prompt: &str, default: Option<bool>) -> Result<bool, String> {
        let def_str = default.map(|d| if d { "y" } else { "n" });
        let resp = self.ask_input(&format!("{prompt} (y/n)"), def_str)?;
        let trimmed = resp.trim().to_lowercase();
        Ok(trimmed == "y"
            || trimmed == "yes"
            || trimmed == "true"
            || trimmed == "1"
            || trimmed == "да")
    }

    /// Prompts the user to enter or select a file path, with autocomplete and validation.
    fn ask_file(
        &self,
        prompt: &str,
        default: Option<&str>,
        must_exist: bool,
        allowed_extensions: Option<Vec<String>>,
    ) -> Result<String, String> {
        let _ = (must_exist, allowed_extensions);
        self.ask_input(prompt, default)
    }
}
