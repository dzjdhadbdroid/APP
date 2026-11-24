//! Export (JSON complet, Markdown lisible) et import (JSON, additif et atomique).
//!
//! L'import AJOUTE le contenu du fichier à l'existant sans rien modifier ni supprimer :
//! tout est recréé avec de nouveaux identifiants. Réimporter deux fois le même fichier
//! crée donc deux copies. En cas d'erreur, rien n'est importé.

use std::collections::HashMap;

use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};

use super::{
    fields::{self, FieldKind},
    knowledge, links,
    nodes::{self, NodeKind},
    script::{self, BlockType},
    tags, timeline, Atomic, DbError, DbResult,
};

const FORMAT: &str = "sitcom-studio";

fn invalid(msg: impl Into<String>) -> DbError {
    DbError::Invalid(msg.into())
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_string()
}

// ───────────────────────── export JSON ─────────────────────────

pub fn export_json(conn: &Connection) -> DbResult<String> {
    export_filtered(conn, None)
}

/// Export complet, ou limité à certains éléments (et aux liens qui les touchent).
pub fn export_filtered(
    conn: &Connection,
    only: Option<&std::collections::HashSet<i64>>,
) -> DbResult<String> {
    let mut nodes_json = Vec::new();
    for n in nodes::all(conn)? {
        if only.is_some_and(|s| !s.contains(&n.id)) {
            continue;
        }
        let defs = fields::defs(n.kind);
        let mut f = Map::new();
        for (d, v) in defs.iter().zip(fields::load(conn, n.kind, n.id)?) {
            if !v.is_empty() {
                f.insert(d.column.to_string(), Value::String(v));
            }
        }
        let extra = match n.kind {
            NodeKind::Episode => conn.query_row(
                "SELECT season, number, air_order FROM episodes WHERE id = ?1",
                [n.id],
                |r| {
                    Ok(json!({
                        "season": r.get::<_, i64>(0)?,
                        "number": r.get::<_, i64>(1)?,
                        "air_order": r.get::<_, Option<i64>>(2)?,
                    }))
                },
            )?,
            NodeKind::Scene => {
                let mut v = conn.query_row(
                    "SELECT episode_id, position FROM scenes WHERE id = ?1",
                    [n.id],
                    |r| Ok(json!({ "episode_id": r.get::<_, i64>(0)?, "position": r.get::<_, i64>(1)? })),
                )?;
                let (act, sequence) = script::structure(conn, n.id)?;
                let blocks: Vec<Value> = script::list(conn, n.id)?
                    .into_iter()
                    .map(|b| json!({ "type": b.block_type.code(), "character": b.character_id, "text": b.text }))
                    .collect();
                v["act"] = json!(act);
                v["sequence"] = json!(sequence);
                v["blocks"] = json!(blocks);
                v
            }
            NodeKind::Event => conn.query_row(
                "SELECT story_start, importance, status FROM events WHERE id = ?1",
                [n.id],
                |r| {
                    Ok(json!({
                        "story_start": r.get::<_, Option<String>>(0)?,
                        "importance": r.get::<_, i64>(1)?,
                        "status": r.get::<_, String>(2)?,
                    }))
                },
            )?,
            _ => Value::Null,
        };
        nodes_json.push(json!({
            "id": n.id,
            "kind": n.kind.as_str(),
            "title": n.title,
            "summary": n.summary,
            "notes": n.notes,
            "fields": f,
            "tags": tags::for_node(conn, n.id)?.into_iter().map(|(_, t)| t).collect::<Vec<_>>(),
            "extra": extra,
        }));
    }

    let links_json: Vec<Value> = links::all(conn)?
        .into_iter()
        .filter(|l| only.map_or(true, |s| s.contains(&l.src_id) || s.contains(&l.dst_id)))
        .map(|l| {
            json!({
                "src": l.src_id, "dst": l.dst_id, "type": l.link_type,
                "label": l.label, "closeness": l.closeness,
            })
        })
        .collect();

    let knowledge_json = conn
        .prepare("SELECT character_id, subject_id, learned_event_id FROM knowledge")?
        .query_map([], |r| {
            Ok(json!({
                "character": r.get::<_, i64>(0)?,
                "subject": r.get::<_, i64>(1)?,
                "learned_event": r.get::<_, Option<i64>>(2)?,
            }))
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|k| {
            only.map_or(true, |set| {
                [&k["character"], &k["subject"]]
                    .iter()
                    .any(|v| v.as_i64().is_some_and(|i| set.contains(&i)))
            })
        })
        .collect::<Vec<_>>();

    let doc = json!({
        "format": FORMAT,
        "version": 1,
        "nodes": nodes_json,
        "links": links_json,
        "knowledge": knowledge_json,
    });
    serde_json::to_string_pretty(&doc).map_err(|e| invalid(e.to_string()))
}

// ───────────────────────── import JSON ─────────────────────────

#[derive(Debug, PartialEq, Eq)]
pub struct ImportReport {
    pub nodes: usize,
    pub links: usize,
    pub knowledge: usize,
}

/// Texte, résumé, notes et champs détaillés d'un nœud importé. Les valeurs de listes
/// de choix invalides sont ignorées (la valeur par défaut est conservée).
fn fill(conn: &Connection, kind: NodeKind, id: i64, v: &Value) -> DbResult<()> {
    nodes::update_text(
        conn,
        id,
        &text(v, "title"),
        &text(v, "summary"),
        &text(v, "notes"),
    )?;
    let defs = fields::defs(kind);
    if defs.is_empty() {
        return Ok(());
    }
    let mut values = fields::load(conn, kind, id)?;
    for (i, d) in defs.iter().enumerate() {
        if let Some(x) = v["fields"][d.column].as_str() {
            let allowed = match d.kind {
                FieldKind::Choice(options) => options.iter().any(|(code, _)| *code == x),
                _ => true,
            };
            if allowed {
                values[i] = x.to_string();
            }
        }
    }
    fields::save(conn, kind, id, &values)
}

/// Étiquettes d'un nœud importé ; une étiquette invalide est ignorée.
fn import_tags(conn: &Connection, id: i64, v: &Value) -> DbResult<()> {
    let none = Vec::new();
    for t in v["tags"].as_array().unwrap_or(&none) {
        if let Some(name) = t.as_str() {
            if tags::normalize(name).is_ok() {
                tags::add(conn, id, name)?;
            }
        }
    }
    Ok(())
}

pub fn import_json(conn: &Connection, source: &str) -> DbResult<ImportReport> {
    import_inner(conn, source, false)
}

/// Import d'un instantané de la corbeille : les liens vers des éléments qui existent
/// encore (même identifiant) sont rétablis.
pub fn import_restoring(conn: &Connection, source: &str) -> DbResult<ImportReport> {
    import_inner(conn, source, true)
}

/// Identifiant actuel d'un élément du fichier : celui qui vient d'être créé, ou (restauration
/// depuis la corbeille) l'élément du même identifiant qui existe encore.
fn resolve_id(
    conn: &Connection,
    map: &HashMap<i64, i64>,
    keep_external: bool,
    old: Option<i64>,
) -> Option<i64> {
    let old = old?;
    if let Some(&new) = map.get(&old) {
        return Some(new);
    }
    let exists = conn
        .query_row("SELECT COUNT(*) FROM nodes WHERE id = ?1", [old], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap_or(0)
        > 0;
    (keep_external && exists).then_some(old)
}

fn import_inner(conn: &Connection, source: &str, keep_external: bool) -> DbResult<ImportReport> {
    let doc: Value =
        serde_json::from_str(source).map_err(|e| invalid(format!("JSON illisible : {e}")))?;
    if doc["format"] != FORMAT {
        return Err(invalid("ce fichier n'est pas un export Sitcom Studio"));
    }
    let empty = Vec::new();
    let nodes_in = doc["nodes"].as_array().unwrap_or(&empty);

    let tx = Atomic::begin(conn)?;
    let mut map: HashMap<i64, i64> = HashMap::new();
    let mut report = ImportReport {
        nodes: 0,
        links: 0,
        knowledge: 0,
    };

    // Tout sauf les scènes ; les épisodes d'abord, dans leur ordre de diffusion d'origine.
    let mut first: Vec<&Value> = nodes_in.iter().filter(|n| n["kind"] != "scene").collect();
    first.sort_by_key(|n| n["extra"]["air_order"].as_i64().unwrap_or(i64::MAX));

    for v in first {
        let old = v["id"]
            .as_i64()
            .ok_or_else(|| invalid("nœud sans identifiant"))?;
        let kind = NodeKind::parse(v["kind"].as_str().unwrap_or_default())?;
        let title = text(v, "title");
        let new = match kind {
            NodeKind::Episode => {
                let season = v["extra"]["season"].as_i64().unwrap_or(1);
                let wanted = v["extra"]["number"].as_i64();
                let taken = match wanted {
                    Some(n) => {
                        tx.query_row(
                            "SELECT COUNT(*) FROM episodes WHERE season = ?1 AND number = ?2",
                            params![season, n],
                            |r| r.get::<_, i64>(0),
                        )? > 0
                    }
                    None => true,
                };
                let number = match wanted {
                    Some(n) if !taken => n,
                    _ => nodes::next_episode_number(&tx, season)?,
                };
                nodes::create_episode(&tx, &title, season, number)?
            }
            _ => nodes::create(&tx, kind, &title)?,
        };
        fill(&tx, kind, new, v)?;
        import_tags(&tx, new, v)?;
        if kind == NodeKind::Event {
            let e = &v["extra"];
            timeline::set_event(
                &tx,
                new,
                e["story_start"].as_str().unwrap_or_default(),
                e["importance"].as_i64().unwrap_or(3),
                e["status"].as_str().unwrap_or("planned"),
            )?;
        }
        map.insert(old, new);
        report.nodes += 1;
    }

    // Scènes : dans l'ordre de présentation d'origine, rattachées aux épisodes importés.
    let mut scenes: Vec<&Value> = nodes_in.iter().filter(|n| n["kind"] == "scene").collect();
    scenes.sort_by_key(|n| n["extra"]["position"].as_i64().unwrap_or(0));
    for v in scenes {
        let old = v["id"]
            .as_i64()
            .ok_or_else(|| invalid("scène sans identifiant"))?;
        let Some(episode) = resolve_id(&tx, &map, keep_external, v["extra"]["episode_id"].as_i64())
        else {
            continue; // épisode absent du fichier
        };
        let position = nodes::next_scene_position(&tx, episode)?;
        let new = nodes::create_scene(&tx, episode, &text(v, "title"), position)?;
        fill(&tx, NodeKind::Scene, new, v)?;
        import_tags(&tx, new, v)?;
        let e = &v["extra"];
        script::set_structure(&tx, new, e["act"].as_i64(), e["sequence"].as_i64())?;
        for b in e["blocks"].as_array().unwrap_or(&empty) {
            let ty = BlockType::parse(b["type"].as_str().unwrap_or("action"))
                .unwrap_or(BlockType::Action);
            let speaker = resolve_id(&tx, &map, keep_external, b["character"].as_i64());
            script::add(&tx, new, ty, speaker, &text(b, "text"))?;
        }
        map.insert(old, new);
        report.nodes += 1;
    }

    // Liens et connaissances : un doublon ou une référence manquante est simplement ignoré.
    let resolve = |old: Option<i64>| resolve_id(&tx, &map, keep_external, old);
    for l in doc["links"].as_array().unwrap_or(&empty) {
        let (Some(a), Some(b)) = (resolve(l["src"].as_i64()), resolve(l["dst"].as_i64())) else {
            continue;
        };
        let ty = l["type"].as_str().unwrap_or("relation");
        match links::create(&tx, a, b, ty, l["label"].as_str(), l["closeness"].as_i64()) {
            Ok(_) => report.links += 1,
            Err(DbError::Sqlite(_)) => {}
            Err(e) => return Err(e),
        }
    }
    for k in doc["knowledge"].as_array().unwrap_or(&empty) {
        let (Some(c), Some(s)) = (
            resolve(k["character"].as_i64()),
            resolve(k["subject"].as_i64()),
        ) else {
            continue;
        };
        let event = resolve(k["learned_event"].as_i64());
        match knowledge::add(&tx, c, s, event) {
            Ok(_) => report.knowledge += 1,
            Err(DbError::Sqlite(_)) => {}
            Err(e) => return Err(e),
        }
    }

    tx.commit()?;
    Ok(report)
}

// ───────────────────────── export Markdown ─────────────────────────

/// « Bible de série » lisible : une section par type, une fiche par élément.
pub fn export_markdown(conn: &Connection) -> DbResult<String> {
    let all_links = links::all(conn)?;
    let titles: HashMap<i64, String> = nodes::all(conn)?
        .into_iter()
        .map(|n| (n.id, n.title))
        .collect();
    let events: HashMap<i64, timeline::EventRow> = timeline::list_events(conn, None)?
        .into_iter()
        .map(|e| (e.id, e))
        .collect();

    let scenes = timeline::list_scenes(conn)?;
    let scene_order: HashMap<i64, usize> =
        scenes.iter().enumerate().map(|(i, s)| (s.id, i)).collect();
    let scene_info: HashMap<i64, (String, i64)> = scenes
        .iter()
        .map(|s| (s.id, (s.episode_title.clone(), s.position)))
        .collect();

    let mut out = String::from("# Bible de la sitcom\n");
    for kind in [
        NodeKind::Character,
        NodeKind::Place,
        NodeKind::Episode,
        NodeKind::Scene,
        NodeKind::Plot,
        NodeKind::Object,
        NodeKind::Event,
    ] {
        let mut list = nodes::list_by_kind(conn, kind)?;
        if list.is_empty() {
            continue;
        }
        if kind == NodeKind::Scene {
            list.sort_by_key(|n| scene_order.get(&n.id).copied().unwrap_or(usize::MAX));
        }
        out.push_str(&format!("\n## {}\n", kind.label_plural()));
        for n in list {
            match scene_info.get(&n.id) {
                Some((episode, position)) => out.push_str(&format!(
                    "\n### {episode} · scène {position} · {}\n",
                    n.title
                )),
                None => out.push_str(&format!("\n### {}\n", n.title)),
            }
            if let Some(e) = events.get(&n.id) {
                let date = e.story_start.as_deref().unwrap_or("non daté");
                out.push_str(&format!(
                    "\n**Date** : {date} · **Importance** : {} · **Statut** : {}\n",
                    e.importance,
                    timeline::status_label(&e.status)
                ));
            }
            if !n.summary.is_empty() {
                out.push_str(&format!("\n{}\n", n.summary));
            }
            for (d, v) in fields::defs(kind)
                .iter()
                .zip(fields::load(conn, kind, n.id)?)
            {
                if v.is_empty() {
                    continue;
                }
                let shown = match d.kind {
                    FieldKind::Choice(options) => options
                        .iter()
                        .find(|(code, _)| *code == v)
                        .map_or(v.clone(), |(_, label)| (*label).to_string()),
                    _ => v,
                };
                out.push_str(&format!("\n**{}** : {}\n", d.label, shown));
            }
            if kind == NodeKind::Scene {
                let (act, sequence) = script::structure(conn, n.id)?;
                if act.is_some() || sequence.is_some() {
                    let part =
                        |label: &str, v: Option<i64>| v.map(|v| format!("**{label}** : {v}"));
                    let both: Vec<String> = [part("Acte", act), part("Séquence", sequence)]
                        .into_iter()
                        .flatten()
                        .collect();
                    out.push_str(&format!("\n{}\n", both.join(" · ")));
                }
                for b in script::list(conn, n.id)? {
                    out.push_str(&format!("\n{}\n", script::block_markdown(&b)));
                }
            }
            if !n.notes.is_empty() {
                out.push_str(&format!("\n**Notes** : {}\n", n.notes));
            }
            let mine: Vec<String> = all_links
                .iter()
                .filter(|l| l.src_id == n.id || l.dst_id == n.id)
                .map(|l| {
                    let (arrow, other) = if l.src_id == n.id {
                        ("→", l.dst_id)
                    } else {
                        ("←", l.src_id)
                    };
                    let what = l.label.clone().unwrap_or_else(|| l.link_type.clone());
                    let name = titles.get(&other).map_or("?", String::as_str);
                    format!("- {arrow} {what} : {name}")
                })
                .collect();
            if !mine.is_empty() {
                out.push_str(&format!("\n**Connexions**\n\n{}\n", mine.join("\n")));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    /// Un petit univers couvrant tous les types d'éléments.
    fn world(conn: &Connection) {
        let alice = nodes::create(conn, NodeKind::Character, "Alice").unwrap();
        let bob = nodes::create(conn, NodeKind::Character, "Bob").unwrap();
        let ep = nodes::create_episode(conn, "Pilote", 1, 1).unwrap();
        let scene = nodes::create_scene(conn, ep, "Scène 1", 1).unwrap();
        let ev = nodes::create(conn, NodeKind::Event, "La rencontre").unwrap();
        let mut f = fields::load(conn, NodeKind::Character, alice).unwrap();
        f[0] = "Alicia".into();
        fields::save(conn, NodeKind::Character, alice, &f).unwrap();
        fields::save(
            conn,
            NodeKind::Scene,
            scene,
            &["ALICE : Salut !".to_string()],
        )
        .unwrap();
        timeline::set_event(conn, ev, "1998-06-12", 5, "confirmed").unwrap();
        links::create(conn, alice, bob, "relation", Some("frère"), Some(8)).unwrap();
        links::create(conn, ev, alice, "involves", None, None).unwrap();
        knowledge::add(conn, alice, bob, Some(ev)).unwrap();
        tags::add(conn, alice, "famille").unwrap();
    }

    #[test]
    fn export_then_import_recreates_the_whole_universe() {
        let a = open_in_memory().unwrap();
        world(&a);
        let json = export_json(&a).unwrap();

        let b = open_in_memory().unwrap();
        let report = import_json(&b, &json).unwrap();
        assert_eq!(
            report,
            ImportReport {
                nodes: 5,
                links: 2,
                knowledge: 1
            }
        );

        let alice = nodes::search_text(&b, "Alice")
            .unwrap()
            .into_iter()
            .find(|n| n.title == "Alice")
            .unwrap();
        assert_eq!(
            fields::load(&b, NodeKind::Character, alice.id).unwrap()[0],
            "Alicia"
        );
        assert_eq!(tags::for_node(&b, alice.id).unwrap()[0].1, "famille");
        let ev = timeline::list_events(&b, None).unwrap();
        assert_eq!(ev[0].story_start.as_deref(), Some("1998-06-12"));
        assert_eq!((ev[0].importance, ev[0].status.as_str()), (5, "confirmed"));
        assert_eq!(timeline::list_scenes(&b).unwrap().len(), 1);
        assert_eq!(nodes::search_text(&b, "Salut").unwrap().len(), 1); // dialogue retrouvé
                                                                       // un export de la copie contient autant d'éléments que l'original
        let again: Value = serde_json::from_str(&export_json(&b).unwrap()).unwrap();
        assert_eq!(again["nodes"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn script_blocks_structure_and_speakers_survive_export_import_and_reach_the_markdown() {
        let a = open_in_memory().unwrap();
        let alice = nodes::create(&a, NodeKind::Character, "Alice").unwrap();
        let ep = nodes::create_episode(&a, "Pilote", 1, 1).unwrap();
        let sc = nodes::create_scene(&a, ep, "Le dîner", 1).unwrap();
        script::set_structure(&a, sc, Some(2), Some(3)).unwrap();
        script::add(&a, sc, BlockType::Action, None, "Alice entre.").unwrap();
        script::add(&a, sc, BlockType::Dialogue, Some(alice), "Bonsoir !").unwrap();
        script::add(&a, sc, BlockType::Direction, None, "elle sourit").unwrap();

        let b = open_in_memory().unwrap();
        import_json(&b, &export_json(&a).unwrap()).unwrap();
        let new_scene = timeline::list_scenes(&b).unwrap()[0].id;
        assert_eq!(
            script::structure(&b, new_scene).unwrap(),
            (Some(2), Some(3))
        );
        let blocks = script::list(&b, new_scene).unwrap();
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].character_title.as_deref(), Some("Alice")); // locuteur remappé
        assert_eq!(nodes::search_text(&b, "Bonsoir").unwrap().len(), 1);

        let md = export_markdown(&b).unwrap();
        assert!(md.contains("### Pilote · scène 1 · Le dîner"));
        assert!(md.contains("**Acte** : 2 · **Séquence** : 3"));
        assert!(md.contains("**ALICE** : Bonsoir !") && md.contains("*(elle sourit)*"));
    }

    #[test]
    fn import_is_additive_and_never_touches_existing_data() {
        let conn = open_in_memory().unwrap();
        world(&conn);
        let json = export_json(&conn).unwrap();
        import_json(&conn, &json).unwrap();
        assert_eq!(nodes::all(&conn).unwrap().len(), 10);
        // les deux « Pilote » coexistent malgré la contrainte (saison, numéro)
        assert_eq!(timeline::list_air(&conn).unwrap().len(), 2);
    }

    #[test]
    fn a_broken_import_changes_nothing() {
        let conn = open_in_memory().unwrap();
        let bad = r#"{"format":"sitcom-studio","version":1,"nodes":[
            {"id":1,"kind":"character","title":"Ok"},
            {"id":2,"kind":"event","title":"Mauvaise date","extra":{"story_start":"pas une date"}}]}"#;
        assert!(import_json(&conn, bad).is_err());
        assert!(nodes::all(&conn).unwrap().is_empty()); // « Ok » a été annulé aussi
        assert!(import_json(&conn, "pas du json").is_err());
        assert!(import_json(&conn, r#"{"format":"autre"}"#).is_err());
    }

    #[test]
    fn markdown_lists_fields_dates_and_connections() {
        let conn = open_in_memory().unwrap();
        world(&conn);
        let md = export_markdown(&conn).unwrap();
        assert!(md.contains("## Personnages"));
        assert!(md.contains("**Prénom** : Alicia"));
        assert!(md.contains("**Date** : 1998-06-12"));
        assert!(md.contains("- → frère : Bob"));
        assert!(md.contains("ALICE : Salut !"));
    }
}
