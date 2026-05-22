use anyhow::Result;
use std::path::PathBuf;

use super::models::AppConfig;

pub fn app_support_dir() -> Result<PathBuf> {
    #[cfg(target_os = "ios")]
    {
        let home = std::env::var("HOME")
            .map_err(|_| anyhow::anyhow!("HOME env var not set"))?;
        let dir = PathBuf::from(home).join("Documents");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME")
            .map_err(|_| anyhow::anyhow!("HOME env var not set"))?;
        // Use the existing path that matches the macOS keychain service name.
        let dir = PathBuf::from(home)
            .join("Library")
            .join("Application Support")
            .join("com.whisp2.app");
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        use directories::ProjectDirs;
        let dirs = ProjectDirs::from("com", "whisp2", "app")
            .ok_or_else(|| anyhow::anyhow!("cannot determine app data directory"))?;
        let dir = dirs.data_dir().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

fn config_path() -> Result<PathBuf> {
    Ok(app_support_dir()?.join("config.json"))
}

pub fn load() -> Result<AppConfig> {
    let path = config_path()?;
    if !path.exists() {
        #[cfg(target_os = "macos")]
        if let Ok(old) = migrate_from_old_path(&path) {
            return Ok(old);
        }
        return Ok(AppConfig::default());
    }
    let bytes = std::fs::read(&path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

pub fn save(config: &AppConfig) -> Result<()> {
    let path = config_path()?;
    let bytes = serde_json::to_vec_pretty(config)?;
    std::fs::write(&path, bytes)?;
    Ok(())
}

/// One-time migration from com.whisp.whisp-rs → com.whisp2.app.
#[cfg(target_os = "macos")]
fn migrate_from_old_path(new_path: &PathBuf) -> Result<AppConfig> {
    let home = std::env::var("HOME")?;
    let old_dir = PathBuf::from(&home)
        .join("Library")
        .join("Application Support")
        .join("com.whisp.whisp-rs");
    let old_config = old_dir.join("config.json");
    if !old_config.exists() {
        anyhow::bail!("no old config");
    }
    let new_dir = new_path.parent().unwrap();
    std::fs::create_dir_all(new_dir)?;

    // Copy config
    let bytes = std::fs::read(&old_config)?;
    let config: AppConfig = serde_json::from_slice(&bytes)?;
    std::fs::write(new_path, serde_json::to_vec_pretty(&config)?)?;

    // Copy history.db if not already present
    let old_db = old_dir.join("history.db");
    let new_db = new_dir.join("history.db");
    if old_db.exists() && !new_db.exists() {
        let _ = std::fs::copy(&old_db, &new_db);
    }

    // Copy models dir if not already present
    let old_models = old_dir.join("models");
    let new_models = new_dir.join("models");
    if old_models.exists() && !new_models.exists() {
        let _ = copy_dir_all(&old_models, &new_models);
    }

    tracing::info!("migrated app data from {:?} to {:?}", old_dir, new_dir);
    Ok(config)
}

#[cfg(target_os = "macos")]
fn copy_dir_all(src: &PathBuf, dst: &PathBuf) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_all(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), dst_path)?;
        }
    }
    Ok(())
}
