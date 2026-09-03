//! Grammers and SQLite implementation of the Telegram authentication port.

use crate::application::authentication::{
    ConnectedSession, InteractiveAuthenticator, SessionRepository, SessionRestorer, SignInStatus,
};
use crate::domain::identity::SavedSession;
use crate::errors::{AuthError, OxideError};
use crate::infrastructure::crypto;
use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use grammers_client::client::{LoginToken, PasswordToken};
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_session::updates::UpdatesLike;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{info, warn};

pub struct AuthenticatedClient {
    pub client: Client,
    pub updates: mpsc::Receiver<UpdatesLike>,
    pub phone: String,
    pub session_password: Option<String>,
}

pub struct PendingLogin {
    client: Client,
    token: LoginToken,
    session_path: PathBuf,
    api_id: i32,
    api_hash: String,
    phone: String,
    updates: mpsc::Receiver<UpdatesLike>,
}

#[derive(Default)]
pub struct GrammersAuthGateway;

impl GrammersAuthGateway {
    /// Captures the latest SQLite session state from disk, re-encrypts it with the password,
    /// persists it to the session repository, and securely removes plaintext SQLite files.
    pub async fn persist_and_secure(
        &self,
        phone: &str,
        password: &str,
        repository: &mut (impl SessionRepository + ?Sized),
    ) -> Result<(), OxideError> {
        let path = session_path(phone);
        if path.exists() {
            let bytes = tokio::fs::read(&path)
                .await
                .map_err(|error| AuthError::SessionStorage(error.to_string()))?;
            let salt = crypto::generate_salt()?;
            let encrypted = crypto::encrypt_string(password, &salt, &hex::encode(bytes))?;

            let (api_id, api_hash) = match repository.session(phone) {
                Some(existing) => (existing.api_id, existing.api_hash),
                None => (0, String::new()),
            };

            let updated_session = SavedSession {
                salt: URL_SAFE_NO_PAD.encode(salt),
                api_id,
                api_hash,
                session_string: encrypted,
            };

            repository
                .save_session(phone.to_string(), updated_session)
                .await?;
            info!(phone, "Persisted and re-encrypted session to repository");

            // Clean up plaintext SQLite file and temporary WAL/SHM files
            let _ = tokio::fs::remove_file(&path).await;
            let shm = path.with_extension("session-shm");
            let _ = tokio::fs::remove_file(shm).await;
            let wal = path.with_extension("session-wal");
            let _ = tokio::fs::remove_file(wal).await;
            info!(phone, "Cleaned up plaintext temporary session files");
        }
        Ok(())
    }
}

async fn connect_session(
    path: &Path,
    api_id: i32,
) -> Result<(Client, mpsc::Receiver<UpdatesLike>), OxideError> {
    let session = SqliteSession::open(path)
        .await
        .map_err(|error| AuthError::SessionStorage(error.to_string()))?;
    let SenderPool {
        runner,
        handle,
        updates,
        ..
    } = SenderPool::new(Arc::new(session), api_id);
    tokio::spawn(runner.run());
    Ok((Client::new(handle), updates))
}

fn session_path(phone: &str) -> PathBuf {
    let clean_phone = phone.replace('+', "").trim().to_string();
    PathBuf::from("data")
        .join("sessions")
        .join(format!("{clean_phone}.session"))
}

impl ConnectedSession for GrammersAuthGateway {
    type Connected = AuthenticatedClient;
}

#[async_trait]
impl SessionRestorer for GrammersAuthGateway {
    async fn restore(
        &self,
        phone: &str,
        password: &str,
        session: SavedSession,
    ) -> Result<Self::Connected, OxideError> {
        info!(phone, "Starting saved-session login");
        let salt: [u8; 16] = URL_SAFE_NO_PAD
            .decode(&session.salt)
            .map_err(|error| AuthError::DecryptionFailed(error.to_string()))?
            .try_into()
            .map_err(|_| AuthError::DecryptionFailed("Invalid salt length".to_string()))?;
        let session_hex = crypto::decrypt_string(password, &salt, &session.session_string)
            .map_err(|error| AuthError::DecryptionFailed(error.to_string()))?;
        let bytes = hex::decode(session_hex)
            .map_err(|error| AuthError::DecryptionFailed(error.to_string()))?;

        let path = session_path(phone);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| AuthError::SessionStorage(error.to_string()))?;
        }
        tokio::fs::write(&path, bytes)
            .await
            .map_err(|error| AuthError::SessionStorage(error.to_string()))?;
        let (client, updates) = connect_session(&path, session.api_id).await?;
        info!(phone, "Saved-session login completed");
        Ok(AuthenticatedClient {
            client,
            updates,
            phone: phone.to_string(),
            session_password: Some(password.to_string()),
        })
    }
}

#[async_trait]
impl InteractiveAuthenticator for GrammersAuthGateway {
    type PendingLogin = PendingLogin;
    type PasswordToken = PasswordToken;

    async fn request_login_code(
        &self,
        phone: &str,
        api_id: i32,
        api_hash: &str,
    ) -> Result<Self::PendingLogin, OxideError> {
        info!(phone, api_id, "Starting manual Telegram login");
        let path = session_path(phone);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| AuthError::SessionStorage(error.to_string()))?;
        }
        let (client, updates) = connect_session(&path, api_id).await?;
        let token = client
            .request_login_code(phone, api_hash)
            .await
            .map_err(AuthError::InvocationError)?;
        info!(phone, "Telegram login code requested");
        Ok(PendingLogin {
            client,
            token,
            session_path: path,
            api_id,
            api_hash: api_hash.to_string(),
            phone: phone.to_string(),
            updates,
        })
    }

    async fn sign_in(
        &self,
        pending: &Self::PendingLogin,
        code: &str,
    ) -> Result<SignInStatus<Self::PasswordToken>, OxideError> {
        match pending.client.sign_in(&pending.token, code).await {
            Ok(_) => Ok(SignInStatus::Authorized),
            Err(SignInError::PasswordRequired(token)) => {
                info!("Telegram account requires 2FA");
                Ok(SignInStatus::PasswordRequired(token))
            }
            Err(error) => {
                warn!(error = %error, "Telegram sign-in failed");
                Err(AuthError::SignIn(Box::new(error)).into())
            }
        }
    }

    async fn check_password(
        &self,
        pending: &Self::PendingLogin,
        token: Self::PasswordToken,
        password: &str,
    ) -> Result<(), OxideError> {
        pending
            .client
            .check_password(token, password)
            .await
            .map_err(|error| AuthError::SignIn(Box::new(error)))?;
        Ok(())
    }

    async fn capture_session(
        &self,
        pending: &Self::PendingLogin,
        password: &str,
    ) -> Result<SavedSession, OxideError> {
        let bytes = tokio::fs::read(&pending.session_path)
            .await
            .map_err(|error| AuthError::SessionStorage(error.to_string()))?;
        let salt = crypto::generate_salt()?;
        let encrypted = crypto::encrypt_string(password, &salt, &hex::encode(bytes))?;
        info!("Encrypted Telegram session captured");
        Ok(SavedSession {
            salt: URL_SAFE_NO_PAD.encode(salt),
            api_id: pending.api_id,
            api_hash: pending.api_hash.clone(),
            session_string: encrypted,
        })
    }

    fn complete(&self, pending: Self::PendingLogin, password: &str) -> Self::Connected {
        AuthenticatedClient {
            client: pending.client,
            updates: pending.updates,
            phone: pending.phone,
            session_password: Some(password.to_string()),
        }
    }
}
