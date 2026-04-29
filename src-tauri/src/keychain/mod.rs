use anyhow::{anyhow, Result};
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

const SERVICE: &str = "com.whisp.whisp-rs";

pub fn set(key: &str, value: &str) -> Result<()> {
    set_generic_password(SERVICE, key, value.as_bytes())
        .map_err(|e| anyhow!("keychain set failed for '{}': {}", key, e))
}

pub fn get(key: &str) -> Result<Option<String>> {
    match get_generic_password(SERVICE, key) {
        Ok(bytes) => {
            let s = String::from_utf8(bytes)
                .map_err(|e| anyhow!("keychain value not utf8: {}", e))?;
            Ok(Some(s))
        }
        Err(e) => {
            // errSecItemNotFound (-25300) is not an error
            if e.code() == -25300 {
                Ok(None)
            } else {
                Err(anyhow!("keychain get failed for '{}': {}", key, e))
            }
        }
    }
}

pub fn delete(key: &str) -> Result<()> {
    match delete_generic_password(SERVICE, key) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == -25300 => Ok(()), // already absent
        Err(e) => Err(anyhow!("keychain delete failed for '{}': {}", key, e)),
    }
}
