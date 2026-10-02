//! Interactive terminal console UI implementation using `inquire`.
//!
//! Provides styled input prompts for text, passwords, numbers, selection lists, and confirmations.

use crate::application::authentication::LoginConsole;
use crate::application::automation::BotConsole;
use inquire::{
    Autocomplete, Confirm, CustomType, CustomUserError, Password, Select, Text,
    ui::{Attributes, Color, ErrorMessageRenderConfig, RenderConfig, StyleSheet, Styled},
};

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

/// Custom autocomplete provider for local filesystem paths.
#[derive(Clone, Default)]
pub struct FilePathAutocomplete {
    pub allowed_extensions: Option<Vec<String>>,
}

impl Autocomplete for FilePathAutocomplete {
    fn get_suggestions(&mut self, input: &str) -> Result<Vec<String>, CustomUserError> {
        let normalized = input.replace('\\', "/");
        let path = std::path::Path::new(&normalized);

        let (dir_to_scan, search_prefix) = if normalized.is_empty() {
            (std::path::PathBuf::from("."), String::new())
        } else if normalized.ends_with('/') {
            (path.to_path_buf(), String::new())
        } else if let Some(parent) = path.parent() {
            let prefix = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            let dir = if parent.as_os_str().is_empty() {
                std::path::PathBuf::from(".")
            } else {
                parent.to_path_buf()
            };
            (dir, prefix)
        } else {
            (std::path::PathBuf::from("."), normalized.to_lowercase())
        };

        let mut suggestions = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir_to_scan) {
            let mut items: Vec<_> = entries.filter_map(|e| e.ok()).collect();
            items.sort_by_key(|e| {
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                (!is_dir, e.file_name())
            });

            for entry in items {
                let name = entry.file_name().to_string_lossy().to_string();
                let name_lower = name.to_lowercase();
                if !search_prefix.is_empty() && !name_lower.starts_with(&search_prefix) {
                    continue;
                }

                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let entry_path = if dir_to_scan == std::path::Path::new(".") {
                    name.clone()
                } else {
                    let base_dir = dir_to_scan.to_string_lossy().replace('\\', "/");
                    let base_clean = base_dir.trim_end_matches('/');
                    format!("{base_clean}/{name}")
                };

                if is_dir {
                    suggestions.push(format!("{entry_path}/"));
                } else {
                    let matches_ext = match &self.allowed_extensions {
                        Some(exts) => {
                            if let Some(ext) = std::path::Path::new(&name)
                                .extension()
                                .and_then(|e| e.to_str())
                            {
                                exts.iter().any(|allowed| {
                                    allowed.eq_ignore_ascii_case(ext.trim_start_matches('.'))
                                })
                            } else {
                                false
                            }
                        }
                        None => true,
                    };

                    if matches_ext {
                        suggestions.push(entry_path);
                    }
                }

                if suggestions.len() >= 30 {
                    break;
                }
            }
        }

        Ok(suggestions)
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

    /// Prints the compact application header before the interactive flow.
    pub fn print_header(&self) {
        println!(
            "\n  OXIDEGRAM  v{}\n  Telegram automation / Lua 5.4\n",
            env!("CARGO_PKG_VERSION")
        );
    }

    /// Prompts the user to enter text.
    pub fn ask_text(&self, hint: &str) -> Result<String, String> {
        Text::new(hint)
            .with_render_config(render_config())
            .prompt()
            .map_err(|e| format!("Failed to get text input: {e}"))
    }

    /// Prompts the user to enter a masked password.
    pub fn ask_password(&self, hint: &str) -> Result<String, String> {
        Password::new(hint)
            .without_confirmation()
            .with_render_config(render_config())
            .prompt()
            .map_err(|e| format!("Failed to get password input: {e}"))
    }

    /// Prompts the user to enter an integer.
    pub fn ask_integer(&self, hint: &str) -> Result<i32, String> {
        CustomType::<i32>::new(hint)
            .with_error_message("Enter a valid integer.")
            .with_render_config(render_config())
            .prompt()
            .map_err(|e| format!("Failed to get integer input: {e}"))
    }

    /// Prompts the user for a boolean confirmation (yes/no).
    pub fn ask_confirm(&self, hint: &str) -> Result<bool, String> {
        Confirm::new(hint)
            .with_render_config(render_config())
            .prompt()
            .map_err(|e| format!("Failed to get confirmation: {e}"))
    }

    /// Prompts the user to pick an option from a selection list.
    pub fn ask_select(&self, hint: &str, choices: Vec<String>) -> Result<String, String> {
        if choices.is_empty() {
            return Err("No choices available.".to_string());
        }

        Select::new(hint, choices)
            .with_render_config(render_config())
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
            .with_render_config(render_config())
            .prompt()
            .map_err(|e| format!("Failed to get autocomplete input: {e}"))
    }

    /// Prints output text to standard output.
    pub fn print(&self, text: &str) {
        println!("{text}");
    }
}

fn render_config() -> RenderConfig<'static> {
    if std::env::var_os("NO_COLOR").is_some() {
        return RenderConfig::empty();
    }

    let accent = Color::LightCyan;
    let strong = StyleSheet::new()
        .with_fg(Color::White)
        .with_attr(Attributes::BOLD);

    RenderConfig::empty()
        .with_prompt_prefix(Styled::new("::").with_fg(accent))
        .with_answered_prompt_prefix(Styled::new("ok").with_fg(Color::LightGreen))
        .with_text_input(StyleSheet::new().with_fg(Color::White))
        .with_default_value(StyleSheet::new().with_fg(Color::DarkGrey))
        .with_help_message(StyleSheet::new().with_fg(Color::DarkGrey))
        .with_answer(StyleSheet::new().with_fg(accent))
        .with_error_message(
            ErrorMessageRenderConfig::empty()
                .with_prefix(Styled::new("!!").with_fg(Color::LightRed))
                .with_message(StyleSheet::new().with_fg(Color::LightRed)),
        )
        .with_highlighted_option_prefix(Styled::new(">").with_fg(accent))
        .with_scroll_up_prefix(Styled::new("^").with_fg(Color::DarkGrey))
        .with_scroll_down_prefix(Styled::new("v").with_fg(Color::DarkGrey))
        .with_option(StyleSheet::new().with_fg(Color::Grey))
        .with_selected_option(Some(strong))
        .with_canceled_prompt_indicator(Styled::new("canceled").with_fg(Color::DarkGrey))
}

impl LoginConsole for OxideConsole {
    fn ask_text(&self, prompt: &str) -> Result<String, String> {
        OxideConsole::ask_text(self, prompt)
    }

    fn ask_password(&self, prompt: &str) -> Result<String, String> {
        OxideConsole::ask_password(self, prompt)
    }

    fn ask_integer(&self, prompt: &str) -> Result<i32, String> {
        OxideConsole::ask_integer(self, prompt)
    }

    fn ask_confirm(&self, prompt: &str) -> Result<bool, String> {
        OxideConsole::ask_confirm(self, prompt)
    }

    fn ask_autocomplete(&self, prompt: &str, choices: Vec<String>) -> Result<String, String> {
        OxideConsole::ask_autocomplete(self, prompt, choices)
    }
}

impl BotConsole for OxideConsole {
    fn ask_input(&self, prompt: &str, default: Option<&str>) -> Result<String, String> {
        let input = Text::new(prompt).with_render_config(render_config());
        let input = match default {
            Some(value) => input.with_default(value),
            None => input,
        };

        input
            .prompt()
            .map_err(|error| format!("Failed to get bot configuration input: {error}"))
    }

    fn ask_select(
        &self,
        prompt: &str,
        choices: Vec<String>,
        default: Option<&str>,
    ) -> Result<String, String> {
        if choices.is_empty() {
            return Err("No choices available for select.".to_string());
        }

        let mut select = Select::new(prompt, choices.clone()).with_render_config(render_config());

        if let Some(pos) =
            default.and_then(|def| choices.iter().position(|c| c.eq_ignore_ascii_case(def)))
        {
            select = select.with_starting_cursor(pos);
        }

        select
            .prompt()
            .map_err(|e| format!("Failed to make selection: {e}"))
    }

    fn ask_confirm(&self, prompt: &str, default: Option<bool>) -> Result<bool, String> {
        let mut confirm = Confirm::new(prompt).with_render_config(render_config());
        if let Some(def) = default {
            confirm = confirm.with_default(def);
        }
        confirm
            .prompt()
            .map_err(|e| format!("Failed to get confirmation: {e}"))
    }

    fn ask_file(
        &self,
        prompt: &str,
        default: Option<&str>,
        must_exist: bool,
        allowed_extensions: Option<Vec<String>>,
    ) -> Result<String, String> {
        let autocomplete = FilePathAutocomplete {
            allowed_extensions: allowed_extensions.clone(),
        };

        let mut input = Text::new(prompt)
            .with_autocomplete(autocomplete)
            .with_render_config(render_config());

        if let Some(def) = default {
            input = input.with_default(def);
        }

        let exts_for_validator = allowed_extensions;
        input = input.with_validator(move |val: &str| {
            let trimmed = val.trim();
            if trimmed.is_empty() {
                return Ok(inquire::validator::Validation::Invalid(
                    "File path cannot be empty.".into(),
                ));
            }
            let path = std::path::Path::new(trimmed);
            if must_exist && !path.exists() {
                return Ok(inquire::validator::Validation::Invalid(
                    format!("File does not exist: {trimmed}").into(),
                ));
            }
            if must_exist && path.is_dir() {
                return Ok(inquire::validator::Validation::Invalid(
                    format!("Path is a directory, expected a file: {trimmed}").into(),
                ));
            }
            if let Some(ref exts) = exts_for_validator {
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                let clean_ext = ext.trim_start_matches('.');
                if !exts
                    .iter()
                    .any(|allowed| allowed.eq_ignore_ascii_case(clean_ext))
                {
                    return Ok(inquire::validator::Validation::Invalid(
                        format!(
                            "Invalid file extension '.{clean_ext}'. Allowed: {}",
                            exts.join(", ")
                        )
                        .into(),
                    ));
                }
            }
            Ok(inquire::validator::Validation::Valid)
        });

        input
            .prompt()
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("Failed to get file path input: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_path_autocomplete_finds_cargo_toml() {
        let mut ac = FilePathAutocomplete {
            allowed_extensions: Some(vec!["toml".into()]),
        };
        let suggestions = ac.get_suggestions("Cargo").unwrap();
        assert!(
            suggestions.iter().any(|s| s.contains("Cargo.toml")),
            "Expected Cargo.toml in suggestions: {suggestions:?}"
        );
    }

    #[test]
    fn test_file_path_autocomplete_filters_extension() {
        let mut ac = FilePathAutocomplete {
            allowed_extensions: Some(vec!["nonexistent_ext_xyz".into()]),
        };
        let suggestions = ac.get_suggestions("Cargo.toml").unwrap();
        // Since allowed_extensions doesn't include toml, it should not be suggested
        assert!(!suggestions.iter().any(|s| s == "Cargo.toml"));
    }

    #[test]
    fn test_file_path_autocomplete_directory_trailing_slash() {
        let mut ac = FilePathAutocomplete {
            allowed_extensions: None,
        };
        let suggestions = ac.get_suggestions("src/").unwrap();
        assert!(
            suggestions
                .iter()
                .any(|s| s.contains("src/domain") || s.contains("src/main.rs")),
            "Expected files/dirs under src/: {suggestions:?}"
        );
    }

    #[test]
    fn test_autocomplete_empty_input_returns_all() {
        let mut ac = ChoiceAutocomplete {
            choices: vec!["apple".into(), "banana".into(), "cherry".into()],
        };
        let suggestions = ac.get_suggestions("").unwrap();
        assert_eq!(suggestions, vec!["apple", "banana", "cherry"]);
    }

    #[test]
    fn test_autocomplete_case_insensitive_matching() {
        let mut ac = ChoiceAutocomplete {
            choices: vec!["OxideBot".into(), "RustGram".into(), "TelegramProxy".into()],
        };
        let suggestions = ac.get_suggestions("oxide").unwrap();
        assert_eq!(suggestions, vec!["OxideBot"]);

        let suggestions_gram = ac.get_suggestions("GRAM").unwrap();
        assert_eq!(suggestions_gram, vec!["RustGram", "TelegramProxy"]);
    }

    #[test]
    fn test_autocomplete_no_matches_returns_empty() {
        let mut ac = ChoiceAutocomplete {
            choices: vec!["alpha".into(), "beta".into()],
        };
        let suggestions = ac.get_suggestions("gamma").unwrap();
        assert!(suggestions.is_empty());
    }

    #[test]
    fn test_autocomplete_get_completion() {
        let mut ac = ChoiceAutocomplete {
            choices: vec!["alpha".into()],
        };
        let completion = ac.get_completion("al", Some("alpha".into())).unwrap();
        assert_eq!(completion, Some("alpha".into()));
    }
}
