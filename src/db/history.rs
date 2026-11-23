//! Historique des modifications d'une fiche, avec restauration (elle-même réversible).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::{fields, nodes, Atomic, DbError, DbResult};

const KEEP_PER_NODE: i64 = 50;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    title: String,
    summary: String,
    notes: String,
    fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct VersionRow {
    pub id: i64,
    pub saved_at: String,
    pub title: String,
}

/// Enregistre une fiche. Si quelque chose change, l'état PRÉCÉDENT est d'abord archivé.
/// Sans changement : rien n'est écrit (ni historique, ni date de modification).
pub fn save_node(
    conn: &Connection,
    id: i64,
    title: &str,
    summary: &str,
    notes: &str,
    new_fields: &[String],
) -> DbResult<()> {
    let node = nodes::get(conn, id)?;
    let old_fields = fields::load(conn, node.kind, id)?;
    if node.title == title
        && node.summary == summary
        && node.notes == notes
        && old_fields == new_fields
    {
        return Ok(());
    }
    let snapshot = serde_json::to_string(&Snapshot {
        title: node.title.clone(),
        summary: node.summary.clone(),
        notes: node.notes.clone(),
        fields: old_fields,
    })
    .map_err(|e| DbError::Invalid(e.to_string()))?;

    let tx = Atomic::begin(conn)?;
    tx.execute(
        "INSERT INTO node_history (node_id, snapshot) VALUES (?1, ?2)",
        params![id, snapshot],
    )?;
    tx.execute(
        "DELETE FROM node_history WHERE node_id = ?1 AND id NOT IN
         (SELECT id FROM node_history WHERE node_id = ?1 ORDER BY id DESC LIMIT ?2)",
        params![id, KEEP_PER_NODE],
    )?;
    nodes::update_text(&tx, id, title, summary, notes)?;
    fields::save(&tx, node.kind, id, new_fields)?;
    tx.commit()
}

/// Ajoute du texte à la fin des notes d'une fiche (version précédente archivée, donc annulable).
pub fn append_note(conn: &Connection, id: i64, addition: &str) -> DbResult<()> {
    let n = nodes::get(conn, id)?;
    let f = fields::load(conn, n.kind, id)?;
    let notes = if n.notes.is_empty() {
        addition.to_string()
    } else {
        format!("{}\n\n{addition}", n.notes)
    };
    save_node(conn, id, &n.title, &n.summary, &notes, &f)
}

/// Versions archivées d'une fiche, de la plus récente à la plus ancienne.
pub fn list(conn: &Connection, node_id: i64) -> DbResult<Vec<VersionRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, saved_at, COALESCE(json_extract(snapshot, '$.title'), '')
         FROM node_history WHERE node_id = ?1 ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([node_id], |r| {
        Ok(VersionRow {
            id: r.get(0)?,
            saved_at: r.get(1)?,
            title: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Remet une fiche dans l'état d'une version archivée. L'état actuel est archivé à son tour,
/// donc une restauration peut elle-même être annulée. Retourne l'id de la fiche.
pub fn restore(conn: &Connection, version_id: i64) -> DbResult<i64> {
    let (node_id, json): (i64, String) = conn
        .query_row(
            "SELECT node_id, snapshot FROM node_history WHERE id = ?1",
            [version_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(version_id),
            other => other.into(),
        })?;
    let mut snap: Snapshot =
        serde_json::from_str(&json).map_err(|e| DbError::Invalid(e.to_string()))?;
    let kind = nodes::get(conn, node_id)?.kind;
    snap.fields.resize(fields::defs(kind).len(), String::new()); // le schéma a pu évoluer
    save_node(
        conn,
        node_id,
        &snap.title,
        &snap.summary,
        &snap.notes,
        &snap.fields,
    )?;
    Ok(node_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{nodes::NodeKind, open_in_memory};

    fn char_fields(conn: &Connection, id: i64, first_name: &str) -> Vec<String> {
        let mut f = fields::load(conn, NodeKind::Character, id).unwrap();
        f[0] = first_name.to_string();
        f
    }

    #[test]
    fn edits_are_archived_and_restorable() {
        let conn = open_in_memory().unwrap();
        let id = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();

        let f = char_fields(&conn, id, "Alice");
        save_node(&conn, id, "Alice", "v1", "", &f).unwrap();
        let f = char_fields(&conn, id, "Alicia");
        save_node(&conn, id, "Alice B.", "v2", "", &f).unwrap();
        assert_eq!(list(&conn, id).unwrap().len(), 2);

        let oldest = list(&conn, id).unwrap().last().unwrap().id; // état d'origine : "Alice", sans résumé
        restore(&conn, oldest).unwrap();
        let n = nodes::get(&conn, id).unwrap();
        assert_eq!((n.title.as_str(), n.summary.as_str()), ("Alice", ""));
        assert_eq!(fields::load(&conn, NodeKind::Character, id).unwrap()[0], "");
        assert_eq!(list(&conn, id).unwrap().len(), 3); // la restauration est réversible
    }

    #[test]
    fn appending_a_note_keeps_the_old_text_in_history() {
        let conn = open_in_memory().unwrap();
        let id = nodes::create(&conn, NodeKind::Plot, "Intrigue").unwrap();
        let f = fields::load(&conn, NodeKind::Plot, id).unwrap();
        save_node(&conn, id, "Intrigue", "", "note existante", &f).unwrap();
        append_note(&conn, id, "[IA] idée").unwrap();
        assert_eq!(
            nodes::get(&conn, id).unwrap().notes,
            "note existante\n\n[IA] idée"
        );
        assert_eq!(list(&conn, id).unwrap().len(), 2);
    }

    #[test]
    fn saving_without_changes_creates_no_version() {
        let conn = open_in_memory().unwrap();
        let id = nodes::create(&conn, NodeKind::Place, "Appart").unwrap();
        let f = fields::load(&conn, NodeKind::Place, id).unwrap();
        save_node(&conn, id, "Appart", "", "", &f).unwrap();
        assert!(list(&conn, id).unwrap().is_empty());
    }

    #[test]
    fn history_is_capped() {
        let conn = open_in_memory().unwrap();
        let id = nodes::create(&conn, NodeKind::Object, "Clé").unwrap();
        let f = fields::load(&conn, NodeKind::Object, id).unwrap();
        for i in 0..(KEEP_PER_NODE + 10) {
            save_node(&conn, id, &format!("Clé {i}"), "", "", &f).unwrap();
        }
        assert_eq!(list(&conn, id).unwrap().len() as i64, KEEP_PER_NODE);
    }
}
