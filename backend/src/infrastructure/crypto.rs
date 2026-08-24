use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;

use crate::domain::error::{Error, Result};

pub const NONCE_LEN: usize = 12;

/// AES-256-GCM cipher for secrets at rest, keyed from the environment. Losing
/// the key orphans every stored token, so it must be backed up with the
/// database.
pub struct SecretCipher {
    cipher: Aes256Gcm,
}

impl SecretCipher {
    /// Builds the cipher from a base64 encoded 32-byte key.
    pub fn from_base64_key(encoded: &str) -> Result<Self> {
        if encoded.trim().is_empty() {
            return Err(Error::External(anyhow::anyhow!(
                "TOKEN_ENCRYPTION_KEY is not set; generate one with: \
                 openssl rand -base64 32"
            )));
        }
        let bytes = STANDARD.decode(encoded.trim()).map_err(|err| {
            Error::External(anyhow::anyhow!("encryption key is not base64: {err}"))
        })?;
        if bytes.len() != 32 {
            return Err(Error::External(anyhow::anyhow!(
                "encryption key must decode to 32 bytes, got {}",
                bytes.len()
            )));
        }
        Ok(Self {
            cipher: Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&bytes)),
        })
    }

    /// Encrypts a secret under a fresh random nonce, returning the pair that
    /// gets stored.
    pub fn encrypt(&self, plaintext: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        let nonce_bytes: [u8; NONCE_LEN] = rand::random();
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self
            .cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| Error::External(anyhow::anyhow!("secret encryption failed")))?;
        Ok((ciphertext, nonce_bytes.to_vec()))
    }

    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8]) -> Result<String> {
        if nonce.len() != NONCE_LEN {
            return Err(Error::External(anyhow::anyhow!("stored nonce is invalid")));
        }
        let plaintext = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), ciphertext)
            .map_err(|_| Error::External(anyhow::anyhow!("secret decryption failed")))?;
        String::from_utf8(plaintext)
            .map_err(|_| Error::External(anyhow::anyhow!("decrypted secret is not utf-8")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cipher() -> SecretCipher {
        SecretCipher::from_base64_key(&STANDARD.encode([7u8; 32])).unwrap()
    }

    #[test]
    fn encrypt_then_decrypt_round_trips() {
        let cipher = cipher();
        let (ciphertext, nonce) = cipher.encrypt("motherduck-token").unwrap();

        assert_eq!(
            cipher.decrypt(&ciphertext, &nonce).unwrap(),
            "motherduck-token"
        );
    }

    #[test]
    fn nonces_differ_between_encryptions() {
        let cipher = cipher();
        let (_, first) = cipher.encrypt("secret").unwrap();
        let (_, second) = cipher.encrypt("secret").unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn decrypt_rejects_tampered_ciphertext() {
        let cipher = cipher();
        let (mut ciphertext, nonce) = cipher.encrypt("secret").unwrap();
        ciphertext[0] ^= 0xff;

        assert!(cipher.decrypt(&ciphertext, &nonce).is_err());
    }

    #[test]
    fn from_base64_key_rejects_an_unset_key() {
        // The example env file ships this empty on purpose, so an install
        // that skipped the step fails at startup rather than encrypting
        // tokens under a key everyone knows.
        let message = match SecretCipher::from_base64_key("  ") {
            Err(err) => format!("{err}"),
            Ok(_) => panic!("an unset key must be refused"),
        };
        assert!(message.contains("TOKEN_ENCRYPTION_KEY"), "{message}");
    }

    #[test]
    fn from_base64_key_rejects_a_short_key() {
        assert!(SecretCipher::from_base64_key(&STANDARD.encode([7u8; 16])).is_err());
    }

    #[test]
    fn from_base64_key_rejects_garbage() {
        assert!(SecretCipher::from_base64_key("not base64!!").is_err());
    }
}
