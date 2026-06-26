use anyhow::Result;
use regex::{NoExpand, Regex};
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

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn apply_entries(text: &str, entries: &[SubEntry]) -> String {
    let mut result = text.to_string();
    for entry in entries {
        // Real transcripts arrive capitalized and punctuated ("Whisp rs."),
        // so match case-insensitively on whole-word boundaries rather than by
        // padding with literal spaces. `NoExpand` keeps `$`/`\` in the
        // replacement literal (so "5 dollars" → "$5" doesn't try to expand a
        // capture group). The replacement text is used verbatim, so the stored
        // casing wins — that's the point of a substitution dictionary.
        //
        // Entries are applied in order over the running result, so a later
        // entry can rewrite an earlier entry's output (see
        // test_apply_entries_cascade) — that ordered behavior is intentional.
        let trimmed = entry.from.trim();
        if trimmed.is_empty() {
            continue;
        }
        // `\b` only fires between a word and a non-word char, so anchoring both
        // ends unconditionally would make a key whose own endpoint is a symbol
        // (".net", "C++") never match. Anchor an end only when the key's own
        // char there is a word char; otherwise the symbol itself is the
        // boundary.
        let lead = if trimmed.starts_with(is_word_char) { r"\b" } else { "" };
        let trail = if trimmed.ends_with(is_word_char) { r"\b" } else { "" };
        let pattern = format!("(?i){lead}{}{trail}", regex::escape(trimmed));
        let Ok(re) = Regex::new(&pattern) else {
            continue;
        };
        result = re.replace_all(&result, NoExpand(&entry.to)).into_owned();
    }
    result
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

    // Real transcripts arrive capitalized; the match must be case-insensitive
    // while the replacement keeps the stored canonical casing.
    #[test]
    fn test_apply_case_insensitive_match() {
        let entries = vec![entry("whisp rs", "whisp-rs")];
        assert_eq!(apply_entries("Whisp rs", &entries), "whisp-rs");
        assert_eq!(apply_entries("Ok then", &[entry("ok", "okay")]), "okay then");
    }

    // Trailing/leading punctuation must not defeat the boundary match.
    #[test]
    fn test_apply_with_punctuation() {
        let entries = vec![entry("whisp rs", "whisp-rs")];
        assert_eq!(apply_entries("I love whisp rs.", &entries), "I love whisp-rs.");
        assert_eq!(apply_entries("ok, sure", &[entry("ok", "okay")]), "okay, sure");
    }

    // Adjacent repeats: the old space-padding hack consumed the shared space
    // and missed the second occurrence.
    #[test]
    fn test_apply_adjacent_repeats() {
        let entries = vec![entry("ok", "okay")];
        assert_eq!(apply_entries("ok ok", &entries), "okay okay");
    }

    // A `from` must only match whole words, never a substring.
    #[test]
    fn test_apply_no_substring_match() {
        let entries = vec![entry("rs", "RS")];
        assert_eq!(apply_entries("rstuff stays", &entries), "rstuff stays");
        assert_eq!(apply_entries("the rs file", &entries), "the RS file");
    }

    // Replacement text is literal: `$`/`\` must not trigger regex expansion.
    #[test]
    fn test_apply_literal_replacement() {
        let entries = vec![entry("five dollars", "$5")];
        assert_eq!(apply_entries("cost five dollars", &entries), "cost $5");
    }

    // A blank `from` entry is skipped, not applied as a match-everything rule.
    #[test]
    fn test_apply_skips_blank_from() {
        let entries = vec![entry("  ", "x"), entry("ok", "okay")];
        assert_eq!(apply_entries("ok", &entries), "okay");
    }

    // Keys whose own endpoint is a symbol (".net", "c++") must still match —
    // \b can't anchor next to a non-word char, so those ends go unanchored.
    #[test]
    fn test_apply_non_word_endpoint_keys() {
        assert_eq!(apply_entries("i use c++", &[entry("c++", "C++")]), "i use C++");
        assert_eq!(apply_entries("love .net", &[entry(".net", ".NET")]), "love .NET");
        // The unanchored side must not over-match into a larger token.
        assert_eq!(apply_entries("gcc", &[entry("c++", "C++")]), "gcc");
        // A word-char endpoint still gets a boundary (no substring match).
        assert_eq!(apply_entries("rstuff", &[entry("rs", "RS")]), "rstuff");
    }

    // Entries apply in order over the running result, so one entry can rewrite
    // a previous entry's output. This cascade is intentional, not a bug.
    #[test]
    fn test_apply_entries_cascade() {
        let entries = vec![entry("foo", "bar"), entry("bar", "baz")];
        assert_eq!(apply_entries("foo", &entries), "baz");
    }

    // End-to-end: the public apply() reads dictionary.json from the app support
    // dir, so exercise the real save -> load -> apply path the transcription
    // pipeline uses. This is the path that was silently no-op'ing before the
    // fix; the apply_entries tests above only cover the in-memory matcher.
    #[test]
    #[cfg(target_os = "macos")]
    fn test_apply_reads_from_disk_and_substitutes() {
        // apply() -> load() resolves dictionary.json under app_support_dir(),
        // which is keyed off HOME; the shared guard serializes the env mutation
        // against every other HOME-mutating test in the binary.
        let tmp = tempfile::tempdir().expect("tempdir");
        let _guard = crate::test_support::HomeGuard::new(tmp.path());

        save(&[entry("whisp rs", "whisp-rs")]).expect("save dictionary");

        // Capitalized + punctuated, exactly like a real transcript.
        let out = apply("I love Whisp rs.".to_string());

        assert_eq!(out, "I love whisp-rs.");
    }
}
