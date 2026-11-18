-- Historique des versions d'une fiche + recherche dans les champs détaillés.

ALTER TABLE nodes ADD COLUMN body TEXT NOT NULL DEFAULT '';   -- texte libre des champs détaillés

CREATE TABLE node_history (
    id       INTEGER PRIMARY KEY,
    node_id  INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    saved_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%d %H:%M:%S','now','localtime')),
    snapshot TEXT NOT NULL CHECK (json_valid(snapshot))
);
CREATE INDEX idx_history_node ON node_history(node_id, id);

-- L'index plein texte couvre désormais aussi `body`.
DROP TRIGGER trg_fts_ai;
DROP TRIGGER trg_fts_ad;
DROP TRIGGER trg_fts_au;
DROP TABLE search_index;
CREATE VIRTUAL TABLE search_index USING fts5(title, summary, notes, body, content='nodes', content_rowid='id');
INSERT INTO search_index(search_index) VALUES ('rebuild');

CREATE TRIGGER trg_fts_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO search_index(rowid,title,summary,notes,body) VALUES (NEW.id,NEW.title,NEW.summary,NEW.notes,NEW.body); END;
CREATE TRIGGER trg_fts_ad AFTER DELETE ON nodes BEGIN
    INSERT INTO search_index(search_index,rowid,title,summary,notes,body) VALUES('delete',OLD.id,OLD.title,OLD.summary,OLD.notes,OLD.body); END;
CREATE TRIGGER trg_fts_au AFTER UPDATE OF title,summary,notes,body ON nodes BEGIN
    INSERT INTO search_index(search_index,rowid,title,summary,notes,body) VALUES('delete',OLD.id,OLD.title,OLD.summary,OLD.notes,OLD.body);
    INSERT INTO search_index(rowid,title,summary,notes,body) VALUES (NEW.id,NEW.title,NEW.summary,NEW.notes,NEW.body); END;
