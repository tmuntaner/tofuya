use crate::domain::tofu::ports::{DatabaseError, DatabasePort};
use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension};
use rusqlite_migration::Migrations;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

#[derive(Clone)]
pub struct DB {
    db: Arc<Mutex<Connection>>,
}

impl DB {
    pub fn new(
        path: PathBuf,
        migrations: &LazyLock<Migrations<'static>>,
    ) -> Result<Self, DatabaseError> {
        let mut db = Connection::open(path)?;
        migrations.to_latest(&mut db)?;
        db.pragma_update(None, "foreign_keys", "ON")?;

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
        })
    }
}

#[async_trait]
impl DatabasePort for DB {
    async fn save(&self, reference: String, size: i64, hash: String) -> Result<(), DatabaseError> {
        let db = self.db.clone();

        tokio::task::spawn_blocking(move || -> Result<(), DatabaseError> {
            let mut conn = db.lock().map_err(|_| DatabaseError::LockError)?;
            let tx = conn.transaction()?;

            let blob_id: i64 = tx.query_row(
                "INSERT INTO blobs (hash, size) VALUES (?1, ?2)
                 ON CONFLICT(hash) DO UPDATE SET size = excluded.size
                 RETURNING id",
                rusqlite::params![hash, size],
                |row| row.get(0),
            )?;

            tx.execute(
                "INSERT INTO tags (reference, blob_id) VALUES (?1, ?2)
                 ON CONFLICT(reference) DO UPDATE SET blob_id = excluded.blob_id",
                rusqlite::params![reference, blob_id],
            )?;

            tx.commit()?;

            Ok(())
        })
        .await??;

        Ok(())
    }

    async fn retrieve(&self, reference: String) -> Result<Option<String>, DatabaseError> {
        let db = self.db.clone();

        let result =
            tokio::task::spawn_blocking(move || -> Result<Option<String>, DatabaseError> {
                let conn = db.lock().map_err(|_| DatabaseError::LockError)?;

                let result: Option<String> = conn
                    .query_row(
                        "SELECT b.hash
                         FROM tags t
                         JOIN blobs b ON t.blob_id = b.id
                         WHERE t.reference = ?1",
                        rusqlite::params![reference],
                        |row| row.get(0),
                    )
                    .optional()?;

                Ok(result)
            })
            .await??;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use include_dir::{Dir, include_dir};
    use test_temp_dir::test_temp_dir;

    static MIGRATIONS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/migrations");

    // Define migrations. These are applied atomically.
    static MIGRATIONS: LazyLock<Migrations<'static>> =
        LazyLock::new(|| Migrations::from_directory(&MIGRATIONS_DIR).unwrap());

    #[tokio::test]
    async fn test_pull() {
        let reference = String::from("ghcr.io/tmuntnaer/tofuya:latest");

        let dir = test_temp_dir!();
        let db_path = dir.as_path_untracked().to_path_buf().join("metadata.db");
        let db = DB::new(db_path, &MIGRATIONS).unwrap();

        let result = db.retrieve(reference.clone()).await.unwrap();
        assert_eq!(None, result);

        let result = db
            .save(reference.clone(), 100, String::from("foobar"))
            .await;
        assert!(!result.is_err());

        let result = db.retrieve(reference).await.unwrap().unwrap_or_default();
        assert_eq!(String::from("foobar"), result);
    }
}
