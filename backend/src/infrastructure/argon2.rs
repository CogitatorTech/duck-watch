use argon2::password_hash::{PasswordHash as ParsedHash, PasswordVerifier, SaltString};
use argon2::{Argon2, PasswordHasher as _};

use crate::application::services::password::PasswordHasher;
use crate::domain::entities::users::PasswordHash;
use crate::domain::error::{Error, Result};

/// Argon2id with the crate's default parameters, which follow the current
/// OWASP recommendation.
pub struct Argon2PasswordHasher;

impl PasswordHasher for Argon2PasswordHasher {
    fn hash(&self, plain: &str) -> Result<PasswordHash> {
        let salt_bytes: [u8; 16] = rand::random();
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|err| Error::External(anyhow::anyhow!("salt encoding failed: {err}")))?;
        let hash = Argon2::default()
            .hash_password(plain.as_bytes(), &salt)
            .map_err(|err| Error::External(anyhow::anyhow!("password hashing failed: {err}")))?;
        Ok(PasswordHash::new(hash.to_string()))
    }

    fn verify(&self, plain: &str, hash: &PasswordHash) -> Result<bool> {
        let parsed = ParsedHash::new(hash.as_str())
            .map_err(|err| Error::External(anyhow::anyhow!("stored hash is invalid: {err}")))?;
        Ok(Argon2::default()
            .verify_password(plain.as_bytes(), &parsed)
            .is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_then_verify_round_trips() {
        let hasher = Argon2PasswordHasher;
        let hash = hasher.hash("correct horse").unwrap();

        assert!(hasher.verify("correct horse", &hash).unwrap());
        assert!(!hasher.verify("wrong horse", &hash).unwrap());
    }

    #[test]
    fn verify_rejects_a_malformed_stored_hash() {
        let hasher = Argon2PasswordHasher;
        let err = hasher
            .verify("password", &PasswordHash::new("not-a-hash".to_string()))
            .unwrap_err();
        assert!(matches!(err, Error::External(_)));
    }
}
