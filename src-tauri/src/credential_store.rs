const SERVICE: &str = "ZaguanBlade";
const USERNAME: &str = "zaguan-cloud-api-key";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, USERNAME).map_err(|error| error.to_string())
}

pub fn load_api_key() -> Option<String> {
    let key = entry().and_then(|entry| entry.get_password().map_err(|error| error.to_string()));
    match key {
        Ok(api_key) if !api_key.trim().is_empty() => Some(api_key),
        Ok(_) => None,
        Err(_) => None,
    }
}

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

pub fn delete_api_key() {
    if let Ok(entry) = entry() {
        let _ = entry.delete_credential();
    }
}
