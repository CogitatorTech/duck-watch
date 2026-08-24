use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// The opaque bearer token handed to the client. Only its hash is stored, so a
/// database leak does not leak usable credentials. It never serializes as part
/// of an entity and never prints.
#[derive(Clone, PartialEq, Eq)]
pub struct SessionToken(String);

impl SessionToken {
    /// Generates a fresh random token: 32 bytes, base64url without padding.
    pub fn generate() -> Self {
        let bytes: [u8; 32] = rand::random();
        Self(URL_SAFE_NO_PAD.encode(bytes))
    }

    /// Reconstructs a token presented by a client, for hashing and lookup.
    pub fn from_raw(raw: &str) -> Self {
        Self(raw.to_string())
    }

    /// The value stored and looked up in place of the token itself.
    pub fn hash(&self) -> Vec<u8> {
        Sha256::digest(self.0.as_bytes()).to_vec()
    }

    /// Exposes the token once, to return it to the client after login.
    pub fn reveal(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SessionToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SessionToken(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: Uuid,
    pub user_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

impl Session {
    /// Builds a new session for a user with the given lifetime.
    pub fn new(user_id: Uuid, ttl: Duration, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            user_id,
            expires_at: now + ttl,
            created_at: now,
        }
    }

    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        self.expires_at <= now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_distinct_tokens() {
        assert_ne!(
            SessionToken::generate().reveal(),
            SessionToken::generate().reveal()
        );
    }

    #[test]
    fn hash_is_stable_for_the_same_token() {
        let token = SessionToken::generate();
        let reconstructed = SessionToken::from_raw(token.reveal());
        assert_eq!(token.hash(), reconstructed.hash());
    }

    #[test]
    fn debug_is_redacted() {
        assert_eq!(
            format!("{:?}", SessionToken::generate()),
            "SessionToken(<redacted>)"
        );
    }

    #[test]
    fn is_expired_matches_the_deadline() {
        let now = Utc::now();
        let session = Session::new(Uuid::new_v4(), Duration::hours(1), now);
        assert!(!session.is_expired(now));
        assert!(session.is_expired(now + Duration::hours(1)));
    }
}
