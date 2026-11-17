//! Dépôt des nœuds : identité commune + ligne de détail du type concerné.

use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use super::{DbError, DbResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

    /// Nom de la table de détail (valeurs fixes : jamais issues de l'utilisateur).
    fn detail_table(self) -> &'static str {
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
    let tx = conn.unchecked_transaction()?;
    let id = insert_node(&tx, kind, title)?;
    tx.execute(
        &format!("INSERT INTO {} (id) VALUES (?1)", kind.detail_table()),
        [id],
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn create_episode(conn: &Connection, title: &str, season: i64, number: i64) -> DbResult<i64> {
    let tx = conn.unchecked_transaction()?;
    let id = insert_node(&tx, NodeKind::Episode, title)?;
    tx.execute(
        "INSERT INTO episodes (id, season, number) VALUES (?1, ?2, ?3)",
        params![id, season, number],
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn create_scene(
    conn: &Connection,
    episode_id: i64,
    title: &str,
    position: i64,
) -> DbResult<i64> {
    let tx = conn.unchecked_transaction()?;
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
    let n = conn.execute("DELETE FROM nodes WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(DbError::NotFound(id));
    }
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
    fn isolated_and_search() {
        let conn = open_in_memory().unwrap();
        create(&conn, NodeKind::Object, "Clé orpheline").unwrap();
        assert_eq!(isolated(&conn).unwrap().len(), 1);
        assert_eq!(search(&conn, "orpheline").unwrap().len(), 1);
    }
}
