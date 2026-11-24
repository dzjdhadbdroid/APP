//! Intrigues, arcs et gags récurrents : où apparaissent-ils ?

use rusqlite::Connection;

use super::DbResult;

/// Épisodes où l'intrigue apparaît : reliés directement, ou par l'une de leurs scènes.
/// `(id, titre, saison, numéro)`, dans l'ordre de la série.
pub fn appearances(conn: &Connection, plot: i64) -> DbResult<Vec<(i64, String, i64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT ep.id, n.title, ep.season, ep.number
         FROM episodes ep JOIN nodes n ON n.id = ep.id
         WHERE ep.id IN (
               SELECT CASE WHEN l.src_id = ?1 THEN l.dst_id ELSE l.src_id END
               FROM links l WHERE l.src_id = ?1 OR l.dst_id = ?1
               UNION
               SELECT s.episode_id FROM scenes s JOIN links l
                 ON (l.src_id = ?1 AND l.dst_id = s.id) OR (l.dst_id = ?1 AND l.src_id = s.id))
         ORDER BY ep.season, ep.number",
    )?;
    let rows = stmt.query_map([plot], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Intrigues présentes dans une saison (via ses épisodes ou leurs scènes), par titre.
pub fn of_season(conn: &Connection, season: i64) -> DbResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.title FROM nodes p
         WHERE p.kind = 'plot' AND (
             EXISTS (SELECT 1 FROM links l JOIN episodes ep
                       ON ep.id = CASE WHEN l.src_id = p.id THEN l.dst_id ELSE l.src_id END
                     WHERE (l.src_id = p.id OR l.dst_id = p.id) AND ep.season = ?1)
          OR EXISTS (SELECT 1 FROM links l JOIN scenes s
                       ON s.id = CASE WHEN l.src_id = p.id THEN l.dst_id ELSE l.src_id END
                     JOIN episodes ep ON ep.id = s.episode_id
                     WHERE (l.src_id = p.id OR l.dst_id = p.id) AND ep.season = ?1))
         ORDER BY p.title COLLATE NOCASE",
    )?;
    let rows = stmt.query_map([season], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Intrigues dont le type est « running gag ».
pub fn gag_ids(conn: &Connection) -> DbResult<std::collections::HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM plots WHERE plot_type = 'running_gag'")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn set_gag(conn: &Connection, plot: i64) -> DbResult<()> {
    conn.execute(
        "UPDATE plots SET plot_type = 'running_gag' WHERE id = ?1",
        [plot],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{links, nodes, nodes::NodeKind, open_in_memory};

    #[test]
    fn gag_appears_in_linked_episodes_and_scenes() {
        let conn = open_in_memory().unwrap();
        let gag = nodes::create(&conn, NodeKind::Plot, "La porte coince").unwrap();
        let arc = nodes::create(&conn, NodeKind::Plot, "Mariage").unwrap();
        set_gag(&conn, gag).unwrap();
        let e1 = nodes::create_episode(&conn, "E1", 1, 1).unwrap();
        let e2 = nodes::create_episode(&conn, "E2", 1, 2).unwrap();
        let e3 = nodes::create_episode(&conn, "E3", 2, 1).unwrap();
        let s2 = nodes::create_scene(&conn, e2, "s", 1).unwrap();
        links::create(&conn, gag, e1, "advances", None, None).unwrap();
        links::create(&conn, s2, gag, "advances", None, None).unwrap();
        links::create(&conn, arc, e3, "advances", None, None).unwrap();
        let ids: Vec<i64> = appearances(&conn, gag)
            .unwrap()
            .into_iter()
            .map(|a| a.0)
            .collect();
        assert_eq!(ids, vec![e1, e2]);
        assert_eq!(gag_ids(&conn).unwrap().len(), 1);
        assert_eq!(
            of_season(&conn, 1).unwrap(),
            vec![(gag, "La porte coince".to_string())]
        );
        assert_eq!(
            of_season(&conn, 2).unwrap(),
            vec![(arc, "Mariage".to_string())]
        );
    }
}
