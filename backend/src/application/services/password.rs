use crate::domain::entities::users::PasswordHash;
use crate::domain::error::Result;

/// Password hashing boundary. The implementation lives in infrastructure so
/// the algorithm (argon2 today) stays swappable and mockable. Hashing is
/// CPU-bound and takes on the order of 100 ms; at MVP auth volume that block
/// is acceptable inside an async handler.
pub trait PasswordHasher: Send + Sync {
    fn hash(&self, plain: &str) -> Result<PasswordHash>;
    fn verify(&self, plain: &str, hash: &PasswordHash) -> Result<bool>;
}

#[cfg(test)]
mockall::mock! {
    pub PasswordHasher {}
    impl PasswordHasher for PasswordHasher {
        fn hash(&self, plain: &str) -> Result<PasswordHash>;
        fn verify(&self, plain: &str, hash: &PasswordHash) -> Result<bool>;
    }
}
