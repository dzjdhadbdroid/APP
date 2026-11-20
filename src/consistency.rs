//! Détection d'incohérences scénaristiques — logique pure (aucune dépendance), testable seule.
//!
//! Règles (les liens sont ceux que crée l'utilisateur dans les fiches) :
//! - KNOWLEDGE_TOO_LATE / KNOWLEDGE_MISSING / DATE_MISSING : un personnage intervient (« implique »)
//!   dans un événement qui « nécessite » une connaissance qu'il apprend trop tard, ou jamais.
//! - RELATION_CONTRADICTION : deux personnages liés par des relations aux libellés différents.
//! - EVENT_MENTIONED_BEFORE : un événement en « mentionne » un autre qui n'a pas encore eu lieu.
//! - FORBIDDEN_PLACE : une scène « se déroule dans » un lieu (ou un de ses sous-lieux) où un
//!   des personnages qu'elle « implique » « ne doit pas être ».
//! - OBJECT_GAP : un objet figure dans des épisodes d'une intrigue avant et après un épisode
//!   de cette même intrigue où il est absent.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

pub struct NodeFact {
    pub id: i64,
    pub title: String,
    /// Code du type : "character", "place", "episode", "plot", "object", "event", "scene".
    pub kind: String,
}

pub struct LinkFact {
    pub src: i64,
    pub dst: i64,
    pub link_type: String,
    pub label: Option<String>,
}

pub struct KnowledgeFact {
    pub character: i64,
    pub subject: i64,
    pub learned_event: Option<i64>,
}

/// Tout ce que les règles ont besoin de savoir, lu depuis la base.
pub struct Facts<'a> {
    pub nodes: &'a [NodeFact],
    /// id de chaque événement → clé de tri de sa date fictive (None = non daté).
    pub event_dates: &'a HashMap<i64, Option<i64>>,
    /// id de chaque épisode → ordre de diffusion.
    pub air_order: &'a HashMap<i64, i64>,
    /// id d'un sous-lieu → id de son lieu parent.
    pub place_parent: &'a HashMap<i64, i64>,
    pub links: &'a [LinkFact],
    pub knowledge: &'a [KnowledgeFact],
}

#[derive(Debug, PartialEq)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub node_id: i64,
    pub message: String,
}

/// Éléments reliés à `id` par un lien de ce type, quel que soit le sens.
fn neighbors<'a>(links: &'a [LinkFact], id: i64, link_type: &'a str) -> impl Iterator<Item = i64> + 'a {
    links.iter().filter(move |l| l.link_type == link_type).filter_map(move |l| {
        if l.src == id {
            Some(l.dst)
        } else if l.dst == id {
            Some(l.src)
        } else {
            None
        }
    })
}

pub fn analyze(f: &Facts) -> Vec<Finding> {
    let titles: HashMap<i64, &str> = f.nodes.iter().map(|n| (n.id, n.title.as_str())).collect();
    let kinds: HashMap<i64, &str> = f.nodes.iter().map(|n| (n.id, n.kind.as_str())).collect();
    let title = |id: i64| titles.get(&id).copied().unwrap_or("?").to_string();
    let is = |id: i64, kind: &str| kinds.get(&id).copied() == Some(kind);
    let mut out = Vec::new();

    knowledge_rule(f, &title, &is, &mut out);
    mention_rule(f, &title, &mut out);
    forbidden_place_rule(f, &title, &is, &mut out);
    object_gap_rule(f, &title, &is, &mut out);

    // Relations contradictoires entre une même paire de personnages.
    let mut pairs: BTreeMap<(i64, i64), Vec<String>> = BTreeMap::new();
    for l in f.links.iter().filter(|l| l.link_type == "relation") {
        if let Some(label) = l.label.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            pairs.entry((l.src.min(l.dst), l.src.max(l.dst))).or_default().push(label.to_lowercase());
        }
    }
    for ((a, b), mut labels) in pairs {
        labels.sort();
        labels.dedup();
        if labels.len() >= 2 {
            out.push(Finding {
                code: "RELATION_CONTRADICTION",
                severity: Severity::Warning,
                node_id: a,
                message: format!(
                    "Relations différentes entre {} et {} : « {} ».",
                    title(a),
                    title(b),
                    labels.join(" » / « ")
                ),
            });
        }
    }
    out
}

fn knowledge_rule(f: &Facts, title: &dyn Fn(i64) -> String, is: &dyn Fn(i64, &str) -> bool, out: &mut Vec<Finding>) {
    for req in f.links.iter().filter(|l| l.link_type == "requires" && f.event_dates.contains_key(&l.src)) {
        let (event, subject) = (req.src, req.dst);
        let when = f.event_dates[&event];
        let mut people: Vec<i64> = neighbors(f.links, event, "involves").filter(|id| is(*id, "character")).collect();
        people.sort_unstable();
        people.dedup();
        for c in people {
            let known = f.knowledge.iter().find(|k| k.character == c && k.subject == subject);
            match known {
                None => out.push(Finding {
                    code: "KNOWLEDGE_MISSING",
                    severity: Severity::Warning,
                    node_id: event,
                    message: format!(
                        "{} intervient dans « {} » qui nécessite « {} », mais ne l'apprend jamais.",
                        title(c),
                        title(event),
                        title(subject)
                    ),
                }),
                Some(k) => {
                    let Some(learned_at) = k.learned_event else { continue };
                    let learned = f.event_dates.get(&learned_at).copied().flatten();
                    match (learned, when) {
                        (Some(l), Some(w)) if l > w => out.push(Finding {
                            code: "KNOWLEDGE_TOO_LATE",
                            severity: Severity::Error,
                            node_id: event,
                            message: format!(
                                "{} intervient dans « {} » avant d'avoir appris « {} » (appris à « {} », plus tard).",
                                title(c),
                                title(event),
                                title(subject),
                                title(learned_at)
                            ),
                        }),
                        (Some(_), Some(_)) => {}
                        _ => out.push(Finding {
                            code: "DATE_MISSING",
                            severity: Severity::Info,
                            node_id: event,
                            message: format!(
                                "Vérification impossible pour {} : « {} » ou « {} » n'est pas daté.",
                                title(c),
                                title(event),
                                title(learned_at)
                            ),
                        }),
                    }
                }
            }
        }
    }
}

fn mention_rule(f: &Facts, title: &dyn Fn(i64) -> String, out: &mut Vec<Finding>) {
    for l in f.links.iter().filter(|l| l.link_type == "mentions") {
        let (Some(Some(from)), Some(Some(to))) = (f.event_dates.get(&l.src), f.event_dates.get(&l.dst)) else {
            continue; // seuls des événements datés sont comparables
        };
        if from < to {
            out.push(Finding {
                code: "EVENT_MENTIONED_BEFORE",
                severity: Severity::Warning,
                node_id: l.src,
                message: format!(
                    "« {} » mentionne « {} » avant que cet événement n'ait eu lieu.",
                    title(l.src),
                    title(l.dst)
                ),
            });
        }
    }
}

fn forbidden_place_rule(f: &Facts, title: &dyn Fn(i64) -> String, is: &dyn Fn(i64, &str) -> bool, out: &mut Vec<Finding>) {
    for scene in f.nodes.iter().filter(|n| n.kind == "scene") {
        // Le lieu de la scène et tous ses lieux parents (un appartement est dans un immeuble).
        let mut places: Vec<i64> = Vec::new();
        for p in neighbors(f.links, scene.id, "set_in").filter(|p| is(*p, "place")) {
            let mut cur = Some(p);
            while let Some(c) = cur {
                if places.contains(&c) || places.len() > 64 {
                    break;
                }
                places.push(c);
                cur = f.place_parent.get(&c).copied();
            }
        }
        if places.is_empty() {
            continue;
        }
        let mut people: Vec<i64> = neighbors(f.links, scene.id, "involves").filter(|id| is(*id, "character")).collect();
        people.sort_unstable();
        people.dedup();
        for c in people {
            for forbidden in neighbors(f.links, c, "not_at") {
                if places.contains(&forbidden) {
                    out.push(Finding {
                        code: "FORBIDDEN_PLACE",
                        severity: Severity::Error,
                        node_id: scene.id,
                        message: format!(
                            "{} apparaît dans la scène « {} » (lieu : « {} ») alors qu'il/elle ne devrait pas être à « {} ».",
                            title(c),
                            title(scene.id),
                            title(places[0]),
                            title(forbidden)
                        ),
                    });
                }
            }
        }
    }
}

fn object_gap_rule(f: &Facts, title: &dyn Fn(i64) -> String, is: &dyn Fn(i64, &str) -> bool, out: &mut Vec<Finding>) {
    for obj in f.nodes.iter().filter(|n| n.kind == "object") {
        let appears: HashSet<i64> = neighbors(f.links, obj.id, "appears_in").filter(|e| is(*e, "episode")).collect();
        if appears.len() < 2 {
            continue;
        }
        let plots: BTreeSet<i64> = f
            .links
            .iter()
            .filter_map(|l| {
                if l.src == obj.id {
                    Some(l.dst)
                } else if l.dst == obj.id {
                    Some(l.src)
                } else {
                    None
                }
            })
            .filter(|p| is(*p, "plot"))
            .collect();
        for plot in plots {
            // Épisodes de l'intrigue, dans l'ordre de diffusion.
            let mut episodes: Vec<(i64, i64)> = neighbors(f.links, plot, "advances")
                .filter(|e| is(*e, "episode"))
                .filter_map(|e| f.air_order.get(&e).map(|o| (*o, e)))
                .collect();
            episodes.sort_unstable();
            episodes.dedup();
            let present: Vec<usize> =
                episodes.iter().enumerate().filter(|(_, (_, e))| appears.contains(e)).map(|(i, _)| i).collect();
            let (Some(&first), Some(&last)) = (present.first(), present.last()) else { continue };
            for (i, (_, e)) in episodes.iter().enumerate() {
                if i > first && i < last && !appears.contains(e) {
                    out.push(Finding {
                        code: "OBJECT_GAP",
                        severity: Severity::Info,
                        node_id: obj.id,
                        message: format!(
                            "L'objet « {} » n'apparaît pas dans « {} » (intrigue « {} ») alors qu'il figure dans des épisodes précédent et suivant.",
                            title(obj.id),
                            title(*e),
                            title(plot)
                        ),
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: i64, title: &str, kind: &str) -> NodeFact {
        NodeFact { id, title: title.into(), kind: kind.into() }
    }
    fn link(src: i64, dst: i64, t: &str, label: Option<&str>) -> LinkFact {
        LinkFact { src, dst, link_type: t.into(), label: label.map(Into::into) }
    }
    fn run(
        nodes: &[NodeFact],
        dates: &[(i64, Option<i64>)],
        air: &[(i64, i64)],
        parents: &[(i64, i64)],
        links: &[LinkFact],
        know: &[KnowledgeFact],
    ) -> Vec<Finding> {
        let dates: HashMap<_, _> = dates.iter().copied().collect();
        let air: HashMap<_, _> = air.iter().copied().collect();
        let parents: HashMap<_, _> = parents.iter().copied().collect();
        analyze(&Facts {
            nodes,
            event_dates: &dates,
            air_order: &air,
            place_parent: &parents,
            links,
            knowledge: know,
        })
    }

    // 1 Alice, 2 secret, 10 événement (nécessite le secret), 11 événement où Alice l'apprend
    fn knowledge_world(learned_date: Option<i64>, event_date: Option<i64>) -> Vec<Finding> {
        let nodes = [node(1, "Alice", "character"), node(2, "Le secret", "plot"), node(10, "Dîner", "event"), node(11, "Révélation", "event")];
        let links = [link(10, 2, "requires", None), link(10, 1, "involves", None)];
        let know = [KnowledgeFact { character: 1, subject: 2, learned_event: Some(11) }];
        run(&nodes, &[(10, event_date), (11, learned_date)], &[], &[], &links, &know)
    }

    #[test]
    fn learning_after_the_event_is_an_error() {
        let f = knowledge_world(Some(200), Some(100));
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].code, f[0].severity, f[0].node_id), ("KNOWLEDGE_TOO_LATE", Severity::Error, 10));
    }

    #[test]
    fn learning_before_or_at_the_same_time_is_fine() {
        assert!(knowledge_world(Some(50), Some(100)).is_empty());
        assert!(knowledge_world(Some(100), Some(100)).is_empty());
    }

    #[test]
    fn undated_events_give_an_info_not_an_error() {
        let f = knowledge_world(None, Some(100));
        assert_eq!((f.len(), f[0].severity), (1, Severity::Info));
    }

    #[test]
    fn never_learning_is_a_warning() {
        let nodes = [node(1, "Alice", "character"), node(2, "Le secret", "plot"), node(10, "Dîner", "event")];
        let links = [link(10, 2, "requires", None), link(1, 10, "involves", None)]; // sens inverse accepté
        let f = run(&nodes, &[(10, Some(100))], &[], &[], &links, &[]);
        assert_eq!((f.len(), f[0].code), (1, "KNOWLEDGE_MISSING"));
    }

    #[test]
    fn contradictory_relations_are_flagged_once() {
        let nodes = [node(1, "Alice", "character"), node(2, "Bob", "character")];
        let links = [link(1, 2, "relation", Some("frère")), link(2, 1, "relation", Some("Ennemi"))];
        let f = run(&nodes, &[], &[], &[], &links, &[]);
        assert_eq!((f.len(), f[0].code), (1, "RELATION_CONTRADICTION"));
    }

    #[test]
    fn same_label_both_ways_is_not_a_contradiction() {
        let nodes = [node(1, "A", "character"), node(2, "B", "character")];
        let links = [link(1, 2, "relation", Some("amis")), link(2, 1, "relation", Some("Amis"))];
        assert!(run(&nodes, &[], &[], &[], &links, &[]).is_empty());
    }

    #[test]
    fn mentioning_an_event_before_it_happens_is_flagged() {
        let nodes = [node(10, "Mariage", "event"), node(11, "Toast", "event")];
        let links = [link(11, 10, "mentions", None)];
        let f = run(&nodes, &[(10, Some(200)), (11, Some(100))], &[], &[], &links, &[]);
        assert_eq!((f.len(), f[0].code, f[0].node_id), (1, "EVENT_MENTIONED_BEFORE", 11));
        assert!(run(&nodes, &[(10, Some(100)), (11, Some(200))], &[], &[], &links, &[]).is_empty()); // après : normal
        assert!(run(&nodes, &[(10, None), (11, Some(100))], &[], &[], &links, &[]).is_empty()); // non daté : ignoré
    }

    // 1 Alice · 20 scène · 30 appartement ∈ 31 immeuble · 32 jardin
    fn place_world(set_in: i64, not_at: i64) -> Vec<Finding> {
        let nodes = [
            node(1, "Alice", "character"),
            node(20, "Scène 1", "scene"),
            node(30, "Appartement", "place"),
            node(31, "Immeuble", "place"),
            node(32, "Jardin", "place"),
        ];
        let links = [link(20, set_in, "set_in", None), link(20, 1, "involves", None), link(1, not_at, "not_at", None)];
        run(&nodes, &[], &[], &[(30, 31)], &links, &[])
    }

    #[test]
    fn character_in_a_forbidden_place_is_an_error() {
        let f = place_world(30, 30);
        assert_eq!((f.len(), f[0].code, f[0].severity), (1, "FORBIDDEN_PLACE", Severity::Error));
    }

    #[test]
    fn forbidden_parent_place_also_counts() {
        assert_eq!(place_world(30, 31).len(), 1); // appartement dans l'immeuble interdit
        assert!(place_world(30, 32).is_empty()); // un autre lieu : rien à signaler
        assert!(place_world(32, 30).is_empty()); // jardin : pas dans l'appartement
    }

    #[test]
    fn a_cycle_in_place_parents_does_not_hang() {
        let nodes = [node(1, "A", "character"), node(20, "S", "scene"), node(30, "P", "place"), node(31, "Q", "place")];
        let links = [link(20, 30, "set_in", None), link(20, 1, "involves", None)];
        assert!(run(&nodes, &[], &[], &[(30, 31), (31, 30)], &links, &[]).is_empty());
    }

    // objet 1 ∈ intrigue 2 ; épisodes 10,11,12 (ordres 1,2,3) font avancer l'intrigue
    fn object_world(present_in: &[i64]) -> Vec<Finding> {
        let nodes = [
            node(1, "Clé", "object"),
            node(2, "Le trésor", "plot"),
            node(10, "Ép. 1", "episode"),
            node(11, "Ép. 2", "episode"),
            node(12, "Ép. 3", "episode"),
        ];
        let mut links = vec![link(1, 2, "owns", None)];
        for e in [10, 11, 12] {
            links.push(link(e, 2, "advances", None));
        }
        for e in present_in {
            links.push(link(1, *e, "appears_in", None));
        }
        run(&nodes, &[], &[(10, 1), (11, 2), (12, 3)], &[], &links, &[])
    }

    #[test]
    fn an_object_missing_between_two_appearances_is_noted() {
        let f = object_world(&[10, 12]);
        assert_eq!((f.len(), f[0].code, f[0].severity), (1, "OBJECT_GAP", Severity::Info));
        assert!(f[0].message.contains("Ép. 2"));
    }

    #[test]
    fn no_gap_means_no_finding() {
        assert!(object_world(&[10, 11, 12]).is_empty());
        assert!(object_world(&[10, 11]).is_empty()); // disparaît à la fin : pas un « trou »
        assert!(object_world(&[12]).is_empty());
    }
}
