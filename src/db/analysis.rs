//! Analyse de l'écriture : équilibre d'une saison, suivi des gags, frise de saison, statistiques.

use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection};

use super::{plots, timeline, DbResult};

/// Nombre de mots d'un texte, en SQL (mots séparés par des espaces).
const WORDS_SQL: &str =
    "CASE WHEN TRIM(b.text) = '' THEN 0 ELSE LENGTH(TRIM(b.text)) - LENGTH(REPLACE(TRIM(b.text), ' ', '')) + 1 END";

#[derive(Debug, Clone, PartialEq)]
pub struct EpisodeCol {
    pub id: i64,
    pub season: i64,
    pub number: i64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatrixRow {
    pub id: i64,
    pub title: String,
    /// Une valeur par colonne (épisode).
    pub values: Vec<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Matrix {
    pub columns: Vec<EpisodeCol>,
    pub rows: Vec<MatrixRow>,
}

/// Épisodes d'une saison, dans l'ordre.
pub fn season_columns(conn: &Connection, season: i64) -> DbResult<Vec<EpisodeCol>> {
    let mut stmt = conn.prepare(
        "SELECT ep.id, ep.season, ep.number, n.title FROM episodes ep JOIN nodes n ON n.id = ep.id
         WHERE ep.season = ?1 ORDER BY ep.number, ep.id",
    )?;
    let rows = stmt.query_map([season], |r| {
        Ok(EpisodeCol {
            id: r.get(0)?,
            season: r.get(1)?,
            number: r.get(2)?,
            title: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Assemble une matrice à partir de triplets (ligne, épisode, valeur) ; lignes triées par total décroissant.
fn build(
    columns: Vec<EpisodeCol>,
    titles: HashMap<i64, String>,
    cells: Vec<(i64, i64, i64)>,
) -> Matrix {
    let index: HashMap<i64, usize> = columns.iter().enumerate().map(|(i, c)| (c.id, i)).collect();
    let mut rows: HashMap<i64, Vec<i64>> = HashMap::new();
    for (row, ep, v) in cells {
        if let Some(&i) = index.get(&ep) {
            rows.entry(row).or_insert_with(|| vec![0; columns.len()])[i] += v;
        }
    }
    let mut rows: Vec<MatrixRow> = rows
        .into_iter()
        .map(|(id, values)| MatrixRow {
            title: titles.get(&id).cloned().unwrap_or_default(),
            id,
            values,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.values
            .iter()
            .sum::<i64>()
            .cmp(&a.values.iter().sum::<i64>())
            .then(a.title.cmp(&b.title))
    });
    Matrix { columns, rows }
}

fn titles(conn: &Connection, ids: &[i64]) -> DbResult<HashMap<i64, String>> {
    let mut out = HashMap::new();
    let mut stmt = conn.prepare("SELECT title FROM nodes WHERE id = ?1")?;
    for id in ids {
        if let Ok(t) = stmt.query_row([id], |r| r.get::<_, String>(0)) {
            out.insert(*id, t);
        }
    }
    Ok(out)
}

/// Temps de parole (en mots) de chaque personnage, épisode par épisode.
pub fn speech_matrix(conn: &Connection, season: i64) -> DbResult<Matrix> {
    let columns = season_columns(conn, season)?;
    let sql = format!(
        "SELECT b.character_id, sc.episode_id, SUM({WORDS_SQL})
         FROM scene_blocks b JOIN scenes sc ON sc.id = b.scene_id JOIN episodes ep ON ep.id = sc.episode_id
         WHERE b.block_type = 'dialogue' AND b.character_id IS NOT NULL AND ep.season = ?1
         GROUP BY b.character_id, sc.episode_id"
    );
    let mut stmt = conn.prepare(&sql)?;
    let cells: Vec<(i64, i64, i64)> = stmt
        .query_map([season], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let ids: Vec<i64> = cells
        .iter()
        .map(|c| c.0)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    Ok(build(columns, titles(conn, &ids)?, cells))
}

/// Nombre de scènes par lieu (lien « se déroule dans »), épisode par épisode.
pub fn places_matrix(conn: &Connection, season: i64) -> DbResult<Matrix> {
    let columns = season_columns(conn, season)?;
    let mut stmt = conn.prepare(
        "SELECT t.place, t.episode, COUNT(DISTINCT t.scene) FROM (
             SELECT l.dst_id AS place, sc.episode_id AS episode, sc.id AS scene
               FROM scenes sc JOIN links l ON l.src_id = sc.id AND l.link_type = 'set_in'
             UNION ALL
             SELECT l.src_id, sc.episode_id, sc.id
               FROM scenes sc JOIN links l ON l.dst_id = sc.id AND l.link_type = 'set_in'
         ) t
         JOIN nodes n ON n.id = t.place AND n.kind = 'place'
         JOIN episodes ep ON ep.id = t.episode
         WHERE ep.season = ?1 GROUP BY t.place, t.episode",
    )?;
    let cells: Vec<(i64, i64, i64)> = stmt
        .query_map([season], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<_, _>>()?;
    let ids: Vec<i64> = cells
        .iter()
        .map(|c| c.0)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    Ok(build(columns, titles(conn, &ids)?, cells))
}

/// Présence (1/0) de chaque intrigue dans les épisodes de la saison.
pub fn plots_matrix(conn: &Connection, season: i64) -> DbResult<Matrix> {
    let columns = season_columns(conn, season)?;
    let mut cells = Vec::new();
    let mut names = HashMap::new();
    for (plot, title) in plots::of_season(conn, season)? {
        for (ep, ..) in plots::appearances(conn, plot)? {
            cells.push((plot, ep, 1));
        }
        names.insert(plot, title);
    }
    Ok(build(columns, names, cells))
}

#[derive(Debug, Clone)]
pub struct GagRow {
    pub id: i64,
    pub title: String,
    /// Une case par épisode (ordre de diffusion) : le gag y apparaît-il ?
    pub uses: Vec<bool>,
    pub count: usize,
    /// Nombre d'épisodes écoulés depuis la dernière apparition (None = jamais utilisé).
    pub since: Option<usize>,
}

pub struct GagReport {
    pub columns: Vec<EpisodeCol>,
    pub gags: Vec<GagRow>,
}

/// Suivi des running gags sur toute la série, dans l'ordre de diffusion.
pub fn gag_report(conn: &Connection) -> DbResult<GagReport> {
    let columns: Vec<EpisodeCol> = timeline::list_air(conn)?
        .into_iter()
        .map(|a| EpisodeCol {
            id: a.id,
            season: a.season,
            number: a.number,
            title: a.title,
        })
        .collect();
    let mut stmt = conn.prepare(
        "SELECT p.id, n.title FROM plots p JOIN nodes n ON n.id = p.id
         WHERE p.plot_type = 'running_gag' ORDER BY n.title COLLATE NOCASE",
    )?;
    let gags_raw: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut gags = Vec::new();
    for (id, title) in gags_raw {
        let seen: HashSet<i64> = plots::appearances(conn, id)?
            .into_iter()
            .map(|a| a.0)
            .collect();
        let uses: Vec<bool> = columns.iter().map(|c| seen.contains(&c.id)).collect();
        let last = uses.iter().rposition(|u| *u);
        gags.push(GagRow {
            id,
            title,
            count: uses.iter().filter(|u| **u).count(),
            since: last.map(|l| uses.len() - 1 - l),
            uses,
        });
    }
    Ok(GagReport { columns, gags })
}

#[derive(Debug, Clone)]
pub struct SeasonCard {
    pub episode: EpisodeCol,
    pub status: String,
    pub scenes: i64,
    pub words: i64,
    pub plots: Vec<(i64, String)>,
}

/// Une « carte » par épisode de la saison, pour la frise.
pub fn season_cards(conn: &Connection, season: i64) -> DbResult<Vec<SeasonCard>> {
    let season_plots = plots::of_season(conn, season)?;
    let mut appears: HashMap<i64, Vec<(i64, String)>> = HashMap::new();
    for (plot, title) in &season_plots {
        for (ep, ..) in plots::appearances(conn, *plot)? {
            appears.entry(ep).or_default().push((*plot, title.clone()));
        }
    }
    let words_sql = format!(
        "SELECT COALESCE(SUM({WORDS_SQL}), 0) FROM scene_blocks b JOIN scenes sc ON sc.id = b.scene_id WHERE sc.episode_id = ?1"
    );
    let mut out = Vec::new();
    for episode in season_columns(conn, season)? {
        let status: String = conn.query_row(
            "SELECT status FROM episodes WHERE id = ?1",
            [episode.id],
            |r| r.get(0),
        )?;
        let scenes: i64 = conn.query_row(
            "SELECT COUNT(*) FROM scenes WHERE episode_id = ?1",
            [episode.id],
            |r| r.get(0),
        )?;
        let words: i64 = conn.query_row(&words_sql, [episode.id], |r| r.get(0))?;
        let plots = appears.remove(&episode.id).unwrap_or_default();
        out.push(SeasonCard {
            episode,
            status,
            scenes,
            words,
            plots,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub episodes: i64,
    pub by_status: Vec<(String, i64)>,
    pub scenes: i64,
    pub lines: i64,
    pub words: i64,
    pub characters: i64,
    pub words_per_episode: Vec<(EpisodeCol, i64)>,
    /// (« AAAA-MM », éléments créés), 12 derniers mois d'activité, du plus ancien au plus récent.
    pub created_per_month: Vec<(String, i64)>,
}

pub fn stats(conn: &Connection) -> DbResult<Stats> {
    let count = |sql: &str| -> DbResult<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
    let mut s = Stats {
        episodes: count("SELECT COUNT(*) FROM episodes")?,
        scenes: count("SELECT COUNT(*) FROM scenes")?,
        lines: count("SELECT COUNT(*) FROM scene_blocks WHERE block_type = 'dialogue'")?,
        characters: count("SELECT COUNT(*) FROM nodes WHERE kind = 'character'")?,
        ..Default::default()
    };
    s.words = conn.query_row(
        &format!("SELECT COALESCE(SUM({WORDS_SQL}), 0) FROM scene_blocks b"),
        [],
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare("SELECT status, COUNT(*) FROM episodes GROUP BY status")?;
    s.by_status = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let words_sql = format!(
        "SELECT COALESCE(SUM({WORDS_SQL}), 0) FROM scene_blocks b JOIN scenes sc ON sc.id = b.scene_id WHERE sc.episode_id = ?1"
    );
    for a in timeline::list_air(conn)? {
        let w: i64 = conn.query_row(&words_sql, params![a.id], |r| r.get(0))?;
        s.words_per_episode.push((
            EpisodeCol {
                id: a.id,
                season: a.season,
                number: a.number,
                title: a.title,
            },
            w,
        ));
    }
    let mut stmt = conn.prepare(
        "SELECT SUBSTR(created_at, 1, 7) AS m, COUNT(*) FROM nodes GROUP BY m ORDER BY m DESC LIMIT 12",
    )?;
    let mut months: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    months.reverse();
    s.created_per_month = months;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{
        links, nodes,
        nodes::NodeKind,
        open_in_memory,
        script::{self, BlockType},
    };

    fn setup() -> (Connection, i64, i64, i64, i64) {
        let conn = open_in_memory().unwrap();
        let alice = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        let bob = nodes::create(&conn, NodeKind::Character, "Bob").unwrap();
        let e1 = nodes::create_episode(&conn, "E1", 1, 1).unwrap();
        let e2 = nodes::create_episode(&conn, "E2", 1, 2).unwrap();
        let _e3 = nodes::create_episode(&conn, "E3", 2, 1).unwrap();
        let s1 = nodes::create_scene(&conn, e1, "cuisine", 1).unwrap();
        let s2 = nodes::create_scene(&conn, e2, "salon", 1).unwrap();
        script::add(
            &conn,
            s1,
            BlockType::Dialogue,
            Some(alice),
            "Bonjour tout le monde",
        )
        .unwrap();
        script::add(&conn, s1, BlockType::Dialogue, Some(bob), "Salut").unwrap();
        script::add(
            &conn,
            s2,
            BlockType::Dialogue,
            Some(alice),
            "Encore moi ici",
        )
        .unwrap();
        script::add(&conn, s2, BlockType::Action, None, "Il pleut").unwrap();
        (conn, alice, bob, e1, e2)
    }

    #[test]
    fn speech_places_and_plots_matrices() {
        let (conn, alice, bob, e1, e2) = setup();
        let m = speech_matrix(&conn, 1).unwrap();
        assert_eq!(
            m.columns.iter().map(|c| c.id).collect::<Vec<_>>(),
            vec![e1, e2]
        );
        assert_eq!(
            m.rows[0],
            MatrixRow {
                id: alice,
                title: "Alice".into(),
                values: vec![4, 3]
            }
        );
        assert_eq!(
            m.rows[1],
            MatrixRow {
                id: bob,
                title: "Bob".into(),
                values: vec![1, 0]
            }
        );
        assert!(speech_matrix(&conn, 2).unwrap().rows.is_empty());

        let kitchen = nodes::create(&conn, NodeKind::Place, "Cuisine").unwrap();
        let scenes = timeline::list_scenes(&conn).unwrap();
        links::create(&conn, scenes[0].id, kitchen, "set_in", None, None).unwrap();
        let p = places_matrix(&conn, 1).unwrap();
        assert_eq!(
            p.rows,
            vec![MatrixRow {
                id: kitchen,
                title: "Cuisine".into(),
                values: vec![1, 0]
            }]
        );

        let arc = nodes::create(&conn, NodeKind::Plot, "Mariage").unwrap();
        links::create(&conn, arc, e2, "advances", None, None).unwrap();
        let pm = plots_matrix(&conn, 1).unwrap();
        assert_eq!(pm.rows[0].values, vec![0, 1]);
    }

    #[test]
    fn gag_report_counts_and_gaps() {
        let (conn, _a, _b, e1, _e2) = setup();
        let gag = nodes::create(&conn, NodeKind::Plot, "La porte coince").unwrap();
        plots::set_gag(&conn, gag).unwrap();
        let never = nodes::create(&conn, NodeKind::Plot, "Jamais").unwrap();
        plots::set_gag(&conn, never).unwrap();
        links::create(&conn, gag, e1, "advances", None, None).unwrap();
        let r = gag_report(&conn).unwrap();
        assert_eq!(r.columns.len(), 3);
        let g = r.gags.iter().find(|g| g.id == gag).unwrap();
        assert_eq!(
            (g.uses.clone(), g.count, g.since),
            (vec![true, false, false], 1, Some(2))
        );
        assert_eq!(r.gags.iter().find(|g| g.id == never).unwrap().since, None);
    }

    #[test]
    fn cards_and_stats() {
        let (conn, _a, _b, e1, _e2) = setup();
        let arc = nodes::create(&conn, NodeKind::Plot, "Mariage").unwrap();
        links::create(&conn, arc, e1, "advances", None, None).unwrap();
        let cards = season_cards(&conn, 1).unwrap();
        assert_eq!(cards.len(), 2);
        assert_eq!((cards[0].scenes, cards[0].words), (1, 5));
        assert_eq!(cards[0].plots, vec![(arc, "Mariage".to_string())]);
        assert!(cards[1].plots.is_empty());
        let s = stats(&conn).unwrap();
        assert_eq!((s.episodes, s.scenes, s.lines, s.characters), (3, 2, 3, 2));
        assert_eq!(s.words, 5 + 3 + 2);
        assert_eq!(s.words_per_episode.len(), 3);
        assert!(!s.created_per_month.is_empty());
        assert!(s.by_status.iter().map(|x| x.1).sum::<i64>() == 3);
    }
}
