//! Dépôt des liens typés et parcours du graphe.

use rusqlite::{params, Connection, Row};
use serde::Serialize;

use super::{DbError, DbResult};

#[derive(Debug, Clone, Serialize)]
pub struct Link {
    pub id: i64,
    pub src_id: i64,
    pub dst_id: i64,
    pub link_type: String,
    pub label: Option<String>,
    pub closeness: Option<i64>,
    pub description: Option<String>,
}

fn from_row(row: &Row) -> rusqlite::Result<Link> {
    Ok(Link {
        id: row.get("id")?,
        src_id: row.get("src_id")?,
        dst_id: row.get("dst_id")?,
        link_type: row.get("link_type")?,
        label: row.get("label")?,
        closeness: row.get("closeness")?,
        description: row.get("description")?,
    })
}

const COLS: &str = "id, src_id, dst_id, link_type, label, closeness, description";

pub fn create(
    conn: &Connection,
    src_id: i64,
    dst_id: i64,
    link_type: &str,
    label: Option<&str>,
    closeness: Option<i64>,
) -> DbResult<i64> {
    conn.execute(
        "INSERT INTO links (src_id, dst_id, link_type, label, closeness)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![src_id, dst_id, link_type, label, closeness],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete(conn: &Connection, id: i64) -> DbResult<()> {
    let n = conn.execute("DELETE FROM links WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(DbError::NotFound(id));
    }
    Ok(())
}

/// Tous les liens de l'univers.
pub fn all(conn: &Connection) -> DbResult<Vec<Link>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM links ORDER BY id"))?;
    let rows = stmt.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Tous les liens touchant un nœud (entrants et sortants).
pub fn for_node(conn: &Connection, node_id: i64) -> DbResult<Vec<Link>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM links WHERE src_id = ?1 OR dst_id = ?1 ORDER BY link_type, id"
    ))?;
    let rows = stmt.query_map([node_id], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Nœuds atteignables depuis `start` en au plus `max_depth` sauts (liens non orientés).
/// Retourne `(id, distance)` ; distance 1 = relation directe, ≥ 2 = indirecte.
pub fn reachable(conn: &Connection, start: i64, max_depth: i64) -> DbResult<Vec<(i64, i64)>> {
    let mut stmt = conn.prepare(
        "WITH RECURSIVE reach(id, depth) AS (
             SELECT ?1, 0
             UNION
             SELECT CASE WHEN l.src_id = r.id THEN l.dst_id ELSE l.src_id END, r.depth + 1
             FROM reach r JOIN links l ON (l.src_id = r.id OR l.dst_id = r.id)
             WHERE r.depth < ?2
         )
         SELECT id, MIN(depth) AS d FROM reach WHERE id <> ?1
         GROUP BY id ORDER BY d, id",
    )?;
    let rows = stmt.query_map(params![start, max_depth], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{nodes, nodes::NodeKind, open_in_memory};

    #[test]
    fn direct_and_indirect_reachability() {
        let conn = open_in_memory().unwrap();
        let a = nodes::create(&conn, NodeKind::Character, "A").unwrap();
        let b = nodes::create(&conn, NodeKind::Character, "B").unwrap();
        let p = nodes::create(&conn, NodeKind::Place, "Appart").unwrap();
        create(&conn, a, b, "relation", Some("frère"), Some(8)).unwrap();
        create(&conn, b, p, "lives_in", None, None).unwrap();

        assert_eq!(reachable(&conn, a, 1).unwrap(), vec![(b, 1)]);
        assert_eq!(reachable(&conn, a, 2).unwrap(), vec![(b, 1), (p, 2)]);
    }

    #[test]
    fn deleting_a_node_removes_its_links() {
        let conn = open_in_memory().unwrap();
        let a = nodes::create(&conn, NodeKind::Character, "A").unwrap();
        let b = nodes::create(&conn, NodeKind::Character, "B").unwrap();
        create(&conn, a, b, "relation", None, None).unwrap();
        nodes::delete(&conn, a).unwrap();
        assert!(for_node(&conn, b).unwrap().is_empty());
    }

    #[test]
    fn self_link_and_duplicates_are_rejected() {
        let conn = open_in_memory().unwrap();
        let a = nodes::create(&conn, NodeKind::Character, "A").unwrap();
        let b = nodes::create(&conn, NodeKind::Character, "B").unwrap();
        assert!(create(&conn, a, a, "relation", None, None).is_err());
        create(&conn, a, b, "relation", None, None).unwrap();
        assert!(create(&conn, a, b, "relation", None, None).is_err());
    }
}
