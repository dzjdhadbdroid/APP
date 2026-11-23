//! Assistant scénaristique — logique pure (aucune dépendance) : contexte envoyé au modèle,
//! consignes, vérification des citations. Testable seule.
//!
//! Garanties de conception :
//! - le modèle ne reçoit QUE ce que l'utilisateur a sélectionné (plus, s'il le demande, les voisins directs) ;
//! - chaque élément porte une étiquette `[#id]` que le modèle doit citer ;
//! - les citations de la réponse sont vérifiées : un `[#id]` absent du contexte est signalé ;
//! - le modèle ne produit que du texte : rien n'est jamais écrit dans la base sans validation.

use std::collections::HashSet;

pub struct Conn {
    /// ex. « → frère » ou « ← habite »
    pub text: String,
    pub target: i64,
    pub target_title: String,
}

pub struct ContextItem {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub when: Option<String>,
    pub fields: Vec<(String, String)>,
    pub notes: String,
    pub connections: Vec<Conn>,
    pub knowledge: Vec<String>,
}

pub fn render_item(it: &ContextItem) -> String {
    let mut s = format!("[#{}] {} « {} »\n", it.id, it.kind, it.title);
    if let Some(w) = &it.when {
        s.push_str(&format!("Date dans l'univers : {w}\n"));
    }
    if !it.summary.is_empty() {
        s.push_str(&format!("Résumé : {}\n", it.summary));
    }
    for (label, value) in &it.fields {
        s.push_str(&format!("{label} : {value}\n"));
    }
    if !it.notes.is_empty() {
        s.push_str(&format!("Notes : {}\n", it.notes));
    }
    for k in &it.knowledge {
        s.push_str(&format!("Sait : {k}\n"));
    }
    for c in &it.connections {
        s.push_str(&format!(
            "Lien {} « {} » [#{}]\n",
            c.text, c.target_title, c.target
        ));
    }
    s
}

fn truncate_chars(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Assemble les éléments dans l'ordre jusqu'à `max_chars` caractères.
/// Retourne le texte et le nombre d'éléments inclus (les suivants sont omis, jamais coupés
/// en plein milieu — sauf le premier s'il dépasse à lui seul le budget).
pub fn render_context(items: &[ContextItem], max_chars: usize) -> (String, usize) {
    let mut out = String::new();
    let mut used = 0usize;
    let mut included = 0usize;
    for it in items {
        let piece = render_item(it);
        let len = piece.chars().count() + 1;
        if included > 0 && used + len > max_chars {
            break;
        }
        if included == 0 && len > max_chars {
            out.push_str(&truncate_chars(&piece, max_chars));
            out.push('\n');
            included = 1;
            break;
        }
        out.push_str(&piece);
        out.push('\n');
        used += len;
        included += 1;
    }
    (out, included)
}

/// Identifiants que le modèle a le droit de citer : éléments du contexte + cibles de leurs liens.
pub fn allowed_ids(items: &[ContextItem]) -> HashSet<i64> {
    let mut ids = HashSet::new();
    for it in items {
        ids.insert(it.id);
        for c in &it.connections {
            ids.insert(c.target);
        }
    }
    ids
}

/// Extrait les `[#123]` d'une réponse. Retourne (citations valides, citations inconnues),
/// sans doublons, dans l'ordre d'apparition.
pub fn extract_citations(text: &str, allowed: &HashSet<i64>) -> (Vec<i64>, Vec<i64>) {
    let b = text.as_bytes();
    let (mut valid, mut unknown) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i + 2 < b.len() {
        if b[i] == b'[' && b[i + 1] == b'#' {
            let mut j = i + 2;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 2 && j < b.len() && b[j] == b']' {
                if let Ok(id) = text[i + 2..j].parse::<i64>() {
                    let list = if allowed.contains(&id) {
                        &mut valid
                    } else {
                        &mut unknown
                    };
                    if !list.contains(&id) {
                        list.push(id);
                    }
                }
                i = j;
            }
        }
        i += 1;
    }
    (valid, unknown)
}

pub const SYSTEM_PROMPT: &str = "Tu es l'assistant d'un scénariste qui écrit une sitcom. \
Tu travailles UNIQUEMENT à partir du CONTEXTE fourni, extrait de sa « bible » de série. \
Règles impératives :\n\
1. Appuie chaque affirmation sur le contexte et cite tes sources sous la forme [#id], \
par exemple [#12] ou [#12] [#15]. N'invente jamais d'identifiant.\n\
2. Si le contexte ne permet pas de répondre, dis-le clairement au lieu de deviner.\n\
3. Distingue ce qui est établi dans le contexte (cité) de ce qui est ta proposition (non cité).\n\
4. Tu ne modifies rien : tu proposes du texte que le scénariste validera lui-même.\n\
5. Réponds en français.";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Task {
    PersonalitySummary,
    Relations,
    EpisodeInconsistencies,
    KnowledgeCheck,
    BiographyContradictions,
    PlotIdeas,
    Interactions,
    RunningGags,
    Evolution,
    RecapBeforeEpisode,
    Free,
}

impl Task {
    pub const ALL: [Task; 11] = [
        Task::PersonalitySummary,
        Task::Relations,
        Task::EpisodeInconsistencies,
        Task::KnowledgeCheck,
        Task::BiographyContradictions,
        Task::PlotIdeas,
        Task::Interactions,
        Task::RunningGags,
        Task::Evolution,
        Task::RecapBeforeEpisode,
        Task::Free,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Task::PersonalitySummary => "Résumer la personnalité d'un personnage",
            Task::Relations => "Retrouver les relations d'un personnage",
            Task::EpisodeInconsistencies => "Repérer les incohérences entre épisodes",
            Task::KnowledgeCheck => "Vérifier qu'un personnage ne sait pas ce qu'il n'a pas appris",
            Task::BiographyContradictions => "Repérer les contradictions dans les biographies",
            Task::PlotIdeas => "Proposer des idées d'intrigues",
            Task::Interactions => "Suggérer des interactions entre personnages",
            Task::RunningGags => "Retrouver les running gags existants",
            Task::Evolution => "Analyser l'évolution d'un personnage",
            Task::RecapBeforeEpisode => "Résumer les événements avant d'écrire un nouvel épisode",
            Task::Free => "Question libre",
        }
    }

    pub fn instruction(self) -> &'static str {
        match self {
            Task::PersonalitySummary => "Résume la personnalité du ou des personnages du contexte : traits, qualités, défauts, peurs, désirs, contradictions.",
            Task::Relations => "Liste toutes les relations des personnages du contexte (famille, amitiés, amours, rivalités, conflits, travail), avec leur nature et ce qui est établi à leur sujet.",
            Task::EpisodeInconsistencies => "Repère les incohérences possibles entre les épisodes, scènes et événements du contexte (faits qui se contredisent, chronologie, comportements). Pour chacune, cite les deux éléments en conflit.",
            Task::KnowledgeCheck => "Vérifie si un personnage dit, fait ou sait quelque chose qu'il n'a pas pu apprendre d'après le contexte (utilise les lignes « Sait » et les dates). Cite les passages concernés.",
            Task::BiographyContradictions => "Repère les contradictions entre les biographies, dates, lieux et faits des personnages du contexte.",
            Task::PlotIdeas => "Propose 5 idées d'intrigues originales qui exploitent les personnages, lieux, objets et intrigues du contexte, en t'appuyant sur ce qui est établi.",
            Task::Interactions => "Suggère des interactions et situations comiques entre les personnages du contexte, fondées sur leurs traits, relations et conflits existants.",
            Task::RunningGags => "Retrouve les running gags existants dans le contexte et indique où ils apparaissent ; propose des occasions de les relancer.",
            Task::Evolution => "Analyse l'évolution du ou des personnages au fil des épisodes et événements du contexte : situation initiale, étapes, changements.",
            Task::RecapBeforeEpisode => "Fais un récapitulatif des événements, intrigues en cours et informations connues de chacun, utile avant d'écrire un nouvel épisode.",
            Task::Free => "Réponds à la question du scénariste.",
        }
    }
}

pub fn build_user_prompt(task: Task, question: &str, context: &str) -> String {
    let mut s = format!(
        "CONTEXTE (bible de la série) :\n{context}\n---\nTÂCHE : {}\n",
        task.instruction()
    );
    let q = question.trim();
    if !q.is_empty() {
        s.push_str(&format!("\nPRÉCISIONS / QUESTION DU SCÉNARISTE :\n{q}\n"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: i64, title: &str) -> ContextItem {
        ContextItem {
            id,
            kind: "Personnage".into(),
            title: title.into(),
            summary: String::new(),
            when: None,
            fields: vec![("Peurs".into(), "les araignées".into())],
            notes: String::new(),
            connections: vec![],
            knowledge: vec![],
        }
    }

    #[test]
    fn items_carry_their_citation_tag() {
        let s = render_item(&item(12, "Alice"));
        assert!(s.starts_with("[#12] Personnage « Alice »"));
        assert!(s.contains("Peurs : les araignées"));
    }

    #[test]
    fn connections_and_knowledge_are_rendered_and_citable() {
        let mut a = item(1, "Alice");
        a.connections.push(Conn {
            text: "→ frère".into(),
            target: 2,
            target_title: "Bob".into(),
        });
        a.knowledge
            .push("« Le secret » — appris à « Révélation »".into());
        let s = render_item(&a);
        assert!(s.contains("Lien → frère « Bob » [#2]"));
        assert!(s.contains("Sait : « Le secret »"));
        assert_eq!(allowed_ids(&[a]), HashSet::from([1, 2]));
    }

    #[test]
    fn citations_are_split_into_valid_and_unknown() {
        let allowed = HashSet::from([1, 2]);
        let (v, u) = extract_citations(
            "Alice [#1] craint [#2] mais [#99] et encore [#1], [#x], [#], [#3",
            &allowed,
        );
        assert_eq!(v, vec![1, 2]);
        assert_eq!(u, vec![99]);
    }

    #[test]
    fn citations_survive_accents_and_edges() {
        let allowed = HashSet::from([7]);
        let (v, u) = extract_citations("été [#7]é[#7]", &allowed);
        assert_eq!((v, u), (vec![7], vec![]));
        assert_eq!(extract_citations("", &allowed), (vec![], vec![]));
        assert_eq!(extract_citations("[#", &allowed), (vec![], vec![]));
    }

    #[test]
    fn budget_omits_whole_items_and_never_drops_the_first() {
        let items: Vec<_> = (1..=5).map(|i| item(i, "x")).collect();
        let one = render_item(&items[0]).chars().count() + 1;
        let (text, n) = render_context(&items, one * 2 + 1);
        assert_eq!(n, 2);
        assert!(text.contains("[#2]") && !text.contains("[#3]"));
        let (tiny, n) = render_context(&items, 10);
        assert_eq!(n, 1);
        assert_eq!(tiny.trim_end().chars().count(), 10); // tronqué sans casser un caractère
    }

    #[test]
    fn truncation_is_char_safe() {
        let mut a = item(1, "Élodie");
        a.summary = "éééééééééé".into();
        let (t, n) = render_context(&[a], 20);
        assert_eq!(n, 1);
        assert!(t.chars().count() <= 21);
    }

    #[test]
    fn prompt_contains_context_task_and_question() {
        let p = build_user_prompt(Task::PlotIdeas, "  pour la saison 2 ", "CTX");
        assert!(
            p.contains("CTX")
                && p.contains("5 idées d'intrigues")
                && p.contains("pour la saison 2")
        );
        assert!(!build_user_prompt(Task::Free, "  ", "CTX").contains("PRÉCISIONS"));
        assert!(SYSTEM_PROMPT.contains("[#id]"));
    }

    #[test]
    fn every_task_has_label_and_instruction() {
        for t in Task::ALL {
            assert!(!t.label().is_empty() && !t.instruction().is_empty());
        }
    }
}
