//! Scénario d'une scène : actes, séquences et blocs (action, dialogue, mise en scène).
//! Les textes des blocs sont indexés pour la recherche globale (dialogues compris).

use rusqlite::{params, Connection};

use super::{fields, nodes::NodeKind, Atomic, DbError, DbResult};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BlockType {
    Action,
    Dialogue,
    Direction,
}

impl BlockType {
    pub const ALL: [BlockType; 3] = [BlockType::Action, BlockType::Dialogue, BlockType::Direction];

    pub fn code(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Dialogue => "dialogue",
            Self::Direction => "direction",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Action => "Action",
            Self::Dialogue => "Dialogue",
            Self::Direction => "Mise en scène",
        }
    }
    pub fn parse(code: &str) -> DbResult<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.code() == code)
            .ok_or_else(|| DbError::Invalid(format!("type de bloc « {code} »")))
    }
}

#[derive(Clone, Debug)]
pub struct Block {
    pub id: i64,
    pub position: i64,
    pub block_type: BlockType,
    pub character_id: Option<i64>,
    pub character_title: Option<String>,
    pub text: String,
}

fn scene_of(conn: &Connection, block_id: i64) -> DbResult<i64> {
    conn.query_row("SELECT scene_id FROM scene_blocks WHERE id = ?1", [block_id], |r| r.get(0))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(block_id),
            other => other.into(),
        })
}

fn reindex(conn: &Connection, scene_id: i64) -> DbResult<()> {
    fields::rebuild_body(conn, NodeKind::Scene, scene_id)
}

pub fn list(conn: &Connection, scene_id: i64) -> DbResult<Vec<Block>> {
    let mut stmt = conn.prepare(
        "SELECT b.id, b.position, b.block_type, b.character_id, n.title, b.text
         FROM scene_blocks b LEFT JOIN nodes n ON n.id = b.character_id
         WHERE b.scene_id = ?1 ORDER BY b.position",
    )?;
    let rows = stmt
        .query_map([scene_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(id, position, ty, character_id, character_title, text)| {
            Ok(Block { id, position, block_type: BlockType::parse(&ty)?, character_id, character_title, text })
        })
        .collect()
}

/// Ajoute un bloc à la fin de la scène. `character` n'a de sens que pour un dialogue.
pub fn add(
    conn: &Connection,
    scene_id: i64,
    block_type: BlockType,
    character: Option<i64>,
    text: &str,
) -> DbResult<i64> {
    let tx = Atomic::begin(conn)?;
    let position: i64 = tx.query_row(
        "SELECT COALESCE(MAX(position), 0) + 1 FROM scene_blocks WHERE scene_id = ?1",
        [scene_id],
        |r| r.get(0),
    )?;
    let character = if block_type == BlockType::Dialogue { character } else { None };
    tx.execute(
        "INSERT INTO scene_blocks (scene_id, position, block_type, character_id, text)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![scene_id, position, block_type.code(), character, text],
    )?;
    let id = tx.last_insert_rowid();
    reindex(&tx, scene_id)?;
    tx.commit()?;
    Ok(id)
}

pub fn update(
    conn: &Connection,
    block_id: i64,
    block_type: BlockType,
    character: Option<i64>,
    text: &str,
) -> DbResult<()> {
    let scene = scene_of(conn, block_id)?;
    let tx = Atomic::begin(conn)?;
    let character = if block_type == BlockType::Dialogue { character } else { None };
    tx.execute(
        "UPDATE scene_blocks SET block_type = ?2, character_id = ?3, text = ?4 WHERE id = ?1",
        params![block_id, block_type.code(), character, text],
    )?;
    reindex(&tx, scene)?;
    tx.commit()
}

pub fn delete(conn: &Connection, block_id: i64) -> DbResult<()> {
    let scene = scene_of(conn, block_id)?;
    let tx = Atomic::begin(conn)?;
    tx.execute("DELETE FROM scene_blocks WHERE id = ?1", [block_id])?;
    reindex(&tx, scene)?;
    tx.commit()
}

/// Monte ou descend un bloc d'un cran (sans effet aux extrémités).
pub fn move_block(conn: &Connection, block_id: i64, up: bool) -> DbResult<()> {
    let scene = scene_of(conn, block_id)?;
    let tx = Atomic::begin(conn)?;
    let pos: i64 = tx.query_row("SELECT position FROM scene_blocks WHERE id = ?1", [block_id], |r| r.get(0))?;
    let sql = if up {
        "SELECT id, position FROM scene_blocks WHERE scene_id = ?1 AND position < ?2 ORDER BY position DESC LIMIT 1"
    } else {
        "SELECT id, position FROM scene_blocks WHERE scene_id = ?1 AND position > ?2 ORDER BY position ASC LIMIT 1"
    };
    let neighbor: Option<(i64, i64)> = tx
        .query_row(sql, params![scene, pos], |r| Ok((r.get(0)?, r.get(1)?)))
        .map(Some)
        .or_else(|e| if matches!(e, rusqlite::Error::QueryReturnedNoRows) { Ok(None) } else { Err(e) })?;
    let Some((other, other_pos)) = neighbor else { return Ok(()) };
    // Position temporaire : la contrainte UNIQUE (scene, position) interdit un échange direct.
    tx.execute("UPDATE scene_blocks SET position = -1 WHERE id = ?1", [block_id])?;
    tx.execute("UPDATE scene_blocks SET position = ?2 WHERE id = ?1", params![other, pos])?;
    tx.execute("UPDATE scene_blocks SET position = ?2 WHERE id = ?1", params![block_id, other_pos])?;
    reindex(&tx, scene)?;
    tx.commit()
}

/// (acte, séquence) d'une scène.
pub fn structure(conn: &Connection, scene_id: i64) -> DbResult<(Option<i64>, Option<i64>)> {
    conn.query_row("SELECT act, sequence FROM scenes WHERE id = ?1", [scene_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(scene_id),
            other => other.into(),
        })
}

pub fn set_structure(conn: &Connection, scene_id: i64, act: Option<i64>, sequence: Option<i64>) -> DbResult<()> {
    let n = conn.execute("UPDATE scenes SET act = ?2, sequence = ?3 WHERE id = ?1", params![scene_id, act, sequence])?;
    if n == 0 {
        return Err(DbError::NotFound(scene_id));
    }
    conn.execute("UPDATE nodes SET title = title WHERE id = ?1", [scene_id])?; // date de modification
    Ok(())
}

#[derive(Clone, Debug)]
pub struct SceneInfo {
    pub id: i64,
    pub title: String,
    pub position: i64,
    pub act: Option<i64>,
    pub sequence: Option<i64>,
}

/// Scènes d'un épisode dans l'ordre de présentation.
pub fn scenes_of(conn: &Connection, episode_id: i64) -> DbResult<Vec<SceneInfo>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, n.title, s.position, s.act, s.sequence
         FROM scenes s JOIN nodes n ON n.id = s.id
         WHERE s.episode_id = ?1 ORDER BY s.position",
    )?;
    let rows = stmt.query_map([episode_id], |r| {
        Ok(SceneInfo { id: r.get(0)?, title: r.get(1)?, position: r.get(2)?, act: r.get(3)?, sequence: r.get(4)? })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Échange l'ordre de présentation de deux scènes du même épisode.
pub fn swap_scenes(conn: &Connection, a: i64, b: i64) -> DbResult<()> {
    let tx = Atomic::begin(conn)?;
    let get = |id: i64| -> DbResult<(i64, i64)> {
        tx.query_row("SELECT episode_id, position FROM scenes WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(id),
                other => other.into(),
            })
    };
    let ((ea, pa), (eb, pb)) = (get(a)?, get(b)?);
    if ea != eb {
        return Err(DbError::Invalid("les deux scènes ne sont pas dans le même épisode".into()));
    }
    tx.execute("UPDATE scenes SET position = -1 WHERE id = ?1", [a])?;
    tx.execute("UPDATE scenes SET position = ?2 WHERE id = ?1", params![b, pa])?;
    tx.execute("UPDATE scenes SET position = ?2 WHERE id = ?1", params![a, pb])?;
    tx.commit()
}

fn speaker(b: &Block) -> String {
    b.character_title.clone().unwrap_or_else(|| "?".to_string()).to_uppercase()
}

/// Un bloc en texte brut, façon scénario.
pub fn block_plain(b: &Block) -> String {
    match b.block_type {
        BlockType::Action => b.text.clone(),
        BlockType::Dialogue => format!("{} : {}", speaker(b), b.text),
        BlockType::Direction => format!("({})", b.text),
    }
}

/// Un bloc en Markdown (repris par les exports Markdown et PDF).
pub fn block_markdown(b: &Block) -> String {
    match b.block_type {
        BlockType::Action => b.text.clone(),
        BlockType::Dialogue => format!("**{}** : {}", speaker(b), b.text),
        BlockType::Direction => format!("*({})*", b.text),
    }
}

/// Lecture continue d'un épisode : actes, séquences, scènes et blocs.
pub fn episode_text(conn: &Connection, episode_id: i64) -> DbResult<String> {
    let mut out = String::new();
    let (mut last_act, mut last_seq) = (None, None);
    for s in scenes_of(conn, episode_id)? {
        if s.act != last_act && s.act.is_some() {
            out.push_str(&format!("=== ACTE {} ===\n\n", s.act.unwrap_or_default()));
        }
        if (s.sequence != last_seq || s.act != last_act) && s.sequence.is_some() {
            out.push_str(&format!("--- Séquence {} ---\n\n", s.sequence.unwrap_or_default()));
        }
        (last_act, last_seq) = (s.act, s.sequence);
        out.push_str(&format!("SCÈNE {} — {}\n", s.position, s.title));
        for b in list(conn, s.id)? {
            out.push_str(&block_plain(&b));
            out.push('\n');
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{fields, nodes, nodes::NodeKind, open_in_memory};

    fn scene(conn: &Connection) -> (i64, i64) {
        let ep = nodes::create_episode(conn, "Pilote", 1, 1).unwrap();
        (ep, nodes::create_scene(conn, ep, "Le dîner", 1).unwrap())
    }

    #[test]
    fn blocks_keep_their_order_and_can_be_moved() {
        let conn = open_in_memory().unwrap();
        let (_, sc) = scene(&conn);
        let a = add(&conn, sc, BlockType::Action, None, "Alice entre.").unwrap();
        let b = add(&conn, sc, BlockType::Dialogue, None, "Bonsoir !").unwrap();
        let c = add(&conn, sc, BlockType::Direction, None, "elle sourit").unwrap();
        let order = |conn: &Connection| list(conn, sc).unwrap().into_iter().map(|x| x.id).collect::<Vec<_>>();
        assert_eq!(order(&conn), [a, b, c]);
        move_block(&conn, c, true).unwrap();
        assert_eq!(order(&conn), [a, c, b]);
        move_block(&conn, a, true).unwrap(); // déjà en tête : sans effet
        move_block(&conn, b, false).unwrap(); // déjà en dernier : sans effet
        assert_eq!(order(&conn), [a, c, b]);
        move_block(&conn, a, false).unwrap();
        assert_eq!(order(&conn), [c, a, b]);
    }

    #[test]
    fn dialogue_keeps_its_speaker_other_types_do_not() {
        let conn = open_in_memory().unwrap();
        let (_, sc) = scene(&conn);
        let alice = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        let d = add(&conn, sc, BlockType::Dialogue, Some(alice), "Salut").unwrap();
        let a = add(&conn, sc, BlockType::Action, Some(alice), "Elle rit").unwrap();
        let blocks = list(&conn, sc).unwrap();
        assert_eq!(blocks[0].character_title.as_deref(), Some("Alice"));
        assert_eq!(blocks[1].character_id, None);
        update(&conn, d, BlockType::Action, Some(alice), "Salut").unwrap(); // plus un dialogue
        assert_eq!(list(&conn, sc).unwrap()[0].character_id, None);
        let _ = a;
        nodes::delete(&conn, alice).unwrap(); // l'interlocuteur supprimé : le bloc reste
        assert_eq!(list(&conn, sc).unwrap().len(), 2);
    }

    #[test]
    fn dialogue_is_searchable_and_survives_saving_the_scene_fields() {
        let conn = open_in_memory().unwrap();
        let (_, sc) = scene(&conn);
        add(&conn, sc, BlockType::Dialogue, None, "Quelle merveilleuse surprise").unwrap();
        assert_eq!(nodes::search_text(&conn, "merveilleuse").unwrap().len(), 1);
        fields::save(&conn, NodeKind::Scene, sc, &["notes de scène".to_string()]).unwrap();
        assert_eq!(nodes::search_text(&conn, "merveilleuse").unwrap().len(), 1); // pas écrasé
        assert_eq!(nodes::search_text(&conn, "notes").unwrap().len(), 1);
        let first = list(&conn, sc).unwrap()[0].id;
        delete(&conn, first).unwrap();
        assert!(nodes::search_text(&conn, "merveilleuse").unwrap().is_empty());
    }

    #[test]
    fn invalid_block_type_is_rejected_and_unknown_block_reported() {
        assert!(BlockType::parse("nimporte").is_err());
        let conn = open_in_memory().unwrap();
        assert!(matches!(delete(&conn, 42), Err(DbError::NotFound(42))));
        assert!(update(&conn, 42, BlockType::Action, None, "x").is_err());
    }

    #[test]
    fn deleting_a_scene_deletes_its_blocks() {
        let conn = open_in_memory().unwrap();
        let (_, sc) = scene(&conn);
        add(&conn, sc, BlockType::Action, None, "x").unwrap();
        nodes::delete(&conn, sc).unwrap();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM scene_blocks", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn scenes_can_be_reordered_within_an_episode_only() {
        let conn = open_in_memory().unwrap();
        let (ep, s1) = scene(&conn);
        let s2 = nodes::create_scene(&conn, ep, "La dispute", 2).unwrap();
        swap_scenes(&conn, s1, s2).unwrap();
        let titles: Vec<_> = scenes_of(&conn, ep).unwrap().into_iter().map(|s| s.title).collect();
        assert_eq!(titles, ["La dispute", "Le dîner"]);
        let other = nodes::create_episode(&conn, "Ép. 2", 1, 2).unwrap();
        let s3 = nodes::create_scene(&conn, other, "Ailleurs", 1).unwrap();
        assert!(swap_scenes(&conn, s1, s3).is_err());
    }

    #[test]
    fn structure_and_reading_text() {
        let conn = open_in_memory().unwrap();
        let (ep, sc) = scene(&conn);
        assert_eq!(structure(&conn, sc).unwrap(), (None, None));
        set_structure(&conn, sc, Some(1), Some(2)).unwrap();
        assert_eq!(structure(&conn, sc).unwrap(), (Some(1), Some(2)));
        let alice = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        add(&conn, sc, BlockType::Action, None, "Alice entre.").unwrap();
        add(&conn, sc, BlockType::Dialogue, Some(alice), "Bonsoir !").unwrap();
        add(&conn, sc, BlockType::Direction, None, "elle sourit").unwrap();
        let text = episode_text(&conn, ep).unwrap();
        assert!(text.contains("=== ACTE 1 ===") && text.contains("--- Séquence 2 ---"));
        assert!(text.contains("SCÈNE 1 — Le dîner") && text.contains("ALICE : Bonsoir !") && text.contains("(elle sourit)"));
        assert_eq!(block_markdown(&list(&conn, sc).unwrap()[1]), "**ALICE** : Bonsoir !");
    }
}
