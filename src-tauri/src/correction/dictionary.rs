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

fn apply_entries(text: &str, entries: &[SubEntry]) -> String {
    if entries.is_empty() {
        return text.to_string();
    }
    let mut result = format!(" {} ", text);
    for entry in entries {
        let from = format!(" {} ", entry.from);
        let to = format!(" {} ", entry.to);
        result = result.replace(&from, &to);
    }
    result.trim().to_string()
}

pub fn apply(text: String) -> String {
    let entries = load().unwrap_or_default();
    apply_entries(&text, &entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(from: &str, to: &str) -> SubEntry {
        SubEntry {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    #[test]
    fn test_apply_exact_match() {
        let entries = vec![entry("ok", "okay")];
        assert_eq!(apply_entries("ok", &entries), "okay");
    }

    #[test]
    fn test_apply_no_match() {
        let entries = vec![entry("ok", "okay")];
        assert_eq!(apply_entries("not a match", &entries), "not a match");
    }

    #[test]
    fn test_apply_word_boundary() {
        let entries = vec![entry("its", "it's")];
        assert_eq!(apply_entries("its itself", &entries), "it's itself");
    }

    #[test]
    fn test_apply_multiple_substitutions() {
        let entries = vec![entry("ok", "okay"), entry("ur", "your")];
        assert_eq!(apply_entries("ok ur problem", &entries), "okay your problem");
    }

    #[test]
    fn test_apply_empty_entries() {
        let entries: Vec<SubEntry> = vec![];
        assert_eq!(apply_entries("hello", &entries), "hello");
    }
}
