//! Détection d'incohérences scénaristiques — logique pure (aucune dépendance), donc testable seule.
//!
//! Règles :
//! - KNOWLEDGE_TOO_LATE : un personnage intervient dans un événement qui « nécessite » une
//!   connaissance (lien `requires`) qu'il n'apprend que plus tard dans la chronologie.
//! - KNOWLEDGE_MISSING  : il n'apprend jamais cette connaissance.
//! - DATE_MISSING       : impossible de comparer car un des événements n'est pas daté.
//! - RELATION_CONTRADICTION : deux personnages liés par des relations aux libellés différents.

use std::collections::{BTreeMap, HashMap, HashSet};

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
    pub is_character: bool,
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

#[derive(Debug, PartialEq)]
pub struct Finding {
    pub code: &'static str,
    pub severity: Severity,
    pub node_id: i64,
    pub message: String,
}

/// `event_dates` : id de chaque événement → clé de tri de sa date fictive (None = non daté).
pub fn analyze(
    nodes: &[NodeFact],
    event_dates: &HashMap<i64, Option<i64>>,
    links: &[LinkFact],
    knowledge: &[KnowledgeFact],
) -> Vec<Finding> {
    let titles: HashMap<i64, &str> = nodes.iter().map(|n| (n.id, n.title.as_str())).collect();
    let title = |id: i64| titles.get(&id).copied().unwrap_or("?");
    let characters: HashSet<i64> = nodes.iter().filter(|n| n.is_character).map(|n| n.id).collect();
    let mut out = Vec::new();

    for req in links
        .iter()
        .filter(|l| l.link_type == "requires" && event_dates.contains_key(&l.src))
    {
        let (event, subject) = (req.src, req.dst);
        let when = event_dates[&event];

        let mut people: Vec<i64> = links
            .iter()
            .filter(|l| l.link_type == "involves")
            .filter_map(|l| {
                if l.src == event {
                    Some(l.dst)
                } else if l.dst == event {
                    Some(l.src)
                } else {
                    None
                }
            })
            .filter(|id| characters.contains(id))
            .collect();
        people.sort_unstable();
        people.dedup();

        for c in people {
            let known = knowledge.iter().find(|k| k.character == c && k.subject == subject);
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
                    let learned = event_dates.get(&learned_at).copied().flatten();
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

    // Relations contradictoires entre une même paire de personnages.
    let mut pairs: BTreeMap<(i64, i64), Vec<String>> = BTreeMap::new();
    for l in links.iter().filter(|l| l.link_type == "relation") {
        if let Some(label) = l.label.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            pairs
                .entry((l.src.min(l.dst), l.src.max(l.dst)))
                .or_default()
                .push(label.to_lowercase());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: i64, title: &str, is_character: bool) -> NodeFact {
        NodeFact { id, title: title.into(), is_character }
    }
    fn link(src: i64, dst: i64, t: &str, label: Option<&str>) -> LinkFact {
        LinkFact { src, dst, link_type: t.into(), label: label.map(Into::into) }
    }

    // 1 Alice, 2 secret, 10 événement (nécessite le secret), 11 événement où Alice l'apprend
    fn world(learned_date: Option<i64>, event_date: Option<i64>) -> Vec<Finding> {
        let nodes = [node(1, "Alice", true), node(2, "Le secret", false), node(10, "Dîner", false), node(11, "Révélation", false)];
        let dates = HashMap::from([(10, event_date), (11, learned_date)]);
        let links = [link(10, 2, "requires", None), link(10, 1, "involves", None)];
        let know = [KnowledgeFact { character: 1, subject: 2, learned_event: Some(11) }];
        analyze(&nodes, &dates, &links, &know)
    }

    #[test]
    fn learning_after_the_event_is_an_error() {
        let f = world(Some(200), Some(100));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].code, "KNOWLEDGE_TOO_LATE");
        assert_eq!(f[0].severity, Severity::Error);
        assert_eq!(f[0].node_id, 10);
    }

    #[test]
    fn learning_before_or_at_the_same_time_is_fine() {
        assert!(world(Some(50), Some(100)).is_empty());
        assert!(world(Some(100), Some(100)).is_empty());
    }

    #[test]
    fn undated_events_give_an_info_not_an_error() {
        let f = world(None, Some(100));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].severity, Severity::Info);
    }

    #[test]
    fn never_learning_is_a_warning() {
        let nodes = [node(1, "Alice", true), node(2, "Le secret", false), node(10, "Dîner", false)];
        let dates = HashMap::from([(10, Some(100))]);
        let links = [link(10, 2, "requires", None), link(1, 10, "involves", None)]; // sens inverse accepté
        let f = analyze(&nodes, &dates, &links, &[]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].code, "KNOWLEDGE_MISSING");
    }

    #[test]
    fn contradictory_relations_are_flagged_once() {
        let nodes = [node(1, "Alice", true), node(2, "Bob", true)];
        let links = [link(1, 2, "relation", Some("frère")), link(2, 1, "relation", Some("Ennemi"))];
        let f = analyze(&nodes, &HashMap::new(), &links, &[]);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].code, "RELATION_CONTRADICTION");
    }

    #[test]
    fn same_label_both_ways_is_not_a_contradiction() {
        let nodes = [node(1, "A", true), node(2, "B", true)];
        let links = [link(1, 2, "relation", Some("amis")), link(2, 1, "relation", Some("Amis"))];
        assert!(analyze(&nodes, &HashMap::new(), &links, &[]).is_empty());
    }
}
