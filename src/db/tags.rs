//! Étiquettes libres sur n'importe quel élément (personnage, épisode, scène, lieu…).
//! Elles sont ajoutées au texte indexé : la recherche globale les retrouve.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};

use super::{fields, nodes::NodeKind, Atomic, DbError, DbResult};

/// Nettoie un nom d'étiquette : espaces réduits, `#` initial retiré, 40 caractères max.
pub fn normalize(raw: &str) -> DbResult<String> {
    let cleaned = raw
        .trim()
        .trim_start_matches('#')
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned: String = cleaned.chars().take(40).collect();
    if cleaned.is_empty() {
        return Err(DbError::Invalid("étiquette vide".into()));
    }
    Ok(cleaned)
}

/// Étiquettes d'un élément, par ordre alphabétique.
pub fn for_node(conn: &Connection, node: i64) -> DbResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name FROM node_tags nt JOIN tags t ON t.id = nt.tag_id
         WHERE nt.node_id = ?1 ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([node], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Toutes les étiquettes utilisées, avec leur nombre d'éléments.
pub fn all(conn: &Connection) -> DbResult<Vec<(i64, String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, COUNT(nt.node_id) FROM tags t LEFT JOIN node_tags nt ON nt.tag_id = t.id
         GROUP BY t.id ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Éléments portant une étiquette.
pub fn node_ids(conn: &Connection, tag: i64) -> DbResult<HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT node_id FROM node_tags WHERE tag_id = ?1")?;
    let rows = stmt.query_map([tag], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn reindex(conn: &Connection, node: i64) -> DbResult<()> {
    let kind: String = conn
        .query_row("SELECT kind FROM nodes WHERE id = ?1", [node], |r| r.get(0))
        .optional()?
        .ok_or(DbError::NotFound(node))?;
    fields::rebuild_body(conn, NodeKind::parse(&kind)?, node)
}

/// Ajoute une étiquette (créée au besoin). Sans effet si l'élément l'a déjà.
pub fn add(conn: &Connection, node: i64, name: &str) -> DbResult<()> {
    let name = normalize(name)?;
    let tx = Atomic::begin(conn)?;
    tx.execute("INSERT OR IGNORE INTO tags (name) VALUES (?1)", [&name])?;
    let tag: i64 = tx.query_row("SELECT id FROM tags WHERE name = ?1", [&name], |r| r.get(0))?;
    tx.execute(
        "INSERT OR IGNORE INTO node_tags (node_id, tag_id) VALUES (?1, ?2)",
        params![node, tag],
    )?;
    reindex(&tx, node)?;
    tx.commit()
}

/// Retire une étiquette d'un élément ; l'étiquette disparaît si plus personne ne l'utilise.
pub fn remove(conn: &Connection, node: i64, tag: i64) -> DbResult<()> {
    let tx = Atomic::begin(conn)?;
    tx.execute(
        "DELETE FROM node_tags WHERE node_id = ?1 AND tag_id = ?2",
        params![node, tag],
    )?;
    tx.execute(
        "DELETE FROM tags WHERE id = ?1 AND id NOT IN (SELECT tag_id FROM node_tags)",
        [tag],
    )?;
    reindex(&tx, node)?;
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{nodes, open_in_memory};

    #[test]
    fn tags_lifecycle_search_and_cascade() {
        let conn = open_in_memory().unwrap();
        let a = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        let b = nodes::create(&conn, NodeKind::Place, "Bar").unwrap();
        add(&conn, a, "  #Famille  ").unwrap();
        add(&conn, a, "famille").unwrap(); // même étiquette (casse ignorée)
        add(&conn, b, "Famille").unwrap();
        add(&conn, a, "saison 1").unwrap();
        assert!(add(&conn, a, "  #  ").is_err());
        assert_eq!(for_node(&conn, a).unwrap().len(), 2);
        assert_eq!(all(&conn).unwrap().len(), 2);
        let fam = for_node(&conn, a).unwrap()[0].0;
        assert_eq!(node_ids(&conn, fam).unwrap().len(), 2);
        assert_eq!(nodes::search_text(&conn, "famille").unwrap().len(), 2); // retrouvées par la recherche
        remove(&conn, a, fam).unwrap();
        assert_eq!(all(&conn).unwrap().len(), 2); // encore utilisée par Bar
        let fam = for_node(&conn, b).unwrap()[0].0;
        remove(&conn, b, fam).unwrap();
        assert_eq!(all(&conn).unwrap().len(), 1); // orpheline : supprimée
        nodes::delete(&conn, a).unwrap();
        assert!(all(&conn).unwrap().is_empty());
    }
}
