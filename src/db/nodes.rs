//! Dépôt des nœuds : identité commune + ligne de détail du type concerné.

use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use super::{Atomic, DbError, DbResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Character,
    Place,
    Episode,
    Plot,
    Object,
    Event,
    Scene,
}

impl NodeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Character => "character",
            Self::Place => "place",
            Self::Episode => "episode",
            Self::Plot => "plot",
            Self::Object => "object",
            Self::Event => "event",
            Self::Scene => "scene",
        }
    }

    pub fn parse(s: &str) -> DbResult<Self> {
        Ok(match s {
            "character" => Self::Character,
            "place" => Self::Place,
            "episode" => Self::Episode,
            "plot" => Self::Plot,
            "object" => Self::Object,
            "event" => Self::Event,
            "scene" => Self::Scene,
            other => return Err(DbError::UnknownKind(other.to_string())),
        })
    }

    pub const ALL: [NodeKind; 7] = [
        Self::Character,
        Self::Place,
        Self::Episode,
        Self::Plot,
        Self::Object,
        Self::Event,
        Self::Scene,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Character => "Personnage",
            Self::Place => "Lieu",
            Self::Episode => "Épisode",
            Self::Plot => "Intrigue",
            Self::Object => "Objet",
            Self::Event => "Événement",
            Self::Scene => "Scène",
        }
    }

    pub fn label_plural(self) -> &'static str {
        match self {
            Self::Character => "Personnages",
            Self::Place => "Lieux",
            Self::Episode => "Épisodes",
            Self::Plot => "Intrigues",
            Self::Object => "Objets",
            Self::Event => "Événements",
            Self::Scene => "Scènes",
        }
    }

    /// Nom de la table de détail (valeurs fixes : jamais issues de l'utilisateur).
    pub fn detail_table(self) -> &'static str {
        match self {
            Self::Character => "characters",
            Self::Place => "places",
            Self::Episode => "episodes",
            Self::Plot => "plots",
            Self::Object => "objects",
            Self::Event => "events",
            Self::Scene => "scenes",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Node {
    pub id: i64,
    pub kind: NodeKind,
    pub title: String,
    pub summary: String,
    pub notes: String,
    pub updated_at: String,
}

fn from_row(row: &Row) -> rusqlite::Result<Node> {
    let kind: String = row.get("kind")?;
    Ok(Node {
        id: row.get("id")?,
        kind: NodeKind::parse(&kind).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())),
            )
        })?,
        title: row.get("title")?,
        summary: row.get("summary")?,
        notes: row.get("notes")?,
        updated_at: row.get("updated_at")?,
    })
}

const COLS: &str = "id, kind, title, summary, notes, updated_at";

/// Crée un nœud de type simple (personnage, lieu, intrigue, objet, événement).
/// Les épisodes et scènes ont des champs obligatoires : voir `create_episode` / `create_scene`.
pub fn create(conn: &Connection, kind: NodeKind, title: &str) -> DbResult<i64> {
    if matches!(kind, NodeKind::Episode | NodeKind::Scene) {
        return Err(DbError::UnknownKind(format!(
            "{} : utiliser create_episode / create_scene",
            kind.as_str()
        )));
    }
    let tx = Atomic::begin(conn)?;
    let id = insert_node(&tx, kind, title)?;
    tx.execute(
        &format!("INSERT INTO {} (id) VALUES (?1)", kind.detail_table()),
        [id],
    )?;
    tx.commit()?;
    Ok(id)
}

/// Crée un épisode ; l'ordre de diffusion initial est « à la suite » des existants.
pub fn create_episode(conn: &Connection, title: &str, season: i64, number: i64) -> DbResult<i64> {
    let tx = Atomic::begin(conn)?;
    let id = insert_node(&tx, NodeKind::Episode, title)?;
    tx.execute(
        "INSERT INTO episodes (id, season, number, air_order)
         VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(air_order), 0) + 1 FROM episodes))",
        params![id, season, number],
    )?;
    tx.commit()?;
    Ok(id)
}

/// Prochain numéro d'épisode libre dans une saison.
pub fn next_episode_number(conn: &Connection, season: i64) -> DbResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM episodes WHERE season = ?1",
        [season],
        |r| r.get(0),
    )?)
}

/// Prochaine position libre pour une scène d'un épisode.
pub fn next_scene_position(conn: &Connection, episode_id: i64) -> DbResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM scenes WHERE episode_id = ?1",
        [episode_id],
        |r| r.get(0),
    )?)
}

/// Tous les nœuds (pour la carte de l'univers).
pub fn all(conn: &Connection) -> DbResult<Vec<Node>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM nodes ORDER BY id"))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Nombre de nœuds par type.
pub fn counts(conn: &Connection) -> DbResult<Vec<(NodeKind, i64)>> {
    let mut stmt = conn.prepare("SELECT kind, COUNT(*) FROM nodes GROUP BY kind")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = Vec::new();
    for row in rows {
        let (k, n) = row?;
        out.push((NodeKind::parse(&k)?, n));
    }
    Ok(out)
}

/// Derniers éléments modifiés.
pub fn recent(conn: &Connection, limit: i64) -> DbResult<Vec<Node>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM nodes ORDER BY updated_at DESC, id DESC LIMIT ?1"
    ))?;
    let rows = stmt.query_map([limit], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Personnage secondaire ? (faux pour tout autre élément)
pub fn is_secondary(conn: &Connection, id: i64) -> DbResult<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM characters WHERE id = ?1 AND role = 'secondary'",
        [id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

pub fn secondary_ids(conn: &Connection) -> DbResult<std::collections::HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM characters WHERE role = 'secondary'")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn set_secondary(conn: &Connection, id: i64, secondary: bool) -> DbResult<()> {
    let n = conn.execute(
        "UPDATE characters SET role = ?2 WHERE id = ?1",
        params![id, if secondary { "secondary" } else { "main" }],
    )?;
    if n == 0 {
        return Err(DbError::NotFound(id));
    }
    Ok(())
}

/// Épisodes dont le statut est « plan » ou « en écriture », dans l'ordre de diffusion.
pub fn episodes_in_progress(conn: &Connection) -> DbResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title FROM episodes ep JOIN nodes n ON n.id = ep.id
         WHERE ep.status IN ('outline', 'drafting')
         ORDER BY ep.air_order IS NULL, ep.air_order, ep.season, ep.number",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn create_scene(
    conn: &Connection,
    episode_id: i64,
    title: &str,
    position: i64,
) -> DbResult<i64> {
    let tx = Atomic::begin(conn)?;
    let id = insert_node(&tx, NodeKind::Scene, title)?;
    tx.execute(
        "INSERT INTO scenes (id, episode_id, position) VALUES (?1, ?2, ?3)",
        params![id, episode_id, position],
    )?;
    tx.commit()?;
    Ok(id)
}

fn insert_node(conn: &Connection, kind: NodeKind, title: &str) -> DbResult<i64> {
    conn.execute(
        "INSERT INTO nodes (kind, title) VALUES (?1, ?2)",
        params![kind.as_str(), title],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn get(conn: &Connection, id: i64) -> DbResult<Node> {
    conn.query_row(
        &format!("SELECT {COLS} FROM nodes WHERE id = ?1"),
        [id],
        from_row,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(id),
        other => other.into(),
    })
}

pub fn update_text(
    conn: &Connection,
    id: i64,
    title: &str,
    summary: &str,
    notes: &str,
) -> DbResult<()> {
    let n = conn.execute(
        "UPDATE nodes SET title = ?2, summary = ?3, notes = ?4 WHERE id = ?1",
        params![id, title, summary, notes],
    )?;
    if n == 0 {
        return Err(DbError::NotFound(id));
    }
    Ok(())
}

/// Supprime le nœud ; fiche de détail, liens et index de recherche suivent (cascade/triggers).
pub fn delete(conn: &Connection, id: i64) -> DbResult<()> {
    let tx = Atomic::begin(conn)?;
    // Les scènes d'un épisode sont aussi des nœuds : la cascade SQL ne supprimerait que
    // leur ligne de détail, laissant des nœuds orphelins. On les supprime explicitement.
    tx.execute(
        "DELETE FROM nodes WHERE id IN (SELECT id FROM scenes WHERE episode_id = ?1)",
        [id],
    )?;
    let n = tx.execute("DELETE FROM nodes WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(DbError::NotFound(id)); // la transaction est annulée à la sortie
    }
    tx.commit()?;
    Ok(())
}

pub fn list_by_kind(conn: &Connection, kind: NodeKind) -> DbResult<Vec<Node>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM nodes WHERE kind = ?1 ORDER BY title COLLATE NOCASE"
    ))?;
    let rows = stmt.query_map([kind.as_str()], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Éléments sans aucune connexion.
pub fn isolated(conn: &Connection) -> DbResult<Vec<Node>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM v_isolated_nodes ORDER BY title"))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Recherche plein texte (titre, résumé, notes) saisie par l'utilisateur : le texte est
/// protégé (guillemets) pour qu'aucune syntaxe FTS5 ne puisse provoquer d'erreur,
/// et chaque mot est cherché comme préfixe (recherche instantanée).
pub fn search_text(conn: &Connection, text: &str) -> DbResult<Vec<Node>> {
    let q = text
        .split_whitespace()
        .map(|w| format!("\"{}\"*", w.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ");
    if q.is_empty() {
        return Ok(Vec::new());
    }
    search(conn, &q)
}

/// Recherche plein texte (titre, résumé, notes). `query` est une requête FTS5.
pub fn search(conn: &Connection, query: &str) -> DbResult<Vec<Node>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.kind, n.title, n.summary, n.notes, n.updated_at
         FROM search_index s JOIN nodes n ON n.id = s.rowid
         WHERE search_index MATCH ?1 ORDER BY rank",
    )?;
    let rows = stmt.query_map([query], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn create_get_update_delete() {
        let conn = open_in_memory().unwrap();
        let id = create(&conn, NodeKind::Character, "Alice").unwrap();
        assert_eq!(get(&conn, id).unwrap().title, "Alice");
        update_text(&conn, id, "Alice M.", "résumé", "").unwrap();
        assert_eq!(get(&conn, id).unwrap().title, "Alice M.");
        delete(&conn, id).unwrap();
        assert!(matches!(get(&conn, id), Err(DbError::NotFound(_))));
    }

    #[test]
    fn episodes_and_scenes_need_their_own_constructors() {
        let conn = open_in_memory().unwrap();
        assert!(create(&conn, NodeKind::Episode, "x").is_err());
        let ep = create_episode(&conn, "Pilote", 1, 1).unwrap();
        create_scene(&conn, ep, "Scène 1", 1).unwrap();
        assert!(create_episode(&conn, "Doublon", 1, 1).is_err()); // (saison, numéro) unique
    }

    #[test]
    fn deleting_an_episode_removes_its_scene_nodes() {
        let conn = open_in_memory().unwrap();
        let ep = create_episode(&conn, "Pilote", 1, 1).unwrap();
        create_scene(&conn, ep, "Scène 1", 1).unwrap();
        delete(&conn, ep).unwrap();
        assert!(all(&conn).unwrap().is_empty());
    }

    #[test]
    fn user_search_text_never_breaks_fts_syntax() {
        let conn = open_in_memory().unwrap();
        create(&conn, NodeKind::Character, "Alice Martin").unwrap();
        assert_eq!(search_text(&conn, "ali").unwrap().len(), 1); // préfixe
        assert!(search_text(&conn, "\" AND ( OR *").is_ok()); // syntaxe hostile
        assert!(search_text(&conn, "   ").unwrap().is_empty());
    }

    #[test]
    fn isolated_and_search() {
        let conn = open_in_memory().unwrap();
        create(&conn, NodeKind::Object, "Clé orpheline").unwrap();
        assert_eq!(isolated(&conn).unwrap().len(), 1);
        assert_eq!(search(&conn, "orpheline").unwrap().len(), 1);
    }
}
