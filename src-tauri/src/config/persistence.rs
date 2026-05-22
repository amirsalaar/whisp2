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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::models::TranscriptionProvider;
    use std::sync::Mutex;

    // Tests in this module mutate the process-wide HOME env var. Serialize
    // them so they don't race each other.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    struct HomeGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous: Option<String>,
    }

    impl HomeGuard {
        fn new(new_home: &std::path::Path) -> Self {
            let lock = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let previous = std::env::var("HOME").ok();
            std::env::set_var("HOME", new_home);
            Self { _lock: lock, previous }
        }
    }

    impl Drop for HomeGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn app_support_dir_lives_under_com_whisp2_app() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _g = HomeGuard::new(tmp.path());

        let dir = app_support_dir().expect("app_support_dir");
        assert!(
            dir.ends_with("Library/Application Support/com.whisp2.app"),
            "unexpected dir: {dir:?}"
        );
        assert!(dir.exists(), "app_support_dir should create the directory");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn load_returns_default_when_no_config_or_old_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _g = HomeGuard::new(tmp.path());

        let cfg = load().expect("load");
        assert_eq!(cfg, AppConfig::default());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn save_then_load_round_trips_a_modified_config() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _g = HomeGuard::new(tmp.path());

        let mut cfg = AppConfig::default();
        cfg.provider = TranscriptionProvider::Groq;
        cfg.openai_model = "whisper-custom".into();
        cfg.play_completion_sound = false;

        save(&cfg).expect("save");
        let loaded = load().expect("load");
        assert_eq!(loaded, cfg);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn migrate_copies_config_db_and_models_from_old_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _g = HomeGuard::new(tmp.path());

        // Seed the old com.whisp.whisp-rs directory.
        let old_dir = tmp
            .path()
            .join("Library/Application Support/com.whisp.whisp-rs");
        std::fs::create_dir_all(&old_dir).unwrap();

        let mut original = AppConfig::default();
        original.gemini_model = "migrated-marker".into();
        std::fs::write(
            old_dir.join("config.json"),
            serde_json::to_vec_pretty(&original).unwrap(),
        )
        .unwrap();
        std::fs::write(old_dir.join("history.db"), b"db-bytes").unwrap();
        let old_models = old_dir.join("models");
        std::fs::create_dir_all(&old_models).unwrap();
        std::fs::write(old_models.join("ggml-tiny.bin"), b"model-bytes").unwrap();

        // load() should see no new config and migrate.
        let loaded = load().expect("load");
        assert_eq!(loaded, original);

        let new_dir = tmp
            .path()
            .join("Library/Application Support/com.whisp2.app");
        assert!(new_dir.join("config.json").exists());
        assert_eq!(
            std::fs::read(new_dir.join("history.db")).unwrap(),
            b"db-bytes"
        );
        assert_eq!(
            std::fs::read(new_dir.join("models/ggml-tiny.bin")).unwrap(),
            b"model-bytes"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn migrate_does_not_clobber_existing_db_or_models() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let _g = HomeGuard::new(tmp.path());

        let old_dir = tmp
            .path()
            .join("Library/Application Support/com.whisp.whisp-rs");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(
            old_dir.join("config.json"),
            serde_json::to_vec_pretty(&AppConfig::default()).unwrap(),
        )
        .unwrap();
        std::fs::write(old_dir.join("history.db"), b"old-db").unwrap();

        let new_dir = tmp
            .path()
            .join("Library/Application Support/com.whisp2.app");
        std::fs::create_dir_all(&new_dir).unwrap();
        std::fs::write(new_dir.join("history.db"), b"new-db").unwrap();

        let _ = load().expect("load");

        // The pre-existing new history.db must be untouched.
        assert_eq!(
            std::fs::read(new_dir.join("history.db")).unwrap(),
            b"new-db"
        );
    }
}
