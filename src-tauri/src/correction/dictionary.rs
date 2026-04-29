use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubEntry {
    pub from: String,
    pub to: String,
}

/// Loads dictionary from app support dir. Returns empty vec if file missing.
pub fn load() -> Result<Vec<SubEntry>> {
    let path = crate::config::persistence::app_support_dir()?.join("dictionary.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data)?)
}

/// Saves dictionary to app support dir.
pub fn save(entries: &[SubEntry]) -> Result<()> {
    let path = crate::config::persistence::app_support_dir()?.join("dictionary.json");
    std::fs::write(&path, serde_json::to_string_pretty(entries)?)?;
    Ok(())
}

/// Applies dictionary substitutions to text. Matches whole words (space-padded).
pub fn apply(text: String) -> String {
    let entries = match load() {
        Ok(e) => e,
        Err(_) => return text,
    };
    if entries.is_empty() {
        return text;
    }
    let mut result = format!(" {} ", text);
    for entry in &entries {
        let from = format!(" {} ", entry.from);
        let to = format!(" {} ", entry.to);
        result = result.replace(&from, &to);
    }
    result.trim().to_string()
}
