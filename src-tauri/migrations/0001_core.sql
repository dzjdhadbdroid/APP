-- Sitcom Studio — schéma noyau (source de données unique)
-- Principe : une table `nodes` = identité de TOUT élément de l'univers.
-- Les tables de détail partagent son id. Une seule table `links` porte
-- toutes les connexions : Universe Map, Timeline et fiches lisent la même source.

PRAGMA foreign_keys = ON;

CREATE TABLE schema_version (version INTEGER NOT NULL);
INSERT INTO schema_version VALUES (1);

-- ───────── Identité commune ─────────
CREATE TABLE nodes (
    id          INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL CHECK (kind IN
                ('character','place','episode','plot','object','event','scene')),
    title       TEXT NOT NULL,
    summary     TEXT NOT NULL DEFAULT '',
    notes       TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE (id, kind)
);
CREATE INDEX idx_nodes_kind ON nodes(kind);
CREATE INDEX idx_nodes_title ON nodes(title);

-- ───────── Tables de détail ─────────
CREATE TABLE characters (
    id INTEGER PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'character' CHECK (kind='character'),
    first_name TEXT, last_name TEXT, nickname TEXT, birth_date TEXT, profession TEXT,
    physical TEXT, style TEXT, personality TEXT, biography TEXT,
    fears TEXT, desires TEXT, secrets TEXT, verbal_tics TEXT,
    initial_situation TEXT, long_term_goals TEXT,
    FOREIGN KEY (id, kind) REFERENCES nodes(id, kind) ON DELETE CASCADE
);

CREATE TABLE places (
    id INTEGER PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'place' CHECK (kind='place'),
    place_type TEXT, address TEXT, ambiance TEXT, role_in_show TEXT, history TEXT,
    parent_id INTEGER REFERENCES places(id) ON DELETE SET NULL,   -- sous-lieux
    FOREIGN KEY (id, kind) REFERENCES nodes(id, kind) ON DELETE CASCADE
);

CREATE TABLE episodes (
    id INTEGER PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'episode' CHECK (kind='episode'),
    season INTEGER NOT NULL, number INTEGER NOT NULL,
    synopsis TEXT, theme TEXT, status TEXT NOT NULL DEFAULT 'idea'
        CHECK (status IN ('idea','outline','drafting','written','final')),
    air_order INTEGER,                      -- ordre de diffusion
    production_notes TEXT,
    UNIQUE (season, number),
    FOREIGN KEY (id, kind) REFERENCES nodes(id, kind) ON DELETE CASCADE
);

CREATE TABLE plots (
    id INTEGER PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'plot' CHECK (kind='plot'),
    plot_type TEXT NOT NULL DEFAULT 'main' CHECK (plot_type IN
        ('main','secondary','running_gag','character_arc','secret','recurring_conflict')),
    status TEXT NOT NULL DEFAULT 'planned'
        CHECK (status IN ('planned','active','resolved','abandoned')),
    planned_resolution TEXT,
    FOREIGN KEY (id, kind) REFERENCES nodes(id, kind) ON DELETE CASCADE
);

CREATE TABLE objects (
    id INTEGER PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'object' CHECK (kind='object'),
    description TEXT,
    FOREIGN KEY (id, kind) REFERENCES nodes(id, kind) ON DELETE CASCADE
);

-- Scène : ordre de PRÉSENTATION = position dans l'épisode
CREATE TABLE scenes (
    id INTEGER PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'scene' CHECK (kind='scene'),
    episode_id INTEGER NOT NULL REFERENCES episodes(id) ON DELETE CASCADE,
    act INTEGER, sequence INTEGER,
    position INTEGER NOT NULL,
    content TEXT NOT NULL DEFAULT '',       -- actions, dialogues, mise en scène
    UNIQUE (episode_id, position),
    FOREIGN KEY (id, kind) REFERENCES nodes(id, kind) ON DELETE CASCADE
);

-- Événement : chronologie RÉELLE de l'univers fictif
CREATE TABLE events (
    id INTEGER PRIMARY KEY, kind TEXT NOT NULL DEFAULT 'event' CHECK (kind='event'),
    story_start TEXT,                       -- ISO partiel : 1998 / 1998-06 / 1998-06-12
    story_end   TEXT,
    story_sort  INTEGER,                    -- clé numérique de tri (jours depuis l'époque fictive)
    precision   TEXT NOT NULL DEFAULT 'day' CHECK (precision IN ('year','month','day','period')),
    importance  INTEGER NOT NULL DEFAULT 3 CHECK (importance BETWEEN 1 AND 5),
    status      TEXT NOT NULL DEFAULT 'planned' CHECK (status IN ('planned','confirmed','modified')),
    FOREIGN KEY (id, kind) REFERENCES nodes(id, kind) ON DELETE CASCADE
);
CREATE INDEX idx_events_sort ON events(story_sort);

-- ───────── Connexions typées (Universe Map + Timeline) ─────────
CREATE TABLE links (
    id          INTEGER PRIMARY KEY,
    src_id      INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    dst_id      INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    link_type   TEXT NOT NULL,   -- ex: relation, lives_in, appears_in, contains, set_in,
                                 --     advances, owns, involves, located_at, mentions
    label       TEXT,            -- ex: "frère", "rival"
    closeness   INTEGER CHECK (closeness BETWEEN 0 AND 10),
    description TEXT,
    meta        TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(meta)),
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    CHECK (src_id <> dst_id),
    UNIQUE (src_id, dst_id, link_type)
);
CREATE INDEX idx_links_src ON links(src_id);
CREATE INDEX idx_links_dst ON links(dst_id);
CREATE INDEX idx_links_type ON links(link_type);

-- Historique/évolution d'un lien (relation qui évolue dans le temps)
CREATE TABLE link_history (
    id INTEGER PRIMARY KEY,
    link_id INTEGER NOT NULL REFERENCES links(id) ON DELETE CASCADE,
    event_id INTEGER REFERENCES nodes(id) ON DELETE SET NULL,
    note TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);

-- ───────── Connaissance des personnages (détection d'incohérences) ─────────
-- "Le personnage X apprend l'information/événement Y à l'instant Z"
CREATE TABLE knowledge (
    id INTEGER PRIMARY KEY,
    character_id INTEGER NOT NULL REFERENCES characters(id) ON DELETE CASCADE,
    subject_id   INTEGER NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,  -- événement, secret, personnage rencontré…
    learned_event_id INTEGER REFERENCES events(id) ON DELETE SET NULL,
    learned_scene_id INTEGER REFERENCES scenes(id) ON DELETE SET NULL,
    UNIQUE (character_id, subject_id)
);

-- ───────── Alertes de cohérence ─────────
CREATE TABLE alerts (
    id INTEGER PRIMARY KEY,
    code TEXT NOT NULL,                      -- ex: EVENT_BEFORE_MEETING, ISOLATED_NODE
    severity TEXT NOT NULL CHECK (severity IN ('info','warning','error')),
    message TEXT NOT NULL,
    node_id INTEGER REFERENCES nodes(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open','ignored','resolved')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE (code, node_id, message)
);

-- ───────── Vues : lecture commune pour Map et Timeline ─────────
-- Graphe : arêtes non orientées pour l'exploration
CREATE VIEW v_graph_edges AS
    SELECT id, src_id, dst_id, link_type, label FROM links;

-- Éléments isolés (aucune connexion)
CREATE VIEW v_isolated_nodes AS
    SELECT n.* FROM nodes n
    WHERE NOT EXISTS (SELECT 1 FROM links l WHERE l.src_id = n.id OR l.dst_id = n.id);

-- Chronologie : trois ordres distincts, une seule source
CREATE VIEW v_timeline_story AS      -- ordre réel de l'univers
    SELECT n.id, n.title, e.story_start, e.story_end, e.story_sort, e.importance, e.status
    FROM events e JOIN nodes n ON n.id = e.id ORDER BY e.story_sort;
CREATE VIEW v_timeline_air AS        -- ordre de diffusion
    SELECT n.id, n.title, ep.season, ep.number, ep.air_order
    FROM episodes ep JOIN nodes n ON n.id = ep.id ORDER BY ep.air_order, ep.season, ep.number;
CREATE VIEW v_timeline_scenes AS     -- ordre de présentation
    SELECT n.id, n.title, s.episode_id, s.position
    FROM scenes s JOIN nodes n ON n.id = s.id ORDER BY s.episode_id, s.position;

-- ───────── Triggers : updated_at ─────────
CREATE TRIGGER trg_nodes_touch AFTER UPDATE ON nodes
BEGIN UPDATE nodes SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = NEW.id; END;

-- ───────── Recherche globale (FTS5) ─────────
CREATE VIRTUAL TABLE search_index USING fts5(title, summary, notes, content='nodes', content_rowid='id');
CREATE TRIGGER trg_fts_ai AFTER INSERT ON nodes BEGIN
    INSERT INTO search_index(rowid,title,summary,notes) VALUES (NEW.id,NEW.title,NEW.summary,NEW.notes); END;
CREATE TRIGGER trg_fts_ad AFTER DELETE ON nodes BEGIN
    INSERT INTO search_index(search_index,rowid,title,summary,notes) VALUES('delete',OLD.id,OLD.title,OLD.summary,OLD.notes); END;
CREATE TRIGGER trg_fts_au AFTER UPDATE OF title,summary,notes ON nodes BEGIN
    INSERT INTO search_index(search_index,rowid,title,summary,notes) VALUES('delete',OLD.id,OLD.title,OLD.summary,OLD.notes);
    INSERT INTO search_index(rowid,title,summary,notes) VALUES (NEW.id,NEW.title,NEW.summary,NEW.notes); END;
