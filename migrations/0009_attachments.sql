-- Fichiers PDF rattachés à un épisode ou une scène (synopsis ou scénario déjà rédigé).
CREATE TABLE attachments (
    id       INTEGER PRIMARY KEY,
    node_id  INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    slot     TEXT NOT NULL CHECK (slot IN ('synopsis', 'script')),
    filename TEXT NOT NULL,
    size     INTEGER NOT NULL,
    added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    data     BLOB NOT NULL
);
CREATE INDEX idx_attachments_node ON attachments(node_id, slot);
