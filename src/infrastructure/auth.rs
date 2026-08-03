//! Telegram authentication controller supporting manual login and automatic login.
//!
//! Handles phone number entry, API ID/hash validation, SMS code verification, 2FA passwords,
//! session encryption using Argon2id + AES-256-GCM, and persistent session storage.

use crate::core::config::{OxideConfig, SessionModel};
use crate::core::encryption::{self, generate_salt};
use crate::errors::{AuthError, OxideError};
use crate::ui::console::OxideConsole;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use grammers_client::client::LoginToken;
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_session::updates::UpdatesLike;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

/// Container holding an authorized `grammers` MTProto client and its updates channel.
pub struct AuthenticatedClient {
    pub client: Client,
    pub updates: mpsc::UnboundedReceiver<UpdatesLike>,
}

/// Helper function to perform auto-login for a specific saved account given its phone and master decryption password.
pub async fn perform_auto_login(
    config: &OxideConfig,
    phone: &str,
    dec_password: &str,
) -> Result<AuthenticatedClient, OxideError> {
    info!(phone, "Starting saved-session login");
    let session_data = config
        .get_session(phone)
        .ok_or_else(|| AuthError::SessionNotFound(phone.to_string()))?
        .clone();

    let salt_bytes = URL_SAFE_NO_PAD
        .decode(&session_data.salt)
        .map_err(|e| AuthError::DecryptionFailed(e.to_string()))?;

    let salt: [u8; 16] = salt_bytes
        .try_into()
        .map_err(|_| AuthError::DecryptionFailed("Invalid salt length".to_string()))?;

    let decrypted_hex =
        encryption::decrypt_string(dec_password, &salt, &session_data.session_string)
            .map_err(|e| AuthError::DecryptionFailed(e.to_string()))?;

    let raw_session_bytes =
        hex::decode(decrypted_hex).map_err(|e| AuthError::DecryptionFailed(e.to_string()))?;

    let session_dir = PathBuf::from("data").join("sessions");
    tokio::fs::create_dir_all(&session_dir)
        .await
        .map_err(|e| AuthError::UiError(e.to_string()))?;

    let clean_phone = phone.replace('+', "").trim().to_string();
    let session_path = session_dir.join(format!("{clean_phone}.session"));

    tokio::fs::write(&session_path, raw_session_bytes)
        .await
        .map_err(|e| AuthError::UiError(e.to_string()))?;

    let sqlite_session = SqliteSession::open(&session_path)
        .await
        .map_err(|e| AuthError::UiError(format!("Failed to open SQLite session: {e}")))?;

    let session_arc = Arc::new(sqlite_session);
    let SenderPool {
        runner,
        handle,
        updates,
        ..
    } = SenderPool::new(Arc::clone(&session_arc), session_data.api_id);
    let client = Client::new(handle);
    tokio::spawn(runner.run());

    info!(phone, "Saved-session login completed");
    Ok(AuthenticatedClient { client, updates })
}

/// Step 1 of manual login: initiates client connection and requests login SMS code.
pub async fn start_manual_login(
    phone: &str,
    api_id: i32,
    api_hash: &str,
) -> Result<
    (
        Client,
        LoginToken,
        PathBuf,
        mpsc::UnboundedReceiver<UpdatesLike>,
    ),
    OxideError,
> {
    info!(phone, api_id, "Starting manual Telegram login");
    let session_dir = PathBuf::from("data").join("sessions");
    tokio::fs::create_dir_all(&session_dir)
        .await
        .map_err(|e| AuthError::UiError(e.to_string()))?;

    let clean_phone = phone.replace('+', "").trim().to_string();
    let session_path = session_dir.join(format!("{clean_phone}.session"));

    let sqlite_session = SqliteSession::open(&session_path)
        .await
        .map_err(|e| AuthError::UiError(format!("Failed to open SQLite session: {e}")))?;

    let session_arc = Arc::new(sqlite_session);
    let SenderPool {
        runner,
        handle,
        updates,
        ..
    } = SenderPool::new(Arc::clone(&session_arc), api_id);
    let client = Client::new(handle);
    tokio::spawn(runner.run());

    let token = client
        .request_login_code(phone, api_hash)
        .await
        .map_err(AuthError::InvocationError)?;

    info!(phone, "Telegram login code requested");
    Ok((client, token, session_path, updates))
}

/// Encrypts session and persists it into OxideConfig TOML file.
pub async fn save_manual_session(
    config: &mut OxideConfig,
    phone: &str,
    api_id: i32,
    api_hash: &str,
    session_path: &PathBuf,
    enc_password: &str,
) -> Result<(), OxideError> {
    let raw_session_bytes = tokio::fs::read(session_path)
        .await
        .map_err(|e| AuthError::UiError(format!("Failed to read session file: {e}")))?;

    let salt = generate_salt()?;
    let session_hex = hex::encode(raw_session_bytes);
    let encrypted_session = encryption::encrypt_string(enc_password, &salt, &session_hex)?;

    let session_model = SessionModel {
        salt: URL_SAFE_NO_PAD.encode(salt),
        api_id,
        api_hash: api_hash.to_string(),
        session_string: encrypted_session,
    };

    config.add_session(phone.to_string(), session_model).await?;
    info!(phone, "Encrypted Telegram session saved");
    Ok(())
}

/// Controller managing user login state for CLI mode.
pub struct OxideAuth<'a> {
    pub config: &'a mut OxideConfig,
    pub console: &'a OxideConsole,
}

impl<'a> OxideAuth<'a> {
    /// Creates a new `OxideAuth` controller with configuration and UI console references.
    pub fn new(config: &'a mut OxideConfig, console: &'a OxideConsole) -> Self {
        Self { config, console }
    }

    /// Primary authentication method for CLI. Prompts user for auto-login or manual login.
    pub async fn login(&mut self) -> Result<AuthenticatedClient, OxideError> {
        self.config.load().await?;

        let use_auto = self
            .console
            .ask_confirm("Do you want to use auto-login?")
            .map_err(AuthError::UiError)?;

        if use_auto && !self.config.data.sessions.is_empty() {
            self.auto_login().await
        } else {
            self.manual_login().await
        }
    }

    /// Performs manual login CLI workflow.
    async fn manual_login(&mut self) -> Result<AuthenticatedClient, OxideError> {
        let phone = self
            .console
            .ask_text("Enter phone number (e.g. +1234567890):")
            .map_err(AuthError::UiError)?;

        if self.config.get_session(&phone).is_some() {
            return Err(AuthError::SessionAlreadyExists(phone).into());
        }

        let api_id = self
            .console
            .ask_integer("Enter account API_ID:")
            .map_err(AuthError::UiError)?;

        let api_hash = self
            .console
            .ask_text("Enter account API_HASH:")
            .map_err(AuthError::UiError)?;

        let (client, token, session_path, updates) =
            start_manual_login(&phone, api_id, &api_hash).await?;

        let code = self
            .console
            .ask_text("Enter the code you received:")
            .map_err(AuthError::UiError)?;

        match client.sign_in(&token, &code).await {
            Ok(_) => {}
            Err(SignInError::PasswordRequired(password_token)) => {
                info!(phone, "Telegram account requires 2FA");
                let password = self
                    .console
                    .ask_password("Enter 2FA password:")
                    .map_err(AuthError::UiError)?;

                client
                    .check_password(password_token, password.trim())
                    .await
                    .map_err(AuthError::SignInError)?;
            }
            Err(e) => {
                warn!(phone, error = %e, "Telegram sign-in failed");
                return Err(AuthError::SignInError(e).into());
            }
        }

        let enc_password = self
            .console
            .ask_password("Enter your encryption password:")
            .map_err(AuthError::UiError)?;

        save_manual_session(
            &mut self.config,
            &phone,
            api_id,
            &api_hash,
            &session_path,
            &enc_password,
        )
        .await?;

        Ok(AuthenticatedClient { client, updates })
    }

    /// Performs auto login CLI workflow.
    async fn auto_login(&mut self) -> Result<AuthenticatedClient, OxideError> {
        let choices: Vec<String> = self.config.data.sessions.keys().cloned().collect();

        let phone = self
            .console
            .ask_autocomplete("Select an authorized session:", choices)
            .map_err(AuthError::UiError)?;

        let dec_password = self
            .console
            .ask_password("Enter your decryption password:")
            .map_err(AuthError::UiError)?;

        perform_auto_login(self.config, &phone, &dec_password).await
    }
}
