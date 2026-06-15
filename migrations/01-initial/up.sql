CREATE TABLE IF NOT EXISTS blobs
(
    hash       TEXT PRIMARY KEY,
    size       INTEGER NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS tags
(
    reference TEXT PRIMARY KEY,
    blob_hash TEXT NOT NULL,
    FOREIGN KEY (blob_hash) REFERENCES blobs (hash)
);
