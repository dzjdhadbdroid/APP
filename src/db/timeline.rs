//! Requêtes de la Story Timeline. Trois ordres distincts, une seule source de données :
//! - réel (univers fictif) : `events.story_sort`
//! - diffusion : `episodes.air_order`
//! - présentation : `scenes.position` au sein d'un épisode

use rusqlite::{params, Connection};

use super::{Atomic, DbError, DbResult};
use crate::story_date;

#[derive(Debug, Clone)]
pub struct EventRow {
    pub id: i64,
    pub title: String,
    pub story_start: Option<String>,
    pub story_sort: Option<i64>,
    pub importance: i64,
    pub status: String,
}

pub const STATUSES: [&str; 3] = ["planned", "confirmed", "modified"];

pub fn status_label(s: &str) -> &'static str {
    match s {
        "planned" => "prévu",
        "confirmed" => "confirmé",
        "modified" => "modifié",
        _ => "?",
    }
}

/// Événements dans l'ordre chronologique de l'univers (sans date : à la fin).
/// `linked_to` restreint aux événements reliés à un élément (personnage, intrigue, lieu…),
/// ce qui donne les chronologies individuelle, d'intrigue, etc.
pub fn list_events(conn: &Connection, linked_to: Option<i64>) -> DbResult<Vec<EventRow>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, e.story_start, e.story_sort, e.importance, e.status
         FROM events e JOIN nodes n ON n.id = e.id
         WHERE ?1 IS NULL OR EXISTS (
               SELECT 1 FROM links l
               WHERE (l.src_id = n.id AND l.dst_id = ?1) OR (l.dst_id = n.id AND l.src_id = ?1))
         ORDER BY e.story_sort IS NULL, e.story_sort, n.id",
    )?;
    let rows = stmt.query_map([linked_to], |r| {
        Ok(EventRow {
            id: r.get(0)?,
            title: r.get(1)?,
            story_start: r.get(2)?,
            story_sort: r.get(3)?,
            importance: r.get(4)?,
            status: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Fixe la date fictive (vide = non datée), l'importance (1–5) et le statut d'un événement.
pub fn set_event(
    conn: &Connection,
    id: i64,
    start: &str,
    importance: i64,
    status: &str,
) -> DbResult<()> {
    let start = start.trim();
    let sort = if start.is_empty() {
        None
    } else {
        Some(story_date::sort_key(start).ok_or_else(|| {
            DbError::Invalid(format!("date « {start} » (formats : AAAA, AAAA-MM, AAAA-MM-JJ)"))
        })?)
    };
    if !STATUSES.contains(&status) {
        return Err(DbError::Invalid(format!("statut « {status} »")));
    }
    let n = conn.execute(
        "UPDATE events SET story_start = ?2, story_sort = ?3, importance = ?4, status = ?5
         WHERE id = ?1",
        params![id, (!start.is_empty()).then_some(start), sort, importance.clamp(1, 5), status],
    )?;
    if n == 0 {
        return Err(DbError::NotFound(id));
    }
    Ok(())
}

/// Numéros de saison existants, croissants.
pub fn seasons(conn: &Connection) -> DbResult<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT DISTINCT season FROM episodes ORDER BY season")?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Événements reliés (dans un sens ou dans l'autre) à un épisode de la saison, ordre chronologique.
pub fn events_of_season(conn: &Connection, season: i64) -> DbResult<Vec<EventRow>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, e.story_start, e.story_sort, e.importance, e.status
         FROM events e JOIN nodes n ON n.id = e.id
         WHERE EXISTS (
               SELECT 1 FROM links l JOIN episodes ep
                 ON ep.id = CASE WHEN l.src_id = n.id THEN l.dst_id ELSE l.src_id END
               WHERE (l.src_id = n.id OR l.dst_id = n.id) AND ep.season = ?1)
         ORDER BY e.story_sort IS NULL, e.story_sort, n.id",
    )?;
    let rows = stmt.query_map([season], |r| {
        Ok(EventRow {
            id: r.get(0)?,
            title: r.get(1)?,
            story_start: r.get(2)?,
            story_sort: r.get(3)?,
            importance: r.get(4)?,
            status: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Garde les événements dont la date tombe dans [from, to] (bornes AAAA[-MM[-JJ]], vides = ouvertes).
/// Les événements non datés sont écartés dès qu'une borne est donnée.
pub fn in_period(rows: Vec<EventRow>, from: &str, to: &str) -> DbResult<Vec<EventRow>> {
    let parse = |s: &str| -> DbResult<Option<i64>> {
        if s.trim().is_empty() {
            return Ok(None);
        }
        story_date::sort_key(s)
            .map(Some)
            .ok_or_else(|| DbError::Invalid(format!("date « {s} » (formats : AAAA, AAAA-MM, AAAA-MM-JJ)")))
    };
    let (lo, hi) = (parse(from)?, parse(to)?);
    if lo.is_none() && hi.is_none() {
        return Ok(rows);
    }
    Ok(rows
        .into_iter()
        .filter(|r| {
            r.story_sort
                .is_some_and(|k| lo.map_or(true, |l| k >= l) && hi.map_or(true, |h| k <= h))
        })
        .collect())
}

#[derive(Debug, Clone)]
pub struct AirRow {
    pub id: i64,
    pub title: String,
    pub season: i64,
    pub number: i64,
    pub air_order: Option<i64>,
}

/// Épisodes dans l'ordre de diffusion.
pub fn list_air(conn: &Connection) -> DbResult<Vec<AirRow>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, ep.season, ep.number, ep.air_order
         FROM episodes ep JOIN nodes n ON n.id = ep.id
         ORDER BY ep.air_order IS NULL, ep.air_order, ep.season, ep.number",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AirRow {
            id: r.get(0)?,
            title: r.get(1)?,
            season: r.get(2)?,
            number: r.get(3)?,
            air_order: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Échange l'ordre de diffusion de deux épisodes (déplacement dans la chronologie).
pub fn swap_air_order(conn: &Connection, a: i64, b: i64) -> DbResult<()> {
    let tx = Atomic::begin(conn)?;
    let get = |id: i64| -> DbResult<i64> {
        tx.query_row("SELECT air_order FROM episodes WHERE id = ?1", [id], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(id),
            other => other.into(),
        })?
        .ok_or_else(|| DbError::Invalid("épisode sans ordre de diffusion".into()))
    };
    let (oa, ob) = (get(a)?, get(b)?);
    tx.execute("UPDATE episodes SET air_order = ?2 WHERE id = ?1", params![a, ob])?;
    tx.execute("UPDATE episodes SET air_order = ?2 WHERE id = ?1", params![b, oa])?;
    tx.commit()?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct SceneRow {
    pub id: i64,
    pub episode_title: String,
    pub position: i64,
    pub title: String,
}

/// Scènes dans l'ordre de présentation (épisodes en ordre de diffusion).
pub fn list_scenes(conn: &Connection) -> DbResult<Vec<SceneRow>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, en.title, s.position, n.title
         FROM scenes s
         JOIN nodes n ON n.id = s.id
         JOIN episodes ep ON ep.id = s.episode_id
         JOIN nodes en ON en.id = ep.id
         ORDER BY ep.air_order IS NULL, ep.air_order, ep.season, ep.number, s.position",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SceneRow {
            id: r.get(0)?,
            episode_title: r.get(1)?,
            position: r.get(2)?,
            title: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{links, nodes, nodes::NodeKind, open_in_memory};

    #[test]
    fn events_are_sorted_and_undated_come_last() {
        let conn = open_in_memory().unwrap();
        let late = nodes::create(&conn, NodeKind::Event, "tard").unwrap();
        let none = nodes::create(&conn, NodeKind::Event, "sans date").unwrap();
        let early = nodes::create(&conn, NodeKind::Event, "tôt").unwrap();
        set_event(&conn, late, "2001-05", 3, "planned").unwrap();
        set_event(&conn, early, "1998-06-12", 5, "confirmed").unwrap();
        let titles: Vec<_> = list_events(&conn, None).unwrap().into_iter().map(|e| e.title).collect();
        assert_eq!(titles, ["tôt", "tard", "sans date"]);
        let _ = none;
    }

    #[test]
    fn invalid_date_or_status_is_rejected() {
        let conn = open_in_memory().unwrap();
        let e = nodes::create(&conn, NodeKind::Event, "x").unwrap();
        assert!(set_event(&conn, e, "pas une date", 3, "planned").is_err());
        assert!(set_event(&conn, e, "2000", 3, "inconnu").is_err());
    }

    #[test]
    fn individual_timeline_filters_by_linked_element() {
        let conn = open_in_memory().unwrap();
        let alice = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        let e1 = nodes::create(&conn, NodeKind::Event, "e1").unwrap();
        nodes::create(&conn, NodeKind::Event, "e2").unwrap();
        links::create(&conn, e1, alice, "involves", None, None).unwrap();
        let rows = list_events(&conn, Some(alice)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "e1");
    }

    #[test]
    fn air_order_and_scene_order_are_independent() {
        let conn = open_in_memory().unwrap();
        let e1 = nodes::create_episode(&conn, "E1", 1, 1).unwrap();
        let e2 = nodes::create_episode(&conn, "E2", 1, 2).unwrap();
        nodes::create_scene(&conn, e1, "s1", 1).unwrap();
        swap_air_order(&conn, e1, e2).unwrap();
        let air: Vec<_> = list_air(&conn).unwrap().into_iter().map(|r| r.title).collect();
        assert_eq!(air, ["E2", "E1"]);
        assert_eq!(list_scenes(&conn).unwrap().len(), 1);
    }

    #[test]
    fn season_and_period_views() {
        let conn = open_in_memory().unwrap();
        let ep1 = nodes::create_episode(&conn, "E1", 1, 1).unwrap();
        let ep2 = nodes::create_episode(&conn, "E2", 2, 1).unwrap();
        let a = nodes::create(&conn, NodeKind::Event, "a").unwrap();
        let b = nodes::create(&conn, NodeKind::Event, "b").unwrap();
        let c = nodes::create(&conn, NodeKind::Event, "c").unwrap();
        set_event(&conn, a, "1998-03", 3, "planned").unwrap();
        set_event(&conn, b, "2001", 3, "planned").unwrap();
        links::create(&conn, a, ep1, "mentions", None, None).unwrap();
        links::create(&conn, ep2, b, "involves", None, None).unwrap();
        assert_eq!(seasons(&conn).unwrap(), vec![1, 2]);
        let s1: Vec<_> = events_of_season(&conn, 1).unwrap().into_iter().map(|e| e.title).collect();
        let s2: Vec<_> = events_of_season(&conn, 2).unwrap().into_iter().map(|e| e.title).collect();
        assert_eq!((s1, s2), (vec!["a".to_string()], vec!["b".to_string()]));
        let all = list_events(&conn, None).unwrap();
        assert_eq!(all.len(), 3);
        let p = in_period(all.clone(), "1998", "1999-12").unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!(in_period(all.clone(), "", "").unwrap().len(), 3);
        assert_eq!(in_period(all.clone(), "1999", "").unwrap().len(), 1); // non daté écarté
        assert!(in_period(all, "n'importe quoi", "").is_err());
        let _ = c;
    }
}
