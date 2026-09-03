//! Authentication use case and its dependency ports.

use crate::domain::identity::SavedSession;
use crate::errors::{AuthError, OxideError};
use async_trait::async_trait;

#[async_trait]
pub trait SessionRepository: Send {
    fn session_phones(&self) -> Vec<String>;
    fn session(&self, phone: &str) -> Option<SavedSession>;
    async fn save_session(
        &mut self,
        phone: String,
        session: SavedSession,
    ) -> Result<(), OxideError>;
}

pub trait LoginConsole: Sync {
    fn ask_text(&self, prompt: &str) -> Result<String, String>;
    fn ask_password(&self, prompt: &str) -> Result<String, String>;
    fn ask_integer(&self, prompt: &str) -> Result<i32, String>;
    fn ask_confirm(&self, prompt: &str) -> Result<bool, String>;
    fn ask_autocomplete(&self, prompt: &str, choices: Vec<String>) -> Result<String, String>;
}

pub enum SignInStatus<T> {
    Authorized,
    PasswordRequired(T),
}

/// Common trait providing associated Connected client type.
pub trait ConnectedSession {
    type Connected;
}

/// Port responsible for restoring an existing authorized Telegram session.
#[async_trait]
pub trait SessionRestorer: ConnectedSession + Sync {
    async fn restore(
        &self,
        phone: &str,
        password: &str,
        session: SavedSession,
    ) -> Result<Self::Connected, OxideError>;
}

/// Port responsible for interactive manual authentication via login code and 2FA.
#[async_trait]
pub trait InteractiveAuthenticator: ConnectedSession + Sync {
    type PendingLogin: Send;
    type PasswordToken: Send;

    async fn request_login_code(
        &self,
        phone: &str,
        api_id: i32,
        api_hash: &str,
    ) -> Result<Self::PendingLogin, OxideError>;
    async fn sign_in(
        &self,
        pending: &Self::PendingLogin,
        code: &str,
    ) -> Result<SignInStatus<Self::PasswordToken>, OxideError>;
    async fn check_password(
        &self,
        pending: &Self::PendingLogin,
        token: Self::PasswordToken,
        password: &str,
    ) -> Result<(), OxideError>;
    async fn capture_session(
        &self,
        pending: &Self::PendingLogin,
        password: &str,
    ) -> Result<SavedSession, OxideError>;
    fn complete(&self, pending: Self::PendingLogin, password: &str) -> Self::Connected;
}

/// Combined authentication gateway unifying session restoration and interactive authentication.
pub trait TelegramAuthGateway: SessionRestorer + InteractiveAuthenticator {}

impl<T> TelegramAuthGateway for T where T: SessionRestorer + InteractiveAuthenticator {}

pub struct LoginService<'a, R, C, G> {
    sessions: &'a mut R,
    console: &'a C,
    telegram: &'a G,
}

impl<'a, R, C, G> LoginService<'a, R, C, G>
where
    R: SessionRepository,
    C: LoginConsole,
    G: TelegramAuthGateway,
{
    pub fn new(sessions: &'a mut R, console: &'a C, telegram: &'a G) -> Self {
        Self {
            sessions,
            console,
            telegram,
        }
    }

    pub async fn login(&mut self) -> Result<G::Connected, OxideError> {
        let use_saved = self
            .console
            .ask_confirm("Do you want to use auto-login?")
            .map_err(AuthError::UiError)?;

        if use_saved && !self.sessions.session_phones().is_empty() {
            self.saved_session_login().await
        } else {
            self.manual_login().await
        }
    }

    async fn saved_session_login(&self) -> Result<G::Connected, OxideError> {
        let phone = self
            .console
            .ask_autocomplete(
                "Select an authorized session:",
                self.sessions.session_phones(),
            )
            .map_err(AuthError::UiError)?;
        let password = self
            .console
            .ask_password("Enter your decryption password:")
            .map_err(AuthError::UiError)?;
        let session = self
            .sessions
            .session(&phone)
            .ok_or_else(|| AuthError::SessionNotFound(phone.clone()))?;

        self.telegram.restore(&phone, &password, session).await
    }

    async fn manual_login(&mut self) -> Result<G::Connected, OxideError> {
        let phone = self
            .console
            .ask_text("Enter phone number (e.g. +1234567890):")
            .map_err(AuthError::UiError)?;
        if self.sessions.session(&phone).is_some() {
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
        let pending = self
            .telegram
            .request_login_code(&phone, api_id, &api_hash)
            .await?;
        let code = self
            .console
            .ask_text("Enter the code you received:")
            .map_err(AuthError::UiError)?;

        if let SignInStatus::PasswordRequired(token) =
            self.telegram.sign_in(&pending, &code).await?
        {
            let password = self
                .console
                .ask_password("Enter 2FA password:")
                .map_err(AuthError::UiError)?;
            self.telegram
                .check_password(&pending, token, password.trim())
                .await?;
        }

        let encryption_password = self
            .console
            .ask_password("Enter your encryption password:")
            .map_err(AuthError::UiError)?;
        let session = self
            .telegram
            .capture_session(&pending, &encryption_password)
            .await?;
        self.sessions.save_session(phone, session).await?;
        Ok(self.telegram.complete(pending, &encryption_password))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemorySessions {
        sessions: Vec<(String, SavedSession)>,
    }

    #[async_trait]
    impl SessionRepository for MemorySessions {
        fn session_phones(&self) -> Vec<String> {
            self.sessions
                .iter()
                .map(|(phone, _)| phone.clone())
                .collect()
        }

        fn session(&self, phone: &str) -> Option<SavedSession> {
            self.sessions
                .iter()
                .find(|(stored, _)| stored == phone)
                .map(|(_, session)| session.clone())
        }

        async fn save_session(
            &mut self,
            phone: String,
            session: SavedSession,
        ) -> Result<(), OxideError> {
            self.sessions.push((phone, session));
            Ok(())
        }
    }

    struct SavedLoginConsole;

    impl LoginConsole for SavedLoginConsole {
        fn ask_text(&self, _: &str) -> Result<String, String> {
            unreachable!()
        }

        fn ask_password(&self, _: &str) -> Result<String, String> {
            Ok("secret".to_string())
        }

        fn ask_integer(&self, _: &str) -> Result<i32, String> {
            unreachable!()
        }

        fn ask_confirm(&self, _: &str) -> Result<bool, String> {
            Ok(true)
        }

        fn ask_autocomplete(&self, _: &str, choices: Vec<String>) -> Result<String, String> {
            Ok(choices[0].clone())
        }
    }

    #[derive(Default)]
    struct FakeGateway {
        restored_phone: Mutex<Option<String>>,
    }

    impl ConnectedSession for FakeGateway {
        type Connected = String;
    }

    #[async_trait]
    impl SessionRestorer for FakeGateway {
        async fn restore(
            &self,
            phone: &str,
            _: &str,
            _: SavedSession,
        ) -> Result<Self::Connected, OxideError> {
            *self.restored_phone.lock().unwrap() = Some(phone.to_string());
            Ok("connected".to_string())
        }
    }

    #[async_trait]
    impl InteractiveAuthenticator for FakeGateway {
        type PendingLogin = ();
        type PasswordToken = ();

        async fn request_login_code(
            &self,
            _: &str,
            _: i32,
            _: &str,
        ) -> Result<Self::PendingLogin, OxideError> {
            unreachable!()
        }

        async fn sign_in(
            &self,
            _: &Self::PendingLogin,
            _: &str,
        ) -> Result<SignInStatus<Self::PasswordToken>, OxideError> {
            unreachable!()
        }

        async fn check_password(
            &self,
            _: &Self::PendingLogin,
            _: Self::PasswordToken,
            _: &str,
        ) -> Result<(), OxideError> {
            unreachable!()
        }

        async fn capture_session(
            &self,
            _: &Self::PendingLogin,
            _: &str,
        ) -> Result<SavedSession, OxideError> {
            unreachable!()
        }

        fn complete(&self, _: Self::PendingLogin, _: &str) -> Self::Connected {
            unreachable!()
        }
    }

    fn saved_session() -> SavedSession {
        SavedSession {
            salt: "salt".to_string(),
            api_id: 1,
            api_hash: "hash".to_string(),
            session_string: "encrypted".to_string(),
        }
    }

    #[tokio::test]
    async fn saved_login_is_orchestrated_through_ports() {
        let mut sessions = MemorySessions {
            sessions: vec![("+100".to_string(), saved_session())],
        };
        let console = SavedLoginConsole;
        let gateway = FakeGateway::default();
        let mut service = LoginService::new(&mut sessions, &console, &gateway);

        assert_eq!(service.login().await.unwrap(), "connected");
        assert_eq!(
            gateway.restored_phone.lock().unwrap().as_deref(),
            Some("+100")
        );
    }
}
