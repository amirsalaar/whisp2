use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use uuid::Uuid;

use super::models::HistoryEntry;

pub async fn create_schema(pool: &SqlitePool) -> Result<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS history (
            id          TEXT PRIMARY KEY,
            text        TEXT NOT NULL,
            source_app  TEXT,
            provider    TEXT NOT NULL,
            word_count  INTEGER NOT NULL DEFAULT 0,
            char_count  INTEGER NOT NULL DEFAULT 0,
            created_at  TEXT NOT NULL
        )
        "#,
    )
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn insert(
    pool: &SqlitePool,
    text: &str,
    source_app: Option<&str>,
    provider: &str,
) -> Result<HistoryEntry> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let word_count = text.split_whitespace().count() as i64;
    let char_count = text.chars().count() as i64;
    let created_at = now.to_rfc3339();

    sqlx::query(
        r#"
        INSERT INTO history (id, text, source_app, provider, word_count, char_count, created_at)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(&id)
    .bind(text)
    .bind(source_app)
    .bind(provider)
    .bind(word_count)
    .bind(char_count)
    .bind(&created_at)
    .execute(pool)
    .await?;

    Ok(HistoryEntry {
        id,
        text: text.to_string(),
        source_app: source_app.map(String::from),
        provider: provider.to_string(),
        word_count,
        char_count,
        created_at: now,
    })
}

pub async fn list(pool: &SqlitePool, limit: i64) -> Result<Vec<HistoryEntry>> {
    // Use runtime query_as (not the macro) to avoid DATABASE_URL at compile time.
    let rows: Vec<HistoryRow> = sqlx::query_as(
        r#"
        SELECT id, text, source_app, provider, word_count, char_count, created_at
        FROM history
        ORDER BY created_at DESC
        LIMIT ?
        "#,
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;

    rows.into_iter().map(|r| {
        let created_at = chrono::DateTime::parse_from_rfc3339(&r.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        Ok(HistoryEntry {
            id: r.id,
            text: r.text,
            source_app: r.source_app,
            provider: r.provider,
            word_count: r.word_count,
            char_count: r.char_count,
            created_at,
        })
    }).collect()
}

#[derive(sqlx::FromRow)]
struct HistoryRow {
    id: String,
    text: String,
    source_app: Option<String>,
    provider: String,
    word_count: i64,
    char_count: i64,
    created_at: String,
}

pub async fn delete(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM history WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_all(pool: &SqlitePool) -> Result<()> {
    sqlx::query("DELETE FROM history").execute(pool).await?;
    Ok(())
}
