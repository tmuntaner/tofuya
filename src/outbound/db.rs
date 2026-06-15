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
            let conn = db.lock().map_err(|_| DatabaseError::LockError)?;

            conn.execute(
                "INSERT INTO blobs (hash, size) VALUES (?1, ?2)",
                rusqlite::params![hash, size],
            )?;

            conn.execute(
                "INSERT INTO tags (reference, blob_hash) VALUES (?1, ?2)
                 ON CONFLICT(reference) DO UPDATE SET blob_hash = excluded.blob_hash",
                rusqlite::params![reference, hash],
            )?;

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
                        "SELECT blob_hash FROM tags WHERE reference = ?1",
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
