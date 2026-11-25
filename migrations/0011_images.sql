-- Une image (portrait, plan…) par fiche, réduite et stockée en PNG dans la base.
CREATE TABLE node_images (
    node_id  INTEGER PRIMARY KEY REFERENCES nodes(id) ON DELETE CASCADE,
    data     BLOB NOT NULL,
    added_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
