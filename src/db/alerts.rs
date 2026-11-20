//! Alertes de cohérence : calculées par `consistency`, stockées pour rester ignorables.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};

use super::{Atomic, DbError, DbResult};
use crate::consistency::{self, Facts, KnowledgeFact, LinkFact, NodeFact};

#[derive(Debug, Clone)]
pub struct AlertRow {
    pub id: i64,
    pub severity: String,
    pub message: String,
    pub node_id: Option<i64>,
    pub status: String,
}

struct Loaded {
    nodes: Vec<NodeFact>,
    event_dates: HashMap<i64, Option<i64>>,
    air_order: HashMap<i64, i64>,
    place_parent: HashMap<i64, i64>,
    links: Vec<LinkFact>,
    knowledge: Vec<KnowledgeFact>,
}

fn load_facts(conn: &Connection) -> DbResult<Loaded> {
    let nodes = conn
        .prepare("SELECT id, title, kind FROM nodes")?
        .query_map([], |r| Ok(NodeFact { id: r.get(0)?, title: r.get(1)?, kind: r.get(2)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    let event_dates = conn
        .prepare("SELECT id, story_sort FROM events")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<i64>>(1)?)))?
        .collect::<Result<HashMap<_, _>, _>>()?;
    let air_order = conn
        .prepare("SELECT id, air_order FROM episodes WHERE air_order IS NOT NULL")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<HashMap<_, _>, _>>()?;
    let place_parent = conn
        .prepare("SELECT id, parent_id FROM places WHERE parent_id IS NOT NULL")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<Result<HashMap<_, _>, _>>()?;
    let links = conn
        .prepare("SELECT src_id, dst_id, link_type, label FROM links")?
        .query_map([], |r| {
            Ok(LinkFact { src: r.get(0)?, dst: r.get(1)?, link_type: r.get(2)?, label: r.get(3)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let knowledge = conn
        .prepare("SELECT character_id, subject_id, learned_event_id FROM knowledge")?
        .query_map([], |r| {
            Ok(KnowledgeFact { character: r.get(0)?, subject: r.get(1)?, learned_event: r.get(2)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Loaded { nodes, event_dates, air_order, place_parent, links, knowledge })
}

/// Recalcule les alertes : les nouvelles sont ajoutées, celles qui ne s'appliquent plus
/// disparaissent, et celles que le scénariste a ignorées restent ignorées.
pub fn refresh(conn: &Connection) -> DbResult<()> {
    let d = load_facts(conn)?;
    let findings = consistency::analyze(&Facts {
        nodes: &d.nodes,
        event_dates: &d.event_dates,
        air_order: &d.air_order,
        place_parent: &d.place_parent,
        links: &d.links,
        knowledge: &d.knowledge,
    });
    let wanted: HashSet<(String, i64, String)> = findings
        .iter()
        .map(|f| (f.code.to_string(), f.node_id, f.message.clone()))
        .collect();

    let tx = Atomic::begin(conn)?;
    let existing: Vec<(i64, String, Option<i64>, String)> = tx
        .prepare("SELECT id, code, node_id, message FROM alerts")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    let mut kept = HashSet::new();
    for (id, code, node_id, message) in existing {
        let key = (code, node_id.unwrap_or(-1), message);
        if wanted.contains(&key) {
            kept.insert(key);
        } else {
            tx.execute("DELETE FROM alerts WHERE id = ?1", [id])?;
        }
    }
    for f in &findings {
        let key = (f.code.to_string(), f.node_id, f.message.clone());
        if !kept.contains(&key) {
            tx.execute(
                "INSERT OR IGNORE INTO alerts (code, severity, message, node_id) VALUES (?1, ?2, ?3, ?4)",
                params![f.code, f.severity.as_str(), f.message, f.node_id],
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Alertes triées par gravité décroissante.
pub fn list(conn: &Connection) -> DbResult<Vec<AlertRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, severity, message, node_id, status FROM alerts
         ORDER BY CASE severity WHEN 'error' THEN 0 WHEN 'warning' THEN 1 ELSE 2 END, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AlertRow {
            id: r.get(0)?,
            severity: r.get(1)?,
            message: r.get(2)?,
            node_id: r.get(3)?,
            status: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn set_status(conn: &Connection, id: i64, status: &str) -> DbResult<()> {
    if !["open", "ignored", "resolved"].contains(&status) {
        return Err(DbError::Invalid(format!("statut « {status} »")));
    }
    conn.execute("UPDATE alerts SET status = ?2 WHERE id = ?1", params![id, status])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{knowledge, links, nodes, nodes::NodeKind, open_in_memory, timeline};

    /// Alice doit connaître le secret pour le dîner (an 100) ; elle l'apprend à la révélation.
    fn setup(conn: &Connection, reveal_date: &str) -> (i64, i64, i64) {
        let alice = nodes::create(conn, NodeKind::Character, "Alice").unwrap();
        let secret = nodes::create(conn, NodeKind::Plot, "Le secret").unwrap();
        let dinner = nodes::create(conn, NodeKind::Event, "Dîner").unwrap();
        let reveal = nodes::create(conn, NodeKind::Event, "Révélation").unwrap();
        timeline::set_event(conn, dinner, "100", 3, "planned").unwrap();
        timeline::set_event(conn, reveal, reveal_date, 3, "planned").unwrap();
        links::create(conn, dinner, secret, "requires", None, None).unwrap();
        links::create(conn, dinner, alice, "involves", None, None).unwrap();
        knowledge::add(conn, alice, secret, Some(reveal)).unwrap();
        (alice, dinner, reveal)
    }

    #[test]
    fn alert_appears_is_ignorable_and_disappears_when_fixed() {
        let conn = open_in_memory().unwrap();
        let (_, _, reveal) = setup(&conn, "200"); // révélation APRÈS le dîner
        refresh(&conn).unwrap();
        let alerts = list(&conn).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, "error");

        set_status(&conn, alerts[0].id, "ignored").unwrap();
        refresh(&conn).unwrap();
        assert_eq!(list(&conn).unwrap()[0].status, "ignored"); // l'ignorance survit au recalcul

        timeline::set_event(&conn, reveal, "50", 3, "planned").unwrap(); // corrigé
        refresh(&conn).unwrap();
        assert!(list(&conn).unwrap().is_empty());
    }

    #[test]
    fn refresh_is_idempotent() {
        let conn = open_in_memory().unwrap();
        setup(&conn, "200");
        refresh(&conn).unwrap();
        refresh(&conn).unwrap();
        assert_eq!(list(&conn).unwrap().len(), 1);
    }
}
