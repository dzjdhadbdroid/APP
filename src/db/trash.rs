//! Corbeille : avant toute suppression, l'élément (et ses scènes, étiquettes, liens) est
//! conservé sous forme d'instantané JSON ; on peut le restaurer ou le supprimer pour de bon.

use std::collections::HashSet;

use rusqlite::{params, Connection};

use super::{nodes::NodeKind, transfer, Atomic, DbError, DbResult};

const KEEP: i64 = 100;

#[derive(Debug, Clone)]
pub struct TrashRow {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub deleted_at: String,
}

/// Identifiants à mettre à la corbeille avec un élément : lui-même et, pour un épisode, ses scènes.
fn with_scenes(conn: &Connection, id: i64) -> DbResult<HashSet<i64>> {
    let mut ids = HashSet::from([id]);
    let mut stmt = conn.prepare("SELECT id FROM scenes WHERE episode_id = ?1")?;
    for r in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
        ids.insert(r?);
    }
    Ok(ids)
}

/// Conserve un instantané de l'élément avant sa suppression.
pub fn put(conn: &Connection, id: i64) -> DbResult<()> {
    let (kind, title): (String, String) = conn
        .query_row("SELECT kind, title FROM nodes WHERE id = ?1", [id], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(|_| DbError::NotFound(id))?;
    NodeKind::parse(&kind)?;
    let payload = transfer::export_filtered(conn, Some(&with_scenes(conn, id)?))?;
    let tx = Atomic::begin(conn)?;
    tx.execute(
        "INSERT INTO trash (kind, title, payload) VALUES (?1, ?2, ?3)",
        params![kind, title, payload],
    )?;
    tx.execute(
        "DELETE FROM trash WHERE id NOT IN (SELECT id FROM trash ORDER BY id DESC LIMIT ?1)",
        [KEEP],
    )?;
    tx.commit()
}

pub fn list(conn: &Connection) -> DbResult<Vec<TrashRow>> {
    let mut stmt =
        conn.prepare("SELECT id, kind, title, deleted_at FROM trash ORDER BY id DESC")?;
    let rows = stmt.query_map([], |r| {
        Ok(TrashRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            deleted_at: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Recrée l'élément (nouvel identifiant) et ses liens avec ce qui existe encore.
pub fn restore(conn: &Connection, id: i64) -> DbResult<()> {
    let payload: String = conn
        .query_row("SELECT payload FROM trash WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .map_err(|_| DbError::NotFound(id))?;
    let tx = Atomic::begin(conn)?;
    transfer::import_restoring(&tx, &payload)?;
    tx.execute("DELETE FROM trash WHERE id = ?1", [id])?;
    tx.commit()
}

pub fn purge(conn: &Connection, id: i64) -> DbResult<()> {
    conn.execute("DELETE FROM trash WHERE id = ?1", [id])?;
    Ok(())
}

pub fn empty(conn: &Connection) -> DbResult<()> {
    conn.execute("DELETE FROM trash", [])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        links, nodes, open_in_memory,
        script::{self, BlockType},
        tags,
    };

    #[test]
    fn deleted_character_comes_back_with_links_and_tags() {
        let conn = open_in_memory().unwrap();
        let alice = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        let bob = nodes::create(&conn, NodeKind::Character, "Bob").unwrap();
        links::create(&conn, alice, bob, "relation", Some("sœurs"), Some(7)).unwrap();
        tags::add(&conn, alice, "famille").unwrap();
        put(&conn, alice).unwrap();
        nodes::delete(&conn, alice).unwrap();
        assert_eq!(nodes::all(&conn).unwrap().len(), 1);
        let row = &list(&conn).unwrap()[0];
        assert_eq!(
            (row.kind.as_str(), row.title.as_str()),
            ("character", "Alice")
        );
        restore(&conn, row.id).unwrap();
        assert!(list(&conn).unwrap().is_empty());
        let back = nodes::search_text(&conn, "Alice")
            .unwrap()
            .into_iter()
            .find(|n| n.title == "Alice")
            .unwrap();
        assert_eq!(tags::for_node(&conn, back.id).unwrap()[0].1, "famille");
        let l = links::for_node(&conn, back.id).unwrap();
        assert_eq!((l.len(), l[0].label.as_deref()), (1, Some("sœurs")));
        assert_eq!(links::for_node(&conn, bob).unwrap().len(), 1);
    }

    #[test]
    fn deleted_episode_returns_with_its_scenes_and_dialogue() {
        let conn = open_in_memory().unwrap();
        let alice = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        let ep = nodes::create_episode(&conn, "Pilote", 1, 1).unwrap();
        let sc = nodes::create_scene(&conn, ep, "Cuisine", 1).unwrap();
        script::add(
            &conn,
            sc,
            BlockType::Dialogue,
            Some(alice),
            "Bonjour tout le monde",
        )
        .unwrap();
        put(&conn, ep).unwrap();
        nodes::delete(&conn, ep).unwrap();
        assert_eq!(nodes::all(&conn).unwrap().len(), 1);
        restore(&conn, list(&conn).unwrap()[0].id).unwrap();
        let scenes = crate::db::timeline::list_scenes(&conn).unwrap();
        assert_eq!(scenes.len(), 1);
        let blocks = script::list(&conn, scenes[0].id).unwrap();
        assert_eq!(
            (blocks[0].text.as_str(), blocks[0].character_id),
            ("Bonjour tout le monde", Some(alice))
        );
        assert!(put(&conn, 99_999).is_err());
    }
}
