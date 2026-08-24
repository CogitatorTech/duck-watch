use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::domain::error::{Error, Result};

// `u64` because `validator` length rules in the web layer expect that type.
pub const ORG_NAME_MAX_LEN: u64 = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct Organization {
    pub id: Uuid,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// One organization as the platform operator sees it: the tenant, how many
/// users it has, and the sync health of its connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(serde::Deserialize))]
pub struct OrganizationOverview {
    pub organization: Organization,
    pub user_count: i64,
    pub connections: Vec<crate::domain::entities::motherduck_connections::MotherDuckConnection>,
}

/// The fields a caller supplies when creating an organization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganizationDraft {
    name: String,
}

impl OrganizationDraft {
    pub fn new(name: &str) -> Result<Self> {
        let name = name.trim();
        if name.is_empty() {
            return Err(Error::validation("organization name must not be empty"));
        }
        if name.chars().count() as u64 > ORG_NAME_MAX_LEN {
            return Err(Error::validation(format!(
                "organization name must be at most {ORG_NAME_MAX_LEN} characters"
            )));
        }

        Ok(Self {
            name: name.to_string(),
        })
    }

    /// Builds a new organization, stamping the identifier and timestamps.
    pub fn into_new_organization(self, now: DateTime<Utc>) -> Organization {
        Organization {
            id: Uuid::new_v4(),
            name: self.name,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_trims_the_name() {
        let org = OrganizationDraft::new("  acme  ")
            .unwrap()
            .into_new_organization(Utc::now());
        assert_eq!(org.name, "acme");
    }

    #[test]
    fn new_rejects_a_blank_name() {
        let err = OrganizationDraft::new("   ").unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }

    #[test]
    fn new_rejects_an_overlong_name() {
        let err = OrganizationDraft::new(&"a".repeat(ORG_NAME_MAX_LEN as usize + 1)).unwrap_err();
        assert!(matches!(err, Error::Validation(_)));
    }
}
