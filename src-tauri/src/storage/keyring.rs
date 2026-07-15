use keyring::Entry;

const SERVICE_NAME: &str = "retl";

/// Store API key securely in system keychain
pub fn store_api_key(provider: &str, api_key: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, provider)
        .map_err(|e| format!("无法创建 keyring entry: {}", e))?;

    entry.set_password(api_key)
        .map_err(|e| format!("无法存储 API Key: {}", e))
}

/// Retrieve API key from system keychain
pub fn get_api_key(provider: &str) -> Result<Option<String>, String> {
    let entry = Entry::new(SERVICE_NAME, provider)
        .map_err(|e| format!("无法创建 keyring entry: {}", e))?;

    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("无法读取 API Key: {}", e)),
    }
}

/// Delete API key from system keychain
#[allow(dead_code)]
pub fn delete_api_key(provider: &str) -> Result<(), String> {
    let entry = Entry::new(SERVICE_NAME, provider)
        .map_err(|e| format!("无法创建 keyring entry: {}", e))?;

    match entry.delete_credential() {
        Ok(_) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
        Err(e) => Err(format!("无法删除 API Key: {}", e)),
    }
}
