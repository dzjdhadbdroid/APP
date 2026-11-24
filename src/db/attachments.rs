//! PDF rattachés à un épisode ou une scène, stockés dans la base (donc sauvegardés avec elle).

use rusqlite::{params, Connection};

use super::{DbError, DbResult};

pub const MAX_BYTES: usize = 50 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Attachment {
    pub id: i64,
    pub filename: String,
    pub size: i64,
    pub added_at: String,
}

/// Ajoute un PDF. `slot` : « synopsis » ou « script ». Le contenu doit être un vrai PDF.
pub fn add(conn: &Connection, node: i64, slot: &str, filename: &str, data: &[u8]) -> DbResult<i64> {
    if !data.starts_with(b"%PDF-") {
        return Err(DbError::Invalid("ce fichier n'est pas un PDF".into()));
    }
    if data.len() > MAX_BYTES {
        return Err(DbError::Invalid(
            "PDF trop volumineux (50 Mo maximum)".into(),
        ));
    }
    let name = filename
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(filename)
        .trim();
    let name = if name.is_empty() {
        "document.pdf"
    } else {
        name
    };
    conn.execute(
        "INSERT INTO attachments (node_id, slot, filename, size, data) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![node, slot, name, data.len() as i64, data],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Enregistre le texte extrait d'un PDF (500 000 caractères au plus) et met à jour la recherche.
pub fn set_text(conn: &Connection, id: i64, text: &str) -> DbResult<()> {
    let text: String = text.chars().take(500_000).collect();
    let node: i64 = conn
        .query_row("SELECT node_id FROM attachments WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .map_err(|_| DbError::NotFound(id))?;
    conn.execute(
        "UPDATE attachments SET text = ?2 WHERE id = ?1",
        params![id, text],
    )?;
    super::tags::reindex(conn, node)
}

pub fn text(conn: &Connection, id: i64) -> DbResult<String> {
    conn.query_row("SELECT text FROM attachments WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .map_err(|_| DbError::NotFound(id))
}

pub fn list(conn: &Connection, node: i64, slot: &str) -> DbResult<Vec<Attachment>> {
    let mut stmt = conn.prepare(
        "SELECT id, filename, size, added_at FROM attachments WHERE node_id = ?1 AND slot = ?2 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![node, slot], |r| {
        Ok(Attachment {
            id: r.get(0)?,
            filename: r.get(1)?,
            size: r.get(2)?,
            added_at: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// (nom du fichier, contenu)
pub fn data(conn: &Connection, id: i64) -> DbResult<(String, Vec<u8>)> {
    conn.query_row(
        "SELECT filename, data FROM attachments WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .map_err(|_| DbError::NotFound(id))
}

pub fn delete(conn: &Connection, id: i64) -> DbResult<()> {
    let n = conn.execute("DELETE FROM attachments WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(DbError::NotFound(id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{nodes, open_in_memory};

    #[test]
    fn pdf_attachments_roundtrip_and_validation() {
        let conn = open_in_memory().unwrap();
        let ep = nodes::create_episode(&conn, "Pilote", 1, 1).unwrap();
        let pdf = b"%PDF-1.4\nhello\n%%EOF".to_vec();
        let id = add(&conn, ep, "synopsis", "C:\\docs\\mon synopsis.pdf", &pdf).unwrap();
        assert!(add(&conn, ep, "synopsis", "x.pdf", b"pas un pdf").is_err());
        assert!(add(&conn, ep, "autre", "x.pdf", &pdf).is_err()); // créneau inconnu
        let list = list(&conn, ep, "synopsis").unwrap();
        assert_eq!(
            (list.len(), list[0].filename.as_str(), list[0].size),
            (1, "mon synopsis.pdf", pdf.len() as i64)
        );
        assert!(super::list(&conn, ep, "script").unwrap().is_empty());
        set_text(&conn, id, "Alice entre en trombe").unwrap();
        assert_eq!(text(&conn, id).unwrap(), "Alice entre en trombe");
        assert_eq!(
            crate::db::nodes::search_text(&conn, "trombe")
                .unwrap()
                .len(),
            1
        );
        assert_eq!(data(&conn, id).unwrap().1, pdf);
        nodes::delete(&conn, ep).unwrap(); // supprimé avec l'épisode
        assert!(data(&conn, id).is_err());
        let ep2 = nodes::create_episode(&conn, "B", 1, 2).unwrap();
        let id2 = add(&conn, ep2, "script", "s.pdf", &pdf).unwrap();
        delete(&conn, id2).unwrap();
        assert!(delete(&conn, id2).is_err());
    }
}
