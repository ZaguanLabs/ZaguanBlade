//! Integration-local OS credentials. No plaintext fallback and no secret reads
//! exposed through IPC. Errors deliberately discard platform messages/values.
use super::RuntimeError;
use uuid::Uuid;

pub trait SecretStore: Send + Sync {
    fn get(&self, integration: Uuid, name: &str) -> Result<Option<String>, RuntimeError>;
}

pub struct OsSecrets;

fn entry(integration: Uuid, name: &str) -> Result<keyring::Entry, RuntimeError> {
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        return Err(RuntimeError::InvalidSecret);
    }
    keyring::Entry::new("ZaguanBlade.Integrations", &format!("{integration}:{name}"))
        .map_err(|_| RuntimeError::SecretUnavailable)
}

impl SecretStore for OsSecrets {
    fn get(&self, integration: Uuid, name: &str) -> Result<Option<String>, RuntimeError> {
        match entry(integration, name)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(RuntimeError::SecretUnavailable),
        }
    }
}

impl OsSecrets {
    pub fn set(integration: Uuid, name: &str, value: &str) -> Result<(), RuntimeError> {
        if value.is_empty() || value.len() > 8192 || value.contains('\0') {
            return Err(RuntimeError::InvalidSecret);
        }
        entry(integration, name)?
            .set_password(value)
            .map_err(|_| RuntimeError::SecretUnavailable)
    }

    pub fn delete(integration: Uuid, name: &str) -> Result<(), RuntimeError> {
        match entry(integration, name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(RuntimeError::SecretUnavailable),
        }
    }
}
