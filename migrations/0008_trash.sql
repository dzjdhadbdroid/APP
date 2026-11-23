-- Corbeille : instantané JSON (même format que l'export) de chaque élément supprimé.
CREATE TABLE trash (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL,
    title      TEXT NOT NULL,
    deleted_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    payload    TEXT NOT NULL
);
