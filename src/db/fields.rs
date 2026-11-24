//! Champs détaillés des fiches, décrits par une table de définitions par type d'élément.
//! Les noms de colonnes viennent exclusivement de ces constantes (jamais de l'utilisateur).

use rusqlite::{params_from_iter, types::Value, Connection};

use super::{nodes::NodeKind, DbError, DbResult};

type Options = &'static [(&'static str, &'static str)];

#[derive(Clone, Copy)]
pub enum FieldKind {
    Line,
    Text,
    /// (code stocké, libellé affiché)
    Choice(Options),
}

#[derive(Clone, Copy)]
pub struct FieldDef {
    pub column: &'static str,
    pub label: &'static str,
    pub kind: FieldKind,
}

const fn line(column: &'static str, label: &'static str) -> FieldDef {
    FieldDef {
        column,
        label,
        kind: FieldKind::Line,
    }
}
const fn text(column: &'static str, label: &'static str) -> FieldDef {
    FieldDef {
        column,
        label,
        kind: FieldKind::Text,
    }
}
const fn choice(column: &'static str, label: &'static str, o: Options) -> FieldDef {
    FieldDef {
        column,
        label,
        kind: FieldKind::Choice(o),
    }
}

const PLOT_TYPES: Options = &[
    ("main", "intrigue principale"),
    ("secondary", "intrigue secondaire"),
    ("running_gag", "running gag"),
    ("character_arc", "arc de personnage"),
    ("secret", "secret / révélation"),
    ("recurring_conflict", "conflit récurrent"),
];
const PLOT_STATUS: Options = &[
    ("planned", "prévue"),
    ("active", "en cours"),
    ("resolved", "résolue"),
    ("abandoned", "abandonnée"),
];
const EPISODE_STATUS: Options = &[
    ("idea", "idée"),
    ("outline", "plan"),
    ("drafting", "en écriture"),
    ("written", "écrit"),
    ("final", "version finale"),
];

const CHARACTER_ROLE: Options = &[("main", "principal"), ("secondary", "secondaire")];

const CHARACTER: &[FieldDef] = &[
    line("first_name", "Prénom"),
    line("last_name", "Nom"),
    line("nickname", "Surnom"),
    line("age", "Âge"),
    line("birth_date", "Date de naissance"),
    line("profession", "Profession"),
    text("physical", "Description physique"),
    text("style", "Style vestimentaire"),
    text("personality", "Traits de personnalité"),
    text("habits", "Particularités et habitudes"),
    text("biography", "Biographie"),
    text("qualities", "Qualités"),
    text("flaws", "Défauts"),
    text("fears", "Peurs"),
    text("desires", "Désirs"),
    text("objectives", "Objectifs"),
    text("contradictions", "Contradictions"),
    text("secrets", "Secrets"),
    text("verbal_tics", "Tics de langage"),
    text("catchphrases", "Expressions récurrentes"),
    text("behaviors", "Comportements caractéristiques"),
    text("initial_situation", "Situation initiale"),
    text("evolution", "Évolution au fil des épisodes"),
    text("key_events", "Événements marquants"),
    text("long_term_goals", "Objectifs à long terme"),
    text("personality_changes", "Changements de personnalité"),
    choice("role", "Rôle dans l'univers", CHARACTER_ROLE),
];
const PLACE: &[FieldDef] = &[
    line("place_type", "Type de lieu"),
    line("address", "Adresse fictive / localisation"),
    text("ambiance", "Ambiance"),
    text("role_in_show", "Fonction dans la sitcom"),
    text("history", "Informations historiques"),
];
const EPISODE: &[FieldDef] = &[
    choice("status", "Statut d'avancement", EPISODE_STATUS),
    line("theme", "Thème principal"),
    text("synopsis", "Synopsis"),
    text("production_notes", "Notes de production"),
];
const PLOT: &[FieldDef] = &[
    choice("plot_type", "Type", PLOT_TYPES),
    choice("status", "Statut", PLOT_STATUS),
    line("planned_resolution", "Résolution prévue"),
];
const OBJECT: &[FieldDef] = &[text("description", "Description")];
const SCENE: &[FieldDef] = &[text("content", "Notes de scène / brouillon libre")];

/// Section d'une fiche où ranger un champ (vide = pas de sections). Sert à replier les fiches longues.
pub fn group(kind: NodeKind, column: &str) -> &'static str {
    if kind != NodeKind::Character {
        return "";
    }
    match column {
        "first_name" | "last_name" | "nickname" | "age" | "birth_date" | "profession" | "role" => {
            "Identité"
        }
        "physical" | "style" => "Apparence",
        "personality" | "habits" | "qualities" | "flaws" | "fears" | "desires"
        | "contradictions" | "verbal_tics" | "catchphrases" | "behaviors" => "Personnalité",
        "biography" | "initial_situation" | "secrets" | "key_events" => "Histoire",
        _ => "Objectifs et évolution",
    }
}

/// Libellé affiché d'une valeur de liste de choix (le code lui-même si inconnu).
pub fn choice_label(kind: NodeKind, column: &str, code: &str) -> String {
    defs(kind)
        .iter()
        .find(|f| f.column == column)
        .and_then(|f| match f.kind {
            FieldKind::Choice(opts) => opts
                .iter()
                .find(|(c, _)| *c == code)
                .map(|(_, l)| l.to_string()),
            _ => None,
        })
        .unwrap_or_else(|| code.to_string())
}

pub fn defs(kind: NodeKind) -> &'static [FieldDef] {
    match kind {
        NodeKind::Character => CHARACTER,
        NodeKind::Place => PLACE,
        NodeKind::Episode => EPISODE,
        NodeKind::Plot => PLOT,
        NodeKind::Object => OBJECT,
        NodeKind::Scene => SCENE,
        NodeKind::Event => &[], // édité dans la Chronologie
    }
}

/// Valeurs des champs, dans l'ordre de `defs(kind)`.
pub fn load(conn: &Connection, kind: NodeKind, id: i64) -> DbResult<Vec<String>> {
    let d = defs(kind);
    if d.is_empty() {
        return Ok(Vec::new());
    }
    let cols: Vec<String> = d
        .iter()
        .map(|f| format!("COALESCE({}, '')", f.column))
        .collect();
    let sql = format!(
        "SELECT {} FROM {} WHERE id = ?1",
        cols.join(", "),
        kind.detail_table()
    );
    conn.query_row(&sql, [id], |r| {
        (0..d.len())
            .map(|i| r.get::<_, String>(i))
            .collect::<Result<Vec<_>, _>>()
    })
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DbError::NotFound(id),
        other => other.into(),
    })
}

/// Enregistre les champs (même ordre que `defs(kind)`) et marque l'élément comme modifié.
pub fn save(conn: &Connection, kind: NodeKind, id: i64, values: &[String]) -> DbResult<()> {
    let d = defs(kind);
    if d.is_empty() {
        return Ok(());
    }
    if values.len() != d.len() {
        return Err(DbError::Invalid("nombre de champs incorrect".into()));
    }
    for (f, v) in d.iter().zip(values) {
        if let FieldKind::Choice(opts) = f.kind {
            if !opts.iter().any(|(code, _)| *code == v.as_str()) {
                return Err(DbError::Invalid(format!("{} : valeur « {v} »", f.label)));
            }
        }
    }
    let sets: Vec<String> = d
        .iter()
        .enumerate()
        .map(|(i, f)| format!("{} = ?{}", f.column, i + 1))
        .collect();
    let sql = format!(
        "UPDATE {} SET {} WHERE id = ?{}",
        kind.detail_table(),
        sets.join(", "),
        d.len() + 1
    );
    let mut params: Vec<Value> = values.iter().map(|s| Value::Text(s.clone())).collect();
    params.push(Value::Integer(id));
    let n = conn.execute(&sql, params_from_iter(params))?;
    if n == 0 {
        return Err(DbError::NotFound(id));
    }
    rebuild_body(conn, kind, id)
}

/// Recalcule le texte indexé pour la recherche globale : champs libres (descriptions…) et,
/// pour une scène, les blocs de scénario (dialogues compris). Met aussi à jour la date de
/// modification (tableau de bord « derniers modifiés »).
pub fn rebuild_body(conn: &Connection, kind: NodeKind, id: i64) -> DbResult<()> {
    let d = defs(kind);
    let mut parts: Vec<String> = Vec::new();
    if !d.is_empty() {
        for (f, v) in d.iter().zip(load(conn, kind, id)?) {
            if !matches!(f.kind, FieldKind::Choice(_)) && !v.is_empty() {
                parts.push(v);
            }
        }
    }
    if kind == NodeKind::Scene {
        let mut stmt = conn.prepare(
            "SELECT COALESCE(n.title || ' : ', '') || b.text
             FROM scene_blocks b LEFT JOIN nodes n ON n.id = b.character_id
             WHERE b.scene_id = ?1 ORDER BY b.position",
        )?;
        let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
        for row in rows {
            parts.push(row?);
        }
    }
    // Les étiquettes sont indexées avec le reste du texte.
    let mut stmt = conn.prepare(
        "SELECT t.name FROM node_tags nt JOIN tags t ON t.id = nt.tag_id
         WHERE nt.node_id = ?1 ORDER BY t.name",
    )?;
    let names = stmt.query_map([id], |r| r.get::<_, String>(0))?;
    for name in names {
        parts.push(format!("#{}", name?));
    }
    conn.execute(
        "UPDATE nodes SET body = ?2 WHERE id = ?1",
        rusqlite::params![id, parts.join("\n")],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{nodes, open_in_memory};

    #[test]
    fn every_kind_roundtrips_its_fields() {
        let conn = open_in_memory().unwrap();
        for kind in [
            NodeKind::Character,
            NodeKind::Place,
            NodeKind::Plot,
            NodeKind::Object,
        ] {
            let id = nodes::create(&conn, kind, "x").unwrap();
            let defaults = load(&conn, kind, id).unwrap();
            assert_eq!(defaults.len(), defs(kind).len());
            let values: Vec<String> = defs(kind)
                .iter()
                .map(|f| match f.kind {
                    FieldKind::Choice(o) => o[o.len() - 1].0.to_string(),
                    _ => format!("valeur {}", f.column),
                })
                .collect();
            save(&conn, kind, id, &values).unwrap();
            assert_eq!(load(&conn, kind, id).unwrap(), values);
        }
    }

    #[test]
    fn episode_and_scene_fields_work() {
        let conn = open_in_memory().unwrap();
        let ep = nodes::create_episode(&conn, "Pilote", 1, 1).unwrap();
        let sc = nodes::create_scene(&conn, ep, "s", 1).unwrap();
        let mut v = load(&conn, NodeKind::Episode, ep).unwrap();
        v[0] = "drafting".into();
        save(&conn, NodeKind::Episode, ep, &v).unwrap();
        save(&conn, NodeKind::Scene, sc, &["ALICE : Salut !".to_string()]).unwrap();
        assert_eq!(
            load(&conn, NodeKind::Scene, sc).unwrap()[0],
            "ALICE : Salut !"
        );
    }

    #[test]
    fn detailed_fields_are_searchable() {
        let conn = open_in_memory().unwrap();
        let ep = nodes::create_episode(&conn, "Pilote", 1, 1).unwrap();
        let sc = nodes::create_scene(&conn, ep, "s", 1).unwrap();
        save(
            &conn,
            NodeKind::Scene,
            sc,
            &["ALICE : Quelle surprise !".to_string()],
        )
        .unwrap();
        assert_eq!(nodes::search_text(&conn, "surprise").unwrap().len(), 1);
    }

    #[test]
    fn invalid_choice_is_rejected() {
        let conn = open_in_memory().unwrap();
        let ep = nodes::create_episode(&conn, "Pilote", 1, 1).unwrap();
        let mut v = load(&conn, NodeKind::Episode, ep).unwrap();
        v[0] = "n'importe quoi".into();
        assert!(save(&conn, NodeKind::Episode, ep, &v).is_err());
    }
}
