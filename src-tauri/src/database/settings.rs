//! Key/value settings persisted as JSON.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::Database;
use crate::error::AppResult;

pub async fn get<T: DeserializeOwned>(db: &Database, key: &str) -> AppResult<Option<T>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(db.pool()).await?;
    // A value that no longer matches its type (e.g. after a format change) is
    // treated as unset rather than an error, so the caller's default applies.
    Ok(row.and_then(|(json,)| serde_json::from_str(&json).ok()))
}

pub async fn set<T: Serialize>(db: &Database, key: &str, value: &T) -> AppResult<()> {
    let json = serde_json::to_string(value).expect("settings values serialise to JSON");
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = unixepoch()",
    )
    .bind(key)
    .bind(json)
    .execute(db.pool())
    .await?;
    Ok(())
}

/// All settings whose key starts with `prefix`, keyed by the remainder.
pub async fn get_prefixed(db: &Database, prefix: &str) -> AppResult<serde_json::Map<String, Value>> {
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT key, value FROM settings WHERE key LIKE ? || '%'")
        .bind(prefix)
        .fetch_all(db.pool())
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(k, v)| Some((k.strip_prefix(prefix)?.to_owned(), serde_json::from_str(&v).ok()?)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trips_and_overwrites_values() {
        let db = Database::in_memory().await.unwrap();
        assert_eq!(get::<f64>(&db, "player.volume").await.unwrap(), None);

        set(&db, "player.volume", &0.5).await.unwrap();
        set(&db, "player.volume", &0.75).await.unwrap();
        assert_eq!(get::<f64>(&db, "player.volume").await.unwrap(), Some(0.75));

        set(&db, "ui.theme", &"violet").await.unwrap();
        let ui = get_prefixed(&db, "ui.").await.unwrap();
        assert_eq!(ui.get("theme"), Some(&Value::from("violet")));
        assert!(!ui.contains_key("player.volume"));
    }

    #[tokio::test]
    async fn mismatched_types_read_as_unset() {
        let db = Database::in_memory().await.unwrap();
        set(&db, "player.volume", &"loud").await.unwrap();
        assert_eq!(get::<f64>(&db, "player.volume").await.unwrap(), None);
    }
}
