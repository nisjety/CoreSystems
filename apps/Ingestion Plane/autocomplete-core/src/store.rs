use std::{
    collections::HashSet,
    path::Path,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexedObject {
    pub collection: String,
    pub bucket: String,
    pub object: String,
    pub text: String,
    pub source: String,
    pub target_url: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct MetadataStore {
    connection: Arc<Mutex<Connection>>,
}

impl MetadataStore {
    pub fn open(path: impl AsRef<Path>) -> AppResult<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> AppResult<Self> {
        let connection = Connection::open_in_memory()?;
        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> AppResult<()> {
        let connection = self.lock()?;
        connection.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;

            CREATE TABLE IF NOT EXISTS autocomplete_objects (
                collection TEXT NOT NULL,
                bucket TEXT NOT NULL,
                object TEXT NOT NULL,
                text TEXT NOT NULL,
                source TEXT NOT NULL,
                target_url TEXT,
                metadata_json TEXT NOT NULL DEFAULT '{}',
                hit_count INTEGER NOT NULL DEFAULT 1,
                last_seen_at INTEGER NOT NULL,
                PRIMARY KEY (collection, bucket, object)
            );

            CREATE INDEX IF NOT EXISTS autocomplete_objects_prefix_idx
                ON autocomplete_objects(collection, bucket, source, text);
            "#,
        )?;
        Ok(())
    }

    pub async fn upsert(&self, item: IndexedObject) -> AppResult<()> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.upsert_blocking(item)).await?
    }

    fn upsert_blocking(&self, item: IndexedObject) -> AppResult<()> {
        let metadata_json = serde_json::to_string(&item.metadata)?;
        let now = now_unix_seconds();
        let connection = self.lock()?;
        connection.execute(
            r#"
            INSERT INTO autocomplete_objects (
                collection, bucket, object, text, source, target_url,
                metadata_json, hit_count, last_seen_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8)
            ON CONFLICT(collection, bucket, object) DO UPDATE SET
                text = excluded.text,
                source = excluded.source,
                target_url = excluded.target_url,
                metadata_json = excluded.metadata_json,
                hit_count = autocomplete_objects.hit_count + 1,
                last_seen_at = excluded.last_seen_at
            "#,
            params![
                item.collection,
                item.bucket,
                item.object,
                item.text,
                item.source,
                item.target_url,
                metadata_json,
                now
            ],
        )?;
        Ok(())
    }

    pub async fn find_by_objects(
        &self,
        collection: String,
        bucket: String,
        objects: Vec<String>,
    ) -> AppResult<Vec<IndexedObject>> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            store.find_by_objects_blocking(&collection, &bucket, &objects)
        })
        .await?
    }

    fn find_by_objects_blocking(
        &self,
        collection: &str,
        bucket: &str,
        objects: &[String],
    ) -> AppResult<Vec<IndexedObject>> {
        let connection = self.lock()?;
        let mut output = Vec::new();

        for object in objects {
            let item = query_item(&connection, collection, bucket, object)?;
            if let Some(item) = item {
                output.push(item);
            }
        }

        Ok(output)
    }

    pub async fn search_prefix(
        &self,
        collection: String,
        bucket: String,
        query: String,
        limit: usize,
    ) -> AppResult<Vec<IndexedObject>> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            store.search_prefix_blocking(&collection, &bucket, &query, limit)
        })
        .await?
    }

    fn search_prefix_blocking(
        &self,
        collection: &str,
        bucket: &str,
        query: &str,
        limit: usize,
    ) -> AppResult<Vec<IndexedObject>> {
        let prefix = format!("{query}%");
        let contains = format!("%{query}%");
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            r#"
            SELECT collection, bucket, object, text, source, target_url, metadata_json
            FROM autocomplete_objects
            WHERE collection = ?1
              AND bucket = ?2
              AND (text LIKE ?3 OR text LIKE ?4)
            ORDER BY
              CASE WHEN text LIKE ?3 THEN 0 ELSE 1 END,
              hit_count DESC,
              last_seen_at DESC
            LIMIT ?5
            "#,
        )?;

        let rows = statement.query_map(
            params![collection, bucket, prefix, contains, limit as i64],
            row_to_item,
        )?;

        let mut output = Vec::new();
        let mut seen = HashSet::new();
        for item in rows {
            let item = item?;
            let dedupe_key = item.text.to_ascii_lowercase();
            if seen.insert(dedupe_key) {
                output.push(item);
            }
        }
        Ok(output)
    }

    fn lock(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| AppError::Config("metadata store lock poisoned".to_string()))
    }
}

fn query_item(
    connection: &Connection,
    collection: &str,
    bucket: &str,
    object: &str,
) -> AppResult<Option<IndexedObject>> {
    let mut statement = connection.prepare(
        r#"
        SELECT collection, bucket, object, text, source, target_url, metadata_json
        FROM autocomplete_objects
        WHERE collection = ?1 AND bucket = ?2 AND object = ?3
        "#,
    )?;

    statement
        .query_row(params![collection, bucket, object], row_to_item)
        .optional()
        .map_err(AppError::from)
}

fn row_to_item(row: &rusqlite::Row<'_>) -> rusqlite::Result<IndexedObject> {
    let metadata_json: String = row.get(6)?;
    Ok(IndexedObject {
        collection: row.get(0)?,
        bucket: row.get(1)?,
        object: row.get(2)?,
        text: row.get(3)?,
        source: row.get(4)?,
        target_url: row.get(5)?,
        metadata: serde_json::from_str(&metadata_json).unwrap_or_else(|_| serde_json::json!({})),
    })
}

fn now_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn stores_and_searches_suggestions() {
        let store = MetadataStore::in_memory().unwrap();
        store
            .upsert(IndexedObject {
                collection: "queries".to_string(),
                bucket: "org_a".to_string(),
                object: "query:1".to_string(),
                text: "Find me restaurants".to_string(),
                source: "query".to_string(),
                target_url: None,
                metadata: json!({"provider": "brave"}),
            })
            .await
            .unwrap();

        let found = store
            .search_prefix(
                "queries".to_string(),
                "org_a".to_string(),
                "Find me".to_string(),
                10,
            )
            .await
            .unwrap();

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "Find me restaurants");
    }
}
