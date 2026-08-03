//! Interactive terminal console UI implementation using `inquire`.
//!
//! Provides styled input prompts for text, passwords, numbers, selection lists, and confirmations.

use inquire::{Autocomplete, Confirm, CustomUserError, Password, Select, Text};

/// Custom autocomplete provider filtering choices based on user input.
#[derive(Clone, Default)]
pub struct ChoiceAutocomplete {
    pub choices: Vec<String>,
}

impl Autocomplete for ChoiceAutocomplete {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, CustomUserError> {
        let input_lower = input.to_lowercase();
        let matches = self
            .choices
            .iter()
            .filter(|c| c.to_lowercase().contains(&input_lower))
            .cloned()
            .collect();
        Ok(matches)
    }

    fn get_completion(
        &mut self,
        _input: &str,
        highlighted_suggestion: Option<String>,
    ) -> Result<Option<String>, CustomUserError> {
        Ok(highlighted_suggestion)
    }
}

/// Terminal console manager wrapping `inquire` interactive prompt methods.
pub struct OxideConsole;

impl OxideConsole {
    /// Creates a new `OxideConsole` instance.
    pub fn new() -> Self {
        Self
    }

    /// Prompts the user to enter text.
    pub fn ask_text(&self, hint: &str) -> Result<String, String> {
        Text::new(hint)
            .prompt()
            .map_err(|e| format!("Failed to get text input: {e}"))
    }

    /// Prompts the user to enter a password without masking echo.
    pub fn ask_password(&self, hint: &str) -> Result<String, String> {
        Password::new(hint)
            .without_confirmation()
            .prompt()
            .map_err(|e| format!("Failed to get password input: {e}"))
    }

    /// Prompts the user to enter an integer.
    pub fn ask_integer(&self, hint: &str) -> Result<i32, String> {
        let input = Text::new(hint)
            .prompt()
            .map_err(|e| format!("Failed to get integer input: {e}"))?;

        input
            .trim()
            .parse::<i32>()
            .map_err(|_| "The answer can only be an integer.".to_string())
    }

    /// Prompts the user for a boolean confirmation (yes/no).
    pub fn ask_confirm(&self, hint: &str) -> Result<bool, String> {
        Confirm::new(hint)
            .prompt()
            .map_err(|e| format!("Failed to get confirmation: {e}"))
    }

    /// Prompts the user to pick an option from a selection list.
    pub fn ask_select(&self, hint: &str, choices: Vec<String>) -> Result<String, String> {
        if choices.is_empty() {
            return Err("No choices available.".to_string());
        }

        Select::new(hint, choices)
            .prompt()
            .map_err(|e| format!("Failed to make selection: {e}"))
    }

    /// Prompts the user to select a choice with interactive autocomplete.
    pub fn ask_autocomplete(&self, hint: &str, choices: Vec<String>) -> Result<String, String> {
        if choices.is_empty() {
            return Err("No choices available for autocomplete.".to_string());
        }

        let autocomplete = ChoiceAutocomplete { choices };
        Text::new(hint)
            .with_autocomplete(autocomplete)
            .prompt()
            .map_err(|e| format!("Failed to get autocomplete input: {e}"))
    }

    /// Prints output text to standard output.
    pub fn print(&self, text: &str) {
        println!("{text}");
    }
}
