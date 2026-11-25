//! Dossier de présentation (pitch) : une version courte et propre de la bible, pour la montrer.

use rusqlite::Connection;

use super::{fields, nodes, nodes::NodeKind, plots, settings, timeline, DbResult};

fn field(conn: &Connection, kind: NodeKind, id: i64, column: &str) -> String {
    let defs = fields::defs(kind);
    let Ok(values) = fields::load(conn, kind, id) else {
        return String::new();
    };
    defs.iter()
        .zip(values)
        .find(|(d, _)| d.column == column)
        .map(|(_, v)| v.trim().to_string())
        .unwrap_or_default()
}

fn push_paragraph(out: &mut String, text: &str) {
    let t = text.trim();
    if !t.is_empty() {
        out.push_str(&format!("\n{t}\n"));
    }
}

/// Markdown du dossier : personnages principaux, lieux, intrigues, puis saison par saison.
pub fn pitch_markdown(conn: &Connection, title: &str) -> DbResult<String> {
    let mut out = format!("# {title}\n");
    let secondary = nodes::secondary_ids(conn)?;

    let characters: Vec<_> = nodes::list_by_kind(conn, NodeKind::Character)?
        .into_iter()
        .filter(|n| !secondary.contains(&n.id))
        .collect();
    if !characters.is_empty() {
        out.push_str("\n## Les personnages\n");
        for c in &characters {
            out.push_str(&format!("\n### {}\n", c.title));
            let age = field(conn, NodeKind::Character, c.id, "age");
            let job = field(conn, NodeKind::Character, c.id, "profession");
            let ident: Vec<String> = [
                (!age.is_empty()).then(|| format!("{age} ans")),
                (!job.is_empty()).then_some(job),
            ]
            .into_iter()
            .flatten()
            .collect();
            if !ident.is_empty() {
                out.push_str(&format!("\n{}\n", ident.join(" · ")));
            }
            push_paragraph(&mut out, &c.summary);
            for (label, column) in [
                ("Personnalité", "personality"),
                ("Désirs", "desires"),
                ("Tics de langage", "verbal_tics"),
            ] {
                let v = field(conn, NodeKind::Character, c.id, column);
                if !v.is_empty() {
                    out.push_str(&format!("\n**{label}** : {v}\n"));
                }
            }
        }
    }

    let places = nodes::list_by_kind(conn, NodeKind::Place)?;
    if !places.is_empty() {
        out.push_str("\n## Les lieux\n");
        for p in &places {
            out.push_str(&format!("\n### {}\n", p.title));
            push_paragraph(&mut out, &p.summary);
        }
    }

    let arcs: Vec<_> = nodes::list_by_kind(conn, NodeKind::Plot)?
        .into_iter()
        .filter(|p| !plots::gag_ids(conn).is_ok_and(|g| g.contains(&p.id)))
        .collect();
    if !arcs.is_empty() {
        out.push_str("\n## Les intrigues\n");
        for p in &arcs {
            out.push_str(&format!("\n### {}\n", p.title));
            push_paragraph(&mut out, &p.summary);
        }
    }

    let air = timeline::list_air(conn)?;
    for season in timeline::seasons(conn)? {
        out.push_str(&format!("\n## Saison {season}\n"));
        if let Some(arc) = settings::get(conn, &format!("season_arc_{season}"))? {
            push_paragraph(&mut out, &arc);
        }
        for ep in air.iter().filter(|e| e.season == season) {
            out.push_str(&format!("\n### Épisode {} — {}\n", ep.number, ep.title));
            let synopsis = field(conn, NodeKind::Episode, ep.id, "synopsis");
            let node = nodes::get(conn, ep.id)?;
            push_paragraph(
                &mut out,
                if synopsis.is_empty() {
                    &node.summary
                } else {
                    &synopsis
                },
            );
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::open_in_memory;

    #[test]
    fn pitch_lists_main_characters_and_seasons() {
        let conn = open_in_memory().unwrap();
        let a = nodes::create(&conn, NodeKind::Character, "Alice").unwrap();
        let b = nodes::create(&conn, NodeKind::Character, "Figurant").unwrap();
        nodes::set_secondary(&conn, b, true).unwrap();
        conn.execute(
            "UPDATE characters SET personality = 'Têtue' WHERE id = ?1",
            [a],
        )
        .unwrap();
        let e = nodes::create_episode(&conn, "Pilote", 1, 1).unwrap();
        conn.execute(
            "UPDATE episodes SET synopsis = 'Alice arrive.' WHERE id = ?1",
            [e],
        )
        .unwrap();
        settings::set(&conn, "season_arc_1", "Alice s'installe.").unwrap();
        let md = pitch_markdown(&conn, "Ma série").unwrap();
        assert!(md.starts_with("# Ma série"));
        assert!(md.contains("### Alice") && md.contains("**Personnalité** : Têtue"));
        assert!(!md.contains("Figurant"));
        assert!(md.contains("## Saison 1") && md.contains("Alice s'installe."));
        assert!(md.contains("### Épisode 1 — Pilote") && md.contains("Alice arrive."));
    }
}
