//! Analyse du texte des PDF de scénario/synopsis rattachés aux épisodes, recoupée avec la bible :
//! qui parle (et combien), quels lieux (intitulés INT./EXT.), quels personnages sont cités.
//! Heuristique pensée pour les scénarios français : « NOM » seul sur une ligne suivi de sa réplique,
//! ou « NOM : réplique ».

use std::collections::HashMap;

use rusqlite::Connection;

use super::DbResult;

/// Minuscules sans accents, pour comparer des noms.
pub fn norm(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, PartialEq)]
pub struct Speaker {
    pub name: String,
    pub lines: i64,
    pub words: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PdfScript {
    pub speakers: Vec<Speaker>,
    /// (lieu tel qu'écrit dans l'intitulé, nombre de scènes)
    pub locations: Vec<(String, i64)>,
}

const NOT_SPEAKERS: [&str; 10] = [
    "fin",
    "cut",
    "fondu",
    "generique",
    "acte",
    "scene",
    "titre",
    "noir",
    "plan",
    "resume",
];

fn is_slugline(line: &str) -> bool {
    let u = norm(line);
    [
        "int.",
        "ext.",
        "int ",
        "ext ",
        "int/",
        "ext/",
        "interieur",
        "exterieur",
    ]
    .iter()
    .any(|p| u.starts_with(p))
}

fn is_upper_name(s: &str) -> bool {
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 2
        && letters.iter().all(|c| c.is_uppercase())
        && s.chars().all(|c| c.is_alphabetic() || " '’-.".contains(c))
        && s.split_whitespace().count() <= 4
        && s.chars().count() <= 30
}

/// « ALICE », « ALICE : » ou « ALICE (off) » seul sur une ligne.
fn header(line: &str) -> Option<String> {
    let mut s = line.trim().trim_end_matches(':').trim();
    if let Some(i) = s.find('(') {
        s = s[..i].trim();
    }
    if !is_upper_name(s) || is_slugline(s) {
        return None;
    }
    if NOT_SPEAKERS.contains(&norm(s).split(' ').next().unwrap_or("")) {
        return None;
    }
    Some(s.to_string())
}

/// « Alice : texte » (nom connu ou tout en majuscules).
fn inline<'a>(line: &'a str, known: &dyn Fn(&str) -> bool) -> Option<(String, &'a str)> {
    let (name, rest) = line.split_once(':')?;
    let name = name.trim();
    let rest = rest.trim();
    if rest.is_empty()
        || name.is_empty()
        || name.chars().count() > 30
        || name.split_whitespace().count() > 3
        || name.chars().any(|c| c.is_ascii_digit())
        || !name.chars().next()?.is_uppercase()
    {
        return None;
    }
    let name = name.split('(').next().unwrap_or(name).trim();
    (is_upper_name(name) || known(name)).then(|| (name.to_string(), rest))
}

fn word_count(s: &str) -> i64 {
    s.split_whitespace().count() as i64
}

/// Analyse un texte de scénario. `known` dit si un nom (non normalisé) est un personnage connu.
pub fn parse_script(text: &str, known: &dyn Fn(&str) -> bool) -> PdfScript {
    let mut order: Vec<String> = Vec::new();
    let mut speakers: HashMap<String, Speaker> = HashMap::new();
    let mut locs: Vec<(String, i64)> = Vec::new();
    let mut current: Option<String> = None;
    let mut add = |name: &str, lines: i64, words: i64, order: &mut Vec<String>| {
        let key = norm(name);
        let e = speakers.entry(key.clone()).or_insert_with(|| {
            order.push(key);
            Speaker {
                name: name.to_string(),
                lines: 0,
                words: 0,
            }
        });
        e.lines += lines;
        e.words += words;
    };
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            current = None;
            continue;
        }
        if is_slugline(line) {
            current = None;
            let loc = slug_place(line);
            if !loc.is_empty() {
                match locs.iter_mut().find(|(l, _)| norm(l) == norm(&loc)) {
                    Some(e) => e.1 += 1,
                    None => locs.push((loc, 1)),
                }
            }
            continue;
        }
        if is_upper_name(line.trim_end_matches(':').trim())
            && NOT_SPEAKERS.contains(&norm(line).split(' ').next().unwrap_or(""))
        {
            current = None;
            continue;
        }
        if let Some(name) = header(line) {
            add(&name, 1, 0, &mut order);
            current = Some(name);
            continue;
        }
        if let Some((name, rest)) = inline(line, known) {
            add(&name, 1, word_count(rest), &mut order);
            current = Some(name);
            continue;
        }
        if let Some(name) = &current {
            if !(line.starts_with('(') && line.ends_with(')')) {
                add(name, 0, word_count(line), &mut order);
            }
        }
    }
    PdfScript {
        speakers: order
            .into_iter()
            .filter_map(|k| speakers.remove(&k))
            .collect(),
        locations: locs,
    }
}

/// « INT. CUISINE - JOUR » → « CUISINE » (on retire le préfixe et le moment de la journée).
fn slug_place(line: &str) -> String {
    let mut s = line.trim();
    for p in [
        "INT./EXT.",
        "INT/EXT",
        "INT.",
        "EXT.",
        "INT ",
        "EXT ",
        "INTÉRIEUR",
        "EXTÉRIEUR",
    ] {
        if s.len() >= p.len() && s.is_char_boundary(p.len()) && s[..p.len()].eq_ignore_ascii_case(p)
        {
            s = &s[p.len()..];
            break;
        }
    }
    let s = s.trim_start_matches([' ', '-', '–', ':', '.']);
    let cut = [" - ", " – ", " — "]
        .iter()
        .filter_map(|sep| s.find(sep))
        .min();
    let s = cut.map_or(s, |i| &s[..i]);
    s.trim()
        .trim_end_matches(['-', '–', '.'])
        .trim()
        .to_string()
}

/// Textes extraits des PDF rattachés à l'épisode ou à ses scènes (créneau « script » par défaut).
pub fn episode_text(conn: &Connection, episode: i64, slot: &str) -> DbResult<String> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(a.text, '') FROM attachments a
         WHERE a.slot = ?2 AND (a.node_id = ?1 OR a.node_id IN (SELECT id FROM scenes WHERE episode_id = ?1))
         ORDER BY a.id",
    )?;
    let parts: Vec<String> = stmt
        .query_map(rusqlite::params![episode, slot], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(parts.join("\n"))
}

/// Personnages de la bible : (id, titre, noms normalisés qui les désignent).
pub fn character_aliases(conn: &Connection) -> DbResult<Vec<(i64, String, Vec<String>)>> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.title, c.first_name, c.nickname FROM nodes n JOIN characters c ON c.id = n.id",
    )?;
    let rows: Vec<(i64, String, Option<String>, Option<String>)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    Ok(rows
        .into_iter()
        .map(|(id, title, first, nick)| {
            let mut a = vec![norm(&title)];
            for x in [first, nick].into_iter().flatten() {
                let x = norm(&x);
                if !x.is_empty() && !a.contains(&x) {
                    a.push(x);
                }
            }
            (id, title, a)
        })
        .collect())
}

/// Retrouve le personnage de la bible désigné par un nom du scénario.
pub fn match_character(name: &str, chars: &[(i64, String, Vec<String>)]) -> Option<i64> {
    let n = norm(name);
    if let Some(c) = chars.iter().find(|c| c.2.contains(&n)) {
        return Some(c.0);
    }
    let hits: Vec<_> = chars
        .iter()
        .filter(|c| c.2.iter().any(|a| a.split(' ').any(|w| w == n)))
        .collect();
    (hits.len() == 1).then(|| hits[0].0)
}

/// Retrouve un lieu de la bible à partir d'un intitulé de scène.
pub fn match_place(loc: &str, places: &[(i64, String)]) -> Option<i64> {
    let l = norm(loc);
    places
        .iter()
        .find(|(_, t)| {
            let t = norm(t);
            !t.is_empty() && (l == t || l.contains(&t) || t.contains(&l))
        })
        .map(|p| p.0)
}

pub fn places(conn: &Connection) -> DbResult<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, title FROM nodes WHERE kind = 'place'")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Analyse du scénario PDF d'un épisode.
pub fn analyze_episode(conn: &Connection, episode: i64) -> DbResult<PdfScript> {
    let text = episode_text(conn, episode, "script")?;
    let chars = character_aliases(conn)?;
    let known = |n: &str| match_character(n, &chars).is_some();
    Ok(parse_script(&text, &known))
}

#[derive(Debug, Clone)]
pub struct EpisodeCheck {
    pub episode_id: i64,
    pub title: String,
    pub script: PdfScript,
    /// Personnage de la bible repéré pour chaque intervenant (même ordre que `script.speakers`).
    pub speaker_ids: Vec<Option<i64>>,
    /// Lieu de la bible repéré pour chaque lieu du PDF (même ordre que `script.locations`).
    pub place_ids: Vec<Option<i64>>,
    /// Personnages de la bible cités dans le synopsis/scénario sans jamais parler.
    pub mentioned_only: Vec<(i64, String)>,
}

/// Recoupement de tous les épisodes qui ont un PDF avec texte.
pub fn cross_check(conn: &Connection) -> DbResult<Vec<EpisodeCheck>> {
    let chars = character_aliases(conn)?;
    let places = places(conn)?;
    let known = |n: &str| match_character(n, &chars).is_some();
    let mut stmt = conn.prepare(
        "SELECT ep.id, n.title FROM episodes ep JOIN nodes n ON n.id = ep.id
         ORDER BY ep.season, ep.number, ep.id",
    )?;
    let eps: Vec<(i64, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, title) in eps {
        let script_text = episode_text(conn, id, "script")?;
        let syn_text = episode_text(conn, id, "synopsis")?;
        if script_text.trim().is_empty() && syn_text.trim().is_empty() {
            continue;
        }
        let script = parse_script(&script_text, &known);
        let speaker_ids: Vec<Option<i64>> = script
            .speakers
            .iter()
            .map(|s| match_character(&s.name, &chars))
            .collect();
        let place_ids = script
            .locations
            .iter()
            .map(|l| match_place(&l.0, &places))
            .collect();
        let all = format!("{} {}", norm(&script_text), norm(&syn_text));
        let padded = format!(
            " {} ",
            all.replace(
                ['.', ',', ';', '!', '?', ':', '\'', '’', '"', '(', ')'],
                " "
            )
        );
        let speaking: Vec<i64> = speaker_ids.iter().flatten().copied().collect();
        let mentioned_only = chars
            .iter()
            .filter(|c| !speaking.contains(&c.0))
            .filter(|c| {
                c.2.iter()
                    .any(|a| a.len() >= 3 && padded.contains(&format!(" {a} ")))
            })
            .map(|c| (c.0, c.1.clone()))
            .collect();
        out.push(EpisodeCheck {
            episode_id: id,
            title,
            script,
            speaker_ids,
            place_ids,
            mentioned_only,
        });
    }
    Ok(out)
}

/// Mots de dialogue par personnage de la bible, d'après le PDF de l'épisode.
pub fn speech_by_character(conn: &Connection, episode: i64) -> DbResult<Vec<(i64, i64, i64)>> {
    let chars = character_aliases(conn)?;
    let script = analyze_episode(conn, episode)?;
    let mut acc: HashMap<i64, (i64, i64)> = HashMap::new();
    for s in &script.speakers {
        if let Some(id) = match_character(&s.name, &chars) {
            let e = acc.entry(id).or_default();
            e.0 += s.lines;
            e.1 += s.words;
        }
    }
    Ok(acc.into_iter().map(|(id, (l, w))| (id, l, w)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{attachments, nodes, nodes::NodeKind, open_in_memory};

    const SCRIPT: &str = "ACTE 1\nINT. CUISINE - JOUR\nAlice entre.\nALICE\nBonjour tout le monde\n(souriant)\net bienvenue\n\nBOB (off)\nSalut\nEXT. JARDIN - NUIT\nCHARLES : Je n'existe pas dans la bible\nFIN";

    #[test]
    fn parses_speakers_and_locations() {
        let r = parse_script(SCRIPT, &|_| false);
        let s: Vec<_> = r
            .speakers
            .iter()
            .map(|s| (s.name.as_str(), s.lines, s.words))
            .collect();
        assert_eq!(s, vec![("ALICE", 1, 6), ("BOB", 1, 1), ("CHARLES", 1, 6)]);
        assert_eq!(
            r.locations,
            vec![("CUISINE".to_string(), 1), ("JARDIN".to_string(), 1)]
        );
        let inl = parse_script("Alice : Salut toi\nNote : rien", &|n| n == "Alice");
        assert_eq!(inl.speakers.len(), 1);
        assert_eq!(inl.speakers[0].words, 2);
    }

    #[test]
    fn crosses_with_bible() {
        let conn = open_in_memory().unwrap();
        let alice = nodes::create(&conn, NodeKind::Character, "Alice Martin").unwrap();
        conn.execute(
            "UPDATE characters SET first_name = 'Alice' WHERE id = ?1",
            [alice],
        )
        .unwrap();
        let bob = nodes::create(&conn, NodeKind::Character, "Bob").unwrap();
        let _ = bob;
        let kitchen = nodes::create(&conn, NodeKind::Place, "Cuisine").unwrap();
        let ep = nodes::create_episode(&conn, "Pilote", 1, 1).unwrap();
        let pdf = b"%PDF-1.4\n%%EOF".to_vec();
        let a = attachments::add(&conn, ep, "script", "s.pdf", &pdf).unwrap();
        attachments::set_text(&conn, a, SCRIPT).unwrap();
        let checks = cross_check(&conn).unwrap();
        assert_eq!(checks.len(), 1);
        let c = &checks[0];
        assert_eq!(c.speaker_ids, vec![Some(alice), Some(bob), None]);
        assert_eq!(c.place_ids, vec![Some(kitchen), None]);
        assert_eq!(speech_by_character(&conn, ep).unwrap().len(), 2);
    }
}
