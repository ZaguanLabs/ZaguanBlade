#[cfg(not(test))]
const SERVICE: &str = "ZaguanBlade";
#[cfg(not(test))]
const USERNAME: &str = "zaguan-cloud-api-key";

// Unit tests must never touch the user's real OS keyring: any test that saves
// a config with a fixture API key would silently replace the user's live
// credential. This actually happened in production — a 24-char test fixture
// overwrote a valid SSO key (docs/internal/2026-07-13-keyring-clobbered-by-tests.md).
// Tests operate on an in-memory store instead.
#[cfg(test)]
static TEST_API_KEY: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[cfg(not(test))]
fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, USERNAME).map_err(|error| error.to_string())
}

#[cfg(not(test))]
pub fn load_api_key() -> Option<String> {
    let key = entry().and_then(|entry| entry.get_password().map_err(|error| error.to_string()));
    match key {
        Ok(api_key) if !api_key.trim().is_empty() => Some(api_key),
        Ok(_) => None,
        Err(_) => None,
    }
}

#[cfg(not(test))]
pub fn store_api_key(api_key: &str) -> Result<(), String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        delete_api_key();
        return Ok(());
    }

    entry()?
        .set_password(trimmed)
        .map_err(|error| error.to_string())
}

#[cfg(not(test))]
pub fn delete_api_key() {
    if let Ok(entry) = entry() {
        let _ = entry.delete_credential();
    }
}

#[cfg(test)]
pub fn load_api_key() -> Option<String> {
    TEST_API_KEY
        .lock()
        .unwrap()
        .clone()
        .filter(|api_key| !api_key.trim().is_empty())
}

#[cfg(test)]
pub fn store_api_key(api_key: &str) -> Result<(), String> {
    let trimmed = api_key.trim();
    let mut stored = TEST_API_KEY.lock().unwrap();
    *stored = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    };
    Ok(())
}

#[cfg(test)]
pub fn delete_api_key() {
    *TEST_API_KEY.lock().unwrap() = None;
}
