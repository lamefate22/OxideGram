//! Cryptographic adapter for persisted session payloads.
//!
//! Uses Argon2id for password key derivation and AES-256-GCM for authenticated encryption.

use crate::errors::{EncryptionError, OxideError};
use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use argon2::Argon2;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::{TryRng, rngs::SysRng};

pub const KEY_LEN: usize = 32;
pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 12;

/// Generates a random 16-byte salt using system RNG.
pub fn generate_salt() -> Result<[u8; SALT_LEN], OxideError> {
    let mut buffer = [0u8; SALT_LEN];
    SysRng
        .try_fill_bytes(&mut buffer)
        .map_err(EncryptionError::SaltGeneration)?;
    Ok(buffer)
}

/// Derives a 32-byte key from password and salt using Argon2.
pub fn derive_key(password: &[u8], salt: &[u8]) -> Result<[u8; KEY_LEN], OxideError> {
    let mut out = [0u8; KEY_LEN];
    Argon2::default()
        .hash_password_into(password, salt, &mut out)
        .map_err(EncryptionError::KeyDerivation)?;
    Ok(out)
}

/// Encrypts plaintext bytes using AES-256-GCM.
///
/// Prepends a randomly generated 12-byte nonce to the resulting ciphertext output.
pub fn encrypt(key: &[u8; KEY_LEN], plaintext: &[u8]) -> Result<Vec<u8>, OxideError> {
    if plaintext.is_empty() {
        return Err(EncryptionError::InvalidData.into());
    }

    let mut nonce_bytes = [0u8; NONCE_LEN];
    SysRng
        .try_fill_bytes(&mut nonce_bytes)
        .map_err(EncryptionError::SaltGeneration)?;
    let nonce = Nonce::from(nonce_bytes);

    let cipher = Aes256Gcm::new(key.into());
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .map_err(EncryptionError::Encryption)?;

    let mut result = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Decrypts ciphertext bytes (including prepended 12-byte nonce) using AES-256-GCM.
pub fn decrypt(key: &[u8; KEY_LEN], data: &[u8]) -> Result<Vec<u8>, OxideError> {
    if data.len() < NONCE_LEN {
        return Err(EncryptionError::InvalidData.into());
    }

    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let nonce_arr: [u8; 12] = nonce_bytes
        .try_into()
        .map_err(|_| EncryptionError::InvalidData)?;
    let nonce = Nonce::from(nonce_arr);
    let cipher = Aes256Gcm::new(key.into());

    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(EncryptionError::Decryption)?;
    Ok(plaintext)
}

/// Encrypts a text string and returns a URL-safe Base64 encoded ciphertext string.
pub fn encrypt_string(
    password: &str,
    salt: &[u8; SALT_LEN],
    plaintext: &str,
) -> Result<String, OxideError> {
    let key = derive_key(password.as_bytes(), salt)?;
    let encrypted_bytes = encrypt(&key, plaintext.as_bytes())?;
    Ok(URL_SAFE_NO_PAD.encode(encrypted_bytes))
}

/// Decrypts a URL-safe Base64 encoded ciphertext string into the original plaintext string.
pub fn decrypt_string(
    password: &str,
    salt: &[u8; SALT_LEN],
    ciphertext_b64: &str,
) -> Result<String, OxideError> {
    let key = derive_key(password.as_bytes(), salt)?;
    let ciphertext_bytes = URL_SAFE_NO_PAD
        .decode(ciphertext_b64)
        .map_err(EncryptionError::Base64)?;
    let decrypted_bytes = decrypt(&key, &ciphertext_bytes)?;
    let text = String::from_utf8(decrypted_bytes).map_err(|_| EncryptionError::InvalidData)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_string() {
        let password = "test_password_123";
        let salt = generate_salt().unwrap();
        let original = "Hello World! Telegram session secret payload.";

        let encrypted = encrypt_string(password, &salt, original).unwrap();
        let decrypted = decrypt_string(password, &salt, &encrypted).unwrap();

        assert_eq!(original, decrypted);
    }

    #[test]
    fn test_wrong_password_fails() {
        let password = "correct_password";
        let wrong_password = "wrong_password";
        let salt = generate_salt().unwrap();
        let original = "Secret Data";

        let encrypted = encrypt_string(password, &salt, original).unwrap();
        let result = decrypt_string(wrong_password, &salt, &encrypted);

        assert!(result.is_err());
    }
}
