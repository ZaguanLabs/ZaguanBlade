//! Integration-local OS credentials. No plaintext fallback and no secret reads
//! exposed through IPC. Errors deliberately discard platform messages/values.
use super::RuntimeError;
use uuid::Uuid;

pub trait SecretStore: Send + Sync {
    fn get(&self, integration: Uuid, name: &str) -> Result<Option<String>, RuntimeError>;

    /// Blade-owned slots (OAuth state). Their keyring names cannot collide with
    /// user-named secrets, which never contain `#`.
    fn get_internal(&self, _integration: Uuid, _key: &str) -> Result<Option<String>, RuntimeError> {
        Err(RuntimeError::SecretUnavailable)
    }
    fn set_internal(
        &self,
        _integration: Uuid,
        _key: &str,
        _value: &str,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::SecretUnavailable)
    }
    fn delete_internal(&self, _integration: Uuid, _key: &str) -> Result<(), RuntimeError> {
        Err(RuntimeError::SecretUnavailable)
    }
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

fn internal_entry(integration: Uuid, key: &str) -> Result<keyring::Entry, RuntimeError> {
    if key.is_empty()
        || key.len() > 64
        || !key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    {
        return Err(RuntimeError::InvalidSecret);
    }
    keyring::Entry::new("ZaguanBlade.Integrations", &format!("{integration}#{key}"))
        .map_err(|_| RuntimeError::SecretUnavailable)
}

fn read(entry: keyring::Entry) -> Result<Option<String>, RuntimeError> {
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(RuntimeError::SecretUnavailable),
    }
}

fn remove(entry: keyring::Entry) -> Result<(), RuntimeError> {
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(_) => Err(RuntimeError::SecretUnavailable),
    }
}

impl SecretStore for OsSecrets {
    fn get(&self, integration: Uuid, name: &str) -> Result<Option<String>, RuntimeError> {
        read(entry(integration, name)?)
    }
    fn get_internal(&self, integration: Uuid, key: &str) -> Result<Option<String>, RuntimeError> {
        read(internal_entry(integration, key)?)
    }
    fn set_internal(&self, integration: Uuid, key: &str, value: &str) -> Result<(), RuntimeError> {
        internal_entry(integration, key)?
            .set_password(value)
            .map_err(|_| RuntimeError::SecretUnavailable)
    }
    fn delete_internal(&self, integration: Uuid, key: &str) -> Result<(), RuntimeError> {
        remove(internal_entry(integration, key)?)
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
        remove(entry(integration, name)?)
    }
}
