use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::error::{Error, Result};

// `u64` because `validator` length rules in the web layer expect that type.
pub const EMAIL_MAX_LEN: u64 = 320;
pub const PASSWORD_MIN_LEN: u64 = 8;
pub const PASSWORD_MAX_LEN: u64 = 128;

/// A normalized email address: trimmed, lowercased, and shaped like an email.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Email(String);

impl Email {
    pub fn new(raw: &str) -> Result<Self> {
        let email = raw.trim().to_lowercase();
        if email.chars().count() as u64 > EMAIL_MAX_LEN {
            return Err(Error::validation(format!(
                "email must be at most {EMAIL_MAX_LEN} characters"
            )));
        }

        // A full RFC 5322 parse buys nothing here; the address only has to be
        // unique and deliverable enough to name an account.
        let mut parts = email.splitn(2, '@');
        let local = parts.next().unwrap_or_default();
        let domain = parts.next().unwrap_or_default();
        if local.is_empty() || domain.is_empty() || !domain.contains('.') {
            return Err(Error::validation("email address is not valid"));
        }

        Ok(Self(email))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An argon2 password hash string. It never serializes and never prints.
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordHash(String);

impl PasswordHash {
    pub fn new(hash: String) -> Self {
        Self(hash)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for PasswordHash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PasswordHash(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct User {
    pub id: Uuid,
    pub email: Email,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    /// Builds the account, stamping the identifier and timestamps. DuckWatch
    /// is a single account tool, so there is only ever one of these.
    pub fn new(email: Email, now: DateTime<Utc>) -> Self {
        Self {
            id: Uuid::new_v4(),
            email,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
impl<'de> serde::Deserialize<'de> for Email {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        Email::new(&raw).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_normalizes_the_address() {
        let email = Email::new("  User@Example.COM  ").unwrap();
        assert_eq!(email.as_str(), "user@example.com");
    }

    #[test]
    fn new_rejects_a_missing_at_sign() {
        assert!(matches!(
            Email::new("user.example.com").unwrap_err(),
            Error::Validation(_)
        ));
    }

    #[test]
    fn new_rejects_a_missing_local_part() {
        assert!(matches!(
            Email::new("@example.com").unwrap_err(),
            Error::Validation(_)
        ));
    }

    #[test]
    fn new_rejects_a_domain_without_a_dot() {
        assert!(matches!(
            Email::new("user@localhost").unwrap_err(),
            Error::Validation(_)
        ));
    }

    #[test]
    fn new_rejects_an_overlong_address() {
        let raw = format!("{}@example.com", "a".repeat(EMAIL_MAX_LEN as usize));
        assert!(matches!(
            Email::new(&raw).unwrap_err(),
            Error::Validation(_)
        ));
    }

    #[test]
    fn password_hash_debug_is_redacted() {
        let hash = PasswordHash::new("secret".to_string());
        assert_eq!(format!("{hash:?}"), "PasswordHash(<redacted>)");
    }

    #[test]
    fn user_new_stamps_matching_timestamps() {
        let now = Utc::now();
        let user = User::new(Email::new("a@b.co").unwrap(), now);
        assert_eq!(user.created_at, user.updated_at);
        assert!(!user.id.is_nil());
    }
}
