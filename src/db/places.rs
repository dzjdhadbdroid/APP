//! Sous-lieux : hiérarchie de lieux (`places.parent_id`) synchronisée avec le lien `contains`.

use rusqlite::{params, Connection, OptionalExtension};

use super::{Atomic, DbError, DbResult};

pub fn parent_of(conn: &Connection, place: i64) -> DbResult<Option<i64>> {
    let p: Option<Option<i64>> = conn
        .query_row("SELECT parent_id FROM places WHERE id = ?1", [place], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(p.flatten())
}

/// Sous-lieux directs, triés par titre.
pub fn children(conn: &Connection, place: i64) -> DbResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, n.title FROM places p JOIN nodes n ON n.id = p.id
         WHERE p.parent_id = ?1 ORDER BY n.title COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([place], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Le lieu `candidate` est-il `place` lui-même ou l'un de ses descendants ?
fn is_self_or_descendant(conn: &Connection, place: i64, candidate: i64) -> DbResult<bool> {
    let n: i64 = conn.query_row(
        "WITH RECURSIVE sub(id) AS (
             SELECT ?1 UNION SELECT p.id FROM places p JOIN sub s ON p.parent_id = s.id
         ) SELECT COUNT(*) FROM sub WHERE id = ?2",
        params![place, candidate],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Place `place` sous `parent` (ou à la racine si `None`). Refuse les cycles.
/// Le lien `contains` parent → sous-lieu est tenu à jour.
pub fn set_parent(conn: &Connection, place: i64, parent: Option<i64>) -> DbResult<()> {
    if let Some(p) = parent {
        if is_self_or_descendant(conn, place, p)? {
            return Err(DbError::Invalid(
                "un lieu ne peut pas être placé dans lui-même ni dans l'un de ses sous-lieux"
                    .into(),
            ));
        }
        let exists: i64 =
            conn.query_row("SELECT COUNT(*) FROM places WHERE id = ?1", [p], |r| {
                r.get(0)
            })?;
        if exists == 0 {
            return Err(DbError::NotFound(p));
        }
    }
    let tx = Atomic::begin(conn)?;
    if let Some(old) = parent_of(&tx, place)? {
        tx.execute(
            "DELETE FROM links WHERE src_id = ?1 AND dst_id = ?2 AND link_type = 'contains'",
            params![old, place],
        )?;
    }
    let n = tx.execute(
        "UPDATE places SET parent_id = ?2 WHERE id = ?1",
        params![place, parent],
    )?;
    if n == 0 {
        return Err(DbError::NotFound(place));
    }
    if let Some(p) = parent {
        tx.execute(
            "INSERT OR IGNORE INTO links (src_id, dst_id, link_type) VALUES (?1, ?2, 'contains')",
            params![p, place],
        )?;
    }
    tx.execute(
        "UPDATE nodes SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE id = ?1",
        [place],
    )?;
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{links, nodes, nodes::NodeKind, open_in_memory};

    #[test]
    fn hierarchy_links_and_cycle_prevention() {
        let conn = open_in_memory().unwrap();
        let house = nodes::create(&conn, NodeKind::Place, "Maison").unwrap();
        let kitchen = nodes::create(&conn, NodeKind::Place, "Cuisine").unwrap();
        let pantry = nodes::create(&conn, NodeKind::Place, "Garde-manger").unwrap();
        set_parent(&conn, kitchen, Some(house)).unwrap();
        set_parent(&conn, pantry, Some(kitchen)).unwrap();
        assert_eq!(
            children(&conn, house).unwrap(),
            vec![(kitchen, "Cuisine".to_string())]
        );
        assert_eq!(links::for_node(&conn, house).unwrap().len(), 1);
        assert!(set_parent(&conn, house, Some(pantry)).is_err()); // cycle
        assert!(set_parent(&conn, house, Some(house)).is_err());
        set_parent(&conn, pantry, Some(house)).unwrap(); // déplacement
        assert_eq!(links::for_node(&conn, kitchen).unwrap().len(), 1);
        set_parent(&conn, kitchen, None).unwrap();
        assert_eq!(parent_of(&conn, kitchen).unwrap(), None);
        assert_eq!(links::for_node(&conn, house).unwrap().len(), 1); // il reste pantry
    }
}
