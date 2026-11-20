-- Éditeur de scénario : une scène est une suite ordonnée de blocs.
CREATE TABLE scene_blocks (
    id           INTEGER PRIMARY KEY,
    scene_id     INTEGER NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    position     INTEGER NOT NULL,
    block_type   TEXT NOT NULL CHECK (block_type IN ('action','dialogue','direction')),
    character_id INTEGER REFERENCES nodes(id) ON DELETE SET NULL,   -- qui parle (dialogue)
    text         TEXT NOT NULL DEFAULT '',
    UNIQUE (scene_id, position)
);
CREATE INDEX idx_blocks_scene ON scene_blocks(scene_id, position);
