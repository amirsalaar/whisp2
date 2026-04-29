use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub text: String,
    pub source_app: Option<String>,
    pub provider: String,
    pub word_count: i64,
    pub char_count: i64,
    pub created_at: DateTime<Utc>,
}
