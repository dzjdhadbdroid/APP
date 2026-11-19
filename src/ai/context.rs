//! Construit le contexte envoyé au modèle à partir des éléments choisis par l'utilisateur.

use std::collections::HashMap;

use rusqlite::Connection;

use super::prompt::{Conn, ContextItem};
use crate::db::{
    fields::{self, FieldKind},
    knowledge,
    links::{self, link_type_label},
    nodes::{self, NodeKind},
    timeline, DbResult,
};

/// Éléments dans l'ordre choisi ; avec `with_neighbors`, leurs voisins directs sont ajoutés à la suite.
pub fn gather(conn: &Connection, ids: &[i64], with_neighbors: bool) -> DbResult<Vec<ContextItem>> {
    let all_links = links::all(conn)?;
    let titles: HashMap<i64, String> = nodes::all(conn)?.into_iter().map(|n| (n.id, n.title)).collect();
    let dates: HashMap<i64, Option<String>> = timeline::list_events(conn, None)?
        .into_iter()
        .map(|e| (e.id, e.story_start))
        .collect();

    let mut order: Vec<i64> = Vec::new();
    for id in ids {
        if !order.contains(id) {
            order.push(*id);
        }
    }
    if with_neighbors {
        let base = order.clone();
        for l in &all_links {
            for (a, b) in [(l.src_id, l.dst_id), (l.dst_id, l.src_id)] {
                if base.contains(&a) && !order.contains(&b) {
                    order.push(b);
                }
            }
        }
    }

    let mut out = Vec::new();
    for id in order {
        let Ok(node) = nodes::get(conn, id) else { continue }; // supprimé entre-temps
        let defs = fields::defs(node.kind);
        let mut shown = Vec::new();
        for (d, v) in defs.iter().zip(fields::load(conn, node.kind, id)?) {
            if v.is_empty() {
                continue;
            }
            let v = match d.kind {
                FieldKind::Choice(options) => options
                    .iter()
                    .find(|(code, _)| *code == v)
                    .map_or(v.clone(), |(_, label)| (*label).to_string()),
                _ => v,
            };
            shown.push((d.label.to_string(), v));
        }
        let connections = all_links
            .iter()
            .filter(|l| l.src_id == id || l.dst_id == id)
            .map(|l| {
                let (arrow, other) = if l.src_id == id { ("→", l.dst_id) } else { ("←", l.src_id) };
                let what = l.label.clone().unwrap_or_else(|| link_type_label(&l.link_type).to_string());
                Conn {
                    text: format!("{arrow} {what}"),
                    target: other,
                    target_title: titles.get(&other).cloned().unwrap_or_default(),
                }
            })
            .collect();
        let knowledge = if node.kind == NodeKind::Character {
            knowledge::for_character(conn, id)?
                .into_iter()
                .map(|k| match k.learned_event_title {
                    Some(e) => format!("« {} » — appris à « {e} »", k.subject_title),
                    None => format!("« {} » — moment non précisé", k.subject_title),
                })
                .collect()
        } else {
            Vec::new()
        };
        out.push(ContextItem {
            id,
            kind: node.kind.label().to_string(),
            title: node.title,
            summary: node.summary,
            when: dates.get(&id).cloned().flatten(),
            fields: shown,
            notes: node.notes,
            connections,
            knowledge,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::prompt;
    use crate::db::{open_in_memory, timeline};

    #[test]
    fn gathers_fields_links_knowledge_dates_and_neighbors() {
        let conn = open_in_memory().unwrap();
        let alice = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        let bob = nodes::create(&conn, NodeKind::Character, "Bob").unwrap();
        let ev = nodes::create(&conn, NodeKind::Event, "La rencontre").unwrap();
        let mut f = fields::load(&conn, NodeKind::Character, alice).unwrap();
        f[0] = "Alicia".into();
        fields::save(&conn, NodeKind::Character, alice, &f).unwrap();
        timeline::set_event(&conn, ev, "1998-06-12", 3, "confirmed").unwrap();
        links::create(&conn, alice, bob, "relation", Some("frère"), None).unwrap();
        knowledge::add(&conn, alice, bob, Some(ev)).unwrap();

        let only_alice = gather(&conn, &[alice], false).unwrap();
        assert_eq!(only_alice.len(), 1);
        let (text, _) = prompt::render_context(&only_alice, 10_000);
        assert!(text.contains("Prénom : Alicia") && text.contains("→ frère « Bob »"));
        assert!(text.contains("appris à « La rencontre »"));
        assert!(!text.contains("[#") || prompt::allowed_ids(&only_alice).contains(&bob));

        let with_neighbors = gather(&conn, &[alice], true).unwrap();
        assert_eq!(with_neighbors.len(), 2); // Alice + Bob
        let dated = gather(&conn, &[ev], false).unwrap();
        assert_eq!(dated[0].when.as_deref(), Some("1998-06-12"));
    }

    #[test]
    fn deleted_elements_are_skipped_and_duplicates_removed() {
        let conn = open_in_memory().unwrap();
        let a = nodes::create(&conn, NodeKind::Place, "Appart").unwrap();
        assert_eq!(gather(&conn, &[a, a, 9999], false).unwrap().len(), 1);
    }
}
