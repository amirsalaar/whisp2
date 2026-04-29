use anyhow::Result;
use directories::ProjectDirs;
use std::path::PathBuf;

use super::models::AppConfig;

fn config_path() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("com", "whisp", "whisp-rs")
        .ok_or_else(|| anyhow::anyhow!("cannot determine app data directory"))?;
    let config_dir = dirs.config_dir().to_path_buf();
    std::fs::create_dir_all(&config_dir)?;
    Ok(config_dir.join("config.json"))
}

pub fn load() -> Result<AppConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let bytes = std::fs::read(&path)?;
    let config: AppConfig = serde_json::from_slice(&bytes)?;
    Ok(config)
}

pub fn save(config: &AppConfig) -> Result<()> {
    let path = config_path()?;
    let bytes = serde_json::to_vec_pretty(config)?;
    std::fs::write(&path, bytes)?;
    Ok(())
}

pub fn app_support_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("com", "whisp", "whisp-rs")
        .ok_or_else(|| anyhow::anyhow!("cannot determine app data directory"))?;
    let dir = dirs.data_dir().to_path_buf();
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
