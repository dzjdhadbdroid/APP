//! Relations entre personnages : catégorie (libellé), proximité 0–10, description,
//! évolution dans le temps (`link_history`) et informations connues de chaque personnage (`meta`).

use rusqlite::{params, Connection};
use serde::Serialize;
use serde_json::{json, Value};

use super::{Atomic, DbError, DbResult};

#[derive(Debug, Clone, Serialize)]
pub struct Relation {
    pub id: i64,
    pub a: i64,
    pub b: i64,
    pub label: String,
    pub closeness: Option<i64>,
    pub description: String,
    /// Ce que `a` sait de `b`, et ce que `b` sait de `a`.
    pub a_knows: String,
    pub b_knows: String,
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub id: i64,
    pub note: String,
    pub event_id: Option<i64>,
    pub created_at: String,
}

fn parse_meta(raw: &str) -> (String, String) {
    let v: Value = serde_json::from_str(raw).unwrap_or(Value::Null);
    let get = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    (get("a_knows"), get("b_knows"))
}

pub fn list(conn: &Connection) -> DbResult<Vec<Relation>> {
    let mut stmt = conn.prepare(
        "SELECT id, src_id, dst_id, COALESCE(label,''), closeness, COALESCE(description,''), meta
         FROM links WHERE link_type = 'relation' ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        let meta: String = r.get(6)?;
        let (a_knows, b_knows) = parse_meta(&meta);
        Ok(Relation {
            id: r.get(0)?,
            a: r.get(1)?,
            b: r.get(2)?,
            label: r.get(3)?,
            closeness: r.get(4)?,
            description: r.get(5)?,
            a_knows,
            b_knows,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Crée une relation entre deux personnages (refuse les autres types d'éléments).
pub fn create(
    conn: &Connection,
    a: i64,
    b: i64,
    label: &str,
    closeness: Option<i64>,
) -> DbResult<i64> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM nodes WHERE kind = 'character' AND id IN (?1, ?2)",
        params![a, b],
        |r| r.get(0),
    )?;
    if a == b || n != 2 {
        return Err(DbError::Invalid(
            "une relation relie deux personnages différents".into(),
        ));
    }
    super::links::create(conn, a, b, "relation", Some(label), closeness)
}

pub fn update(
    conn: &Connection,
    id: i64,
    label: &str,
    closeness: Option<i64>,
    description: &str,
    a_knows: &str,
    b_knows: &str,
) -> DbResult<()> {
    if let Some(c) = closeness {
        if !(0..=10).contains(&c) {
            return Err(DbError::Invalid("la proximité va de 0 à 10".into()));
        }
    }
    let meta = json!({ "a_knows": a_knows, "b_knows": b_knows }).to_string();
    let n = conn.execute(
        "UPDATE links SET label = ?2, closeness = ?3, description = ?4, meta = ?5
         WHERE id = ?1 AND link_type = 'relation'",
        params![id, label, closeness, description, meta],
    )?;
    if n == 0 {
        return Err(DbError::NotFound(id));
    }
    Ok(())
}

/// Note l'évolution de la relation (optionnellement rattachée à un événement).
pub fn add_history(conn: &Connection, link: i64, note: &str, event: Option<i64>) -> DbResult<()> {
    if note.trim().is_empty() {
        return Err(DbError::Invalid("note vide".into()));
    }
    let tx = Atomic::begin(conn)?;
    tx.execute(
        "INSERT INTO link_history (link_id, event_id, note) VALUES (?1, ?2, ?3)",
        params![link, event, note.trim()],
    )?;
    tx.commit()
}

pub fn history(conn: &Connection, link: i64) -> DbResult<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, note, event_id, created_at FROM link_history WHERE link_id = ?1 ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([link], |r| {
        Ok(HistoryEntry {
            id: r.get(0)?,
            note: r.get(1)?,
            event_id: r.get(2)?,
            created_at: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{nodes, nodes::NodeKind, open_in_memory};

    #[test]
    fn relation_lifecycle() {
        let conn = open_in_memory().unwrap();
        let a = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        let b = nodes::create(&conn, NodeKind::Character, "Bob").unwrap();
        let p = nodes::create(&conn, NodeKind::Place, "Bar").unwrap();
        assert!(create(&conn, a, p, "ami", None).is_err());
        assert!(create(&conn, a, a, "ami", None).is_err());
        let id = create(&conn, a, b, "colocataires", Some(6)).unwrap();
        update(
            &conn,
            id,
            "amis",
            Some(8),
            "se chamaillent",
            "il ronfle",
            "",
        )
        .unwrap();
        assert!(update(&conn, id, "x", Some(11), "", "", "").is_err());
        let r = &list(&conn).unwrap()[0];
        assert_eq!(
            (r.label.as_str(), r.closeness, r.a_knows.as_str()),
            ("amis", Some(8), "il ronfle")
        );
        add_history(&conn, id, "Dispute pour le loyer", None).unwrap();
        assert!(add_history(&conn, id, "  ", None).is_err());
        assert_eq!(history(&conn, id).unwrap().len(), 1);
    }
}
