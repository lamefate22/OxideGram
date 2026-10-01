//! Centralized error handling definitions for OxideGram using `thiserror`.

use std::path::PathBuf;
use thiserror::Error;

/// Error types related to file appender and logger initialization.
#[derive(Debug, Error)]
pub enum LogError {
    #[error("Failed to create tracing appender.")]
    AppenderCreationError(#[source] tracing_appender::rolling::InitError),
}

/// Error types related to Telegram authorization and session management.
#[derive(Debug, Error)]
pub enum AuthError {
    #[error("Grammers invocation error: {0}")]
    InvocationError(#[from] grammers_client::InvocationError),
    #[error("Grammers sign-in error: {0}")]
    SignIn(#[source] Box<grammers_client::SignInError>),
    #[error("Session for {0} already exists in config.")]
    SessionAlreadyExists(String),
    #[error("Session for {0} not found in config.")]
    SessionNotFound(String),
    #[error("Failed to decrypt session data: {0}")]
    DecryptionFailed(String),
    #[error("Interactive UI error: {0}")]
    UiError(String),
    #[error("Telegram session storage error: {0}")]
    SessionStorage(String),
    #[error("Operation timed out: {0}")]
    Timeout(String),
}

/// Error types related to cryptographic operations (Argon2id and AES-256-GCM).
#[derive(Debug, Error)]
pub enum EncryptionError {
    #[error("Invalid data passed to encryption/decryption.")]
    InvalidData,
    #[error("Failed to derive key.")]
    KeyDerivation(#[source] argon2::Error),
    #[error("Failed to encrypt plaintext.")]
    Encryption(#[source] aes_gcm::Error),
    #[error("Failed to decrypt ciphertext.")]
    Decryption(#[source] aes_gcm::Error),
    #[error("Failed to generate salt.")]
    SaltGeneration(#[source] rand::rngs::SysError),
    #[error("Base64 decode error: {0}")]
    Base64(#[from] base64::DecodeError),
}

/// Error types related to TOML configuration loading and saving.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Failed to read or write config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse TOML configuration: {0}")]
    Deserialization(#[from] toml::de::Error),
    #[error("Failed to serialize TOML configuration: {0}")]
    Serialization(#[from] toml::ser::Error),
}

/// Error types related to Lua bot execution and dynamic loading.
#[derive(Debug, Error)]
pub enum ScriptError {
    #[error("Lua runtime error: {0}")]
    LuaError(#[from] mlua::Error),
    #[error("Failed to access bot script at {path}")]
    ScriptIo {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to initialize Telegram update stream: {0}")]
    UpdateStream(String),
    #[error("Operation timed out: {0}")]
    Timeout(String),
    #[error("Script runtime error: {0}")]
    Runtime(String),
}

/// Error types related to filesystem operations.
#[derive(Debug, Error)]
pub enum FileSystemError {
    #[error("Failed to create directory: {0}")]
    FolderCreationError(#[source] std::io::Error),
    #[error("Failed to get current directory: {0}")]
    CurrentDirectoryError(#[source] std::io::Error),
}

/// Master error enum for OxideGram aggregating module-level errors.
#[derive(Debug, Error)]
pub enum OxideError {
    #[error(transparent)]
    Log(#[from] LogError),
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Encryption(#[from] EncryptionError),
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Script(#[from] ScriptError),
    #[error(transparent)]
    FileSystem(#[from] FileSystemError),
}
