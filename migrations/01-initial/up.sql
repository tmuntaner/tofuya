CREATE TABLE IF NOT EXISTS blobs
(
    id         INTEGER PRIMARY KEY,
    hash       TEXT UNIQUE NOT NULL,
    size       INTEGER     NOT NULL,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP
) STRICT;

CREATE TABLE IF NOT EXISTS tags
(
    id        INTEGER PRIMARY KEY,
    reference TEXT UNIQUE NOT NULL,
    blob_id   INTEGER     NOT NULL,
    FOREIGN KEY (blob_id) REFERENCES blobs (id) ON DELETE CASCADE
) STRICT;

CREATE INDEX IF NOT EXISTS idx_tags_blob_id ON tags (blob_id);
