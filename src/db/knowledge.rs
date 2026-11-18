//! Ce que sait chaque personnage, et à quel événement il l'apprend.

use rusqlite::Connection;

use super::DbResult;

#[derive(Debug, Clone)]
pub struct KnowledgeRow {
    pub id: i64,
    pub subject_id: i64,
    pub subject_title: String,
    pub learned_event_title: Option<String>,
}

pub fn add(
    conn: &Connection,
    character_id: i64,
    subject_id: i64,
    learned_event_id: Option<i64>,
) -> DbResult<i64> {
    conn.execute(
        "INSERT INTO knowledge (character_id, subject_id, learned_event_id) VALUES (?1, ?2, ?3)",
        rusqlite::params![character_id, subject_id, learned_event_id],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete(conn: &Connection, id: i64) -> DbResult<()> {
    conn.execute("DELETE FROM knowledge WHERE id = ?1", [id])?;
    Ok(())
}

pub fn for_character(conn: &Connection, character_id: i64) -> DbResult<Vec<KnowledgeRow>> {
    let mut stmt = conn.prepare(
        "SELECT k.id, k.subject_id, s.title, e.title
         FROM knowledge k
         JOIN nodes s ON s.id = k.subject_id
         LEFT JOIN nodes e ON e.id = k.learned_event_id
         WHERE k.character_id = ?1 ORDER BY k.id",
    )?;
    let rows = stmt.query_map([character_id], |r| {
        Ok(KnowledgeRow {
            id: r.get(0)?,
            subject_id: r.get(1)?,
            subject_title: r.get(2)?,
            learned_event_title: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}
