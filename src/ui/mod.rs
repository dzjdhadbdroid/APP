//! Interface egui : fenêtre principale, navigation, tableau de bord, fiches, recherche.
//! Toute écriture passe par `apply`, qui recharge aussitôt les données partagées :
//! Universe Map, Timeline et fiches voient donc les modifications immédiatement.

mod alerts_view;
mod graph_view;
mod timeline_view;

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui::{self, Color32, RichText};
use rusqlite::Connection;

use crate::db::{
    self,
    alerts::{self, AlertRow},
    fields::{self, FieldKind},
    knowledge::{self, KnowledgeRow},
    links::{self, Link},
    nodes::{self, Node, NodeKind},
    DbError, DbResult,
};

/// (code stocké en base, libellé affiché)
pub const LINK_TYPES: [(&str, &str); 11] = [
    ("relation", "relation entre personnages"),
    ("lives_in", "habite"),
    ("located_at", "se situe à"),
    ("appears_in", "apparaît dans"),
    ("contains", "contient"),
    ("set_in", "se déroule dans"),
    ("advances", "fait avancer"),
    ("owns", "possède"),
    ("involves", "implique"),
    ("requires", "nécessite (connaissance préalable)"),
    ("mentions", "mentionne"),
];

pub fn link_type_label(t: &str) -> &str {
    LINK_TYPES
        .iter()
        .find(|(k, _)| *k == t)
        .map(|(_, l)| *l)
        .unwrap_or(t)
}

pub fn kind_color(kind: NodeKind) -> Color32 {
    match kind {
        NodeKind::Character => Color32::from_rgb(110, 170, 255),
        NodeKind::Place => Color32::from_rgb(110, 210, 140),
        NodeKind::Episode => Color32::from_rgb(255, 190, 90),
        NodeKind::Plot => Color32::from_rgb(220, 120, 220),
        NodeKind::Object => Color32::from_rgb(240, 230, 110),
        NodeKind::Event => Color32::from_rgb(255, 120, 120),
        NodeKind::Scene => Color32::from_rgb(150, 150, 170),
    }
}

fn title_in(nodes: &[Node], id: i64) -> &str {
    nodes
        .iter()
        .find(|n| n.id == id)
        .map_or("(supprimé)", |n| n.title.as_str())
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Dashboard,
    Kind(NodeKind),
    Timeline,
    Graph,
    Alerts,
}

struct Edit {
    id: i64,
    title: String,
    summary: String,
    notes: String,
    /// Champs détaillés, dans l'ordre de `fields::defs(kind)`.
    fields: Vec<String>,
}

#[derive(Default)]
struct KnowForm {
    subject: Option<i64>,
    event: Option<i64>,
}

#[derive(Default)]
struct LinkForm {
    target: Option<i64>,
    link_type: usize,
    label: String,
}

pub struct StudioApp {
    conn: Connection,
    data_dir: PathBuf,
    view: View,
    nodes: Vec<Node>,
    links: Vec<Link>,
    selected: Option<i64>,
    edit: Option<Edit>,
    new_title: String,
    search: String,
    search_results: Vec<Node>,
    status: String,
    link_form: LinkForm,
    know_form: KnowForm,
    alerts: Vec<AlertRow>,
    show_ignored: bool,
    graph: graph_view::GraphState,
    timeline: timeline_view::TimelineState,
}

impl StudioApp {
    pub fn new(conn: Connection, data_dir: PathBuf) -> Self {
        let mut app = Self {
            conn,
            data_dir,
            view: View::Dashboard,
            nodes: Vec::new(),
            links: Vec::new(),
            selected: None,
            edit: None,
            new_title: String::new(),
            search: String::new(),
            search_results: Vec::new(),
            status: String::from("Prêt."),
            link_form: LinkForm::default(),
            know_form: KnowForm::default(),
            alerts: Vec::new(),
            show_ignored: false,
            graph: graph_view::GraphState::new(),
            timeline: timeline_view::TimelineState::default(),
        };
        app.reload();
        app
    }

    /// Recharge la source de données commune. Appelé après chaque écriture.
    fn reload(&mut self) {
        match (nodes::all(&self.conn), links::all(&self.conn)) {
            (Ok(n), Ok(l)) => {
                self.nodes = n;
                self.links = l;
            }
            (Err(e), _) | (_, Err(e)) => self.status = e.to_string(),
        }
        if let Some(id) = self.selected {
            if !self.nodes.iter().any(|n| n.id == id) {
                self.selected = None;
                self.edit = None;
            }
        }
        if let Some(f) = self.graph.focus {
            if !self.nodes.iter().any(|n| n.id == f) {
                self.graph.focus = None;
            }
        }
        // Alertes de cohérence : recalculées sur la même source de données.
        match alerts::refresh(&self.conn).and_then(|_| alerts::list(&self.conn)) {
            Ok(a) => self.alerts = a,
            Err(e) => self.status = format!("Erreur d'analyse : {e}"),
        }
        self.refresh_search();
        self.graph.settle = 300; // le graphe se réorganise après chaque changement
    }

    fn refresh_search(&mut self) {
        self.search_results = if self.search.trim().is_empty() {
            Vec::new()
        } else {
            nodes::search_text(&self.conn, &self.search).unwrap_or_default()
        };
    }

    /// Exécute une écriture ; en cas de succès recharge tout, sinon affiche l'erreur.
    fn apply<T>(&mut self, f: impl FnOnce(&Connection) -> DbResult<T>) -> Option<T> {
        match f(&self.conn) {
            Ok(v) => {
                self.status = String::from("Enregistré.");
                self.reload();
                Some(v)
            }
            Err(e) => {
                self.status = format!("Erreur : {e}");
                None
            }
        }
    }

    fn select(&mut self, id: i64) {
        self.selected = Some(id);
        let conn = &self.conn;
        self.edit = self.nodes.iter().find(|n| n.id == id).map(|n| {
            let mut f = fields::load(conn, n.kind, id).unwrap_or_default();
            f.resize(fields::defs(n.kind).len(), String::new());
            Edit {
                id,
                title: n.title.clone(),
                summary: n.summary.clone(),
                notes: n.notes.clone(),
                fields: f,
            }
        });
        self.link_form = LinkForm::default();
        self.know_form = KnowForm::default();
    }

    /// Ouvre la fiche d'un élément dans son module.
    fn go_to(&mut self, id: i64) {
        if let Some(kind) = self.nodes.iter().find(|n| n.id == id).map(|n| n.kind) {
            self.view = View::Kind(kind);
            self.search.clear();
            self.refresh_search();
            self.select(id);
        }
    }

    // ───────────────────────── barre latérale ─────────────────────────

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.heading("Sitcom Studio");
        ui.add_space(6.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Recherche globale…")
                .desired_width(f32::INFINITY),
        );
        if resp.changed() {
            self.refresh_search();
        }
        ui.separator();

        let open_alerts = self.alerts.iter().filter(|a| a.status == "open").count();
        let items = [
            ("Tableau de bord".to_string(), View::Dashboard),
            ("Personnages".to_string(), View::Kind(NodeKind::Character)),
            ("Lieux".to_string(), View::Kind(NodeKind::Place)),
            ("Épisodes".to_string(), View::Kind(NodeKind::Episode)),
            ("Intrigues".to_string(), View::Kind(NodeKind::Plot)),
            ("Objets".to_string(), View::Kind(NodeKind::Object)),
            ("Événements".to_string(), View::Kind(NodeKind::Event)),
            ("Chronologie".to_string(), View::Timeline),
            ("Graphe de l'univers".to_string(), View::Graph),
            (format!("Alertes ({open_alerts})"), View::Alerts),
        ];
        for (label, v) in items {
            if ui.selectable_label(self.view == v, label).clicked() {
                self.view = v;
                self.search.clear();
                self.refresh_search();
            }
        }
        ui.separator();
        if ui.button("Sauvegarder maintenant").clicked() {
            self.status = match db::manual_backup(&self.conn, &self.data_dir) {
                Ok(p) => format!("Sauvegarde créée : {}", p.display()),
                Err(e) => format!("Erreur de sauvegarde : {e}"),
            };
        }
    }

    // ───────────────────────── tableau de bord ─────────────────────────

    fn dashboard(&mut self, ui: &mut egui::Ui) {
        ui.heading("Tableau de bord");
        ui.add_space(8.0);
        let mut goto = None;
        let mut open = None;

        ui.horizontal_wrapped(|ui| {
            for kind in [
                NodeKind::Character,
                NodeKind::Place,
                NodeKind::Episode,
                NodeKind::Plot,
                NodeKind::Object,
                NodeKind::Event,
            ] {
                let n = self.nodes.iter().filter(|x| x.kind == kind).count();
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(n.to_string()).size(30.0).color(kind_color(kind)));
                        if ui.link(kind.label_plural()).clicked() {
                            goto = Some(View::Kind(kind));
                        }
                    });
                });
            }
        });

        ui.add_space(12.0);
        ui.heading("Derniers éléments modifiés");
        let mut recent: Vec<&Node> = self.nodes.iter().collect();
        recent.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(b.id.cmp(&a.id)));
        for n in recent.into_iter().take(8) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(n.kind.label()).color(kind_color(n.kind)));
                if ui.link(&n.title).clicked() {
                    open = Some(n.id);
                }
            });
        }

        ui.add_space(12.0);
        let linked: HashSet<i64> = self
            .links
            .iter()
            .flat_map(|l| [l.src_id, l.dst_id])
            .collect();
        let isolated: Vec<&Node> = self.nodes.iter().filter(|n| !linked.contains(&n.id)).collect();
        ui.heading(format!("Éléments isolés ({})", isolated.len()));
        for n in isolated.into_iter().take(8) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(n.kind.label()).color(kind_color(n.kind)));
                if ui.link(&n.title).clicked() {
                    open = Some(n.id);
                }
            });
        }
        ui.add_space(12.0);
        let count = |sev: &str| self.alerts.iter().filter(|a| a.status == "open" && a.severity == sev).count();
        ui.heading("Alertes de cohérence");
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} erreur(s)", count("error"))).color(Color32::from_rgb(255, 90, 90)));
            ui.label(RichText::new(format!("{} attention", count("warning"))).color(Color32::from_rgb(255, 180, 80)));
            ui.label(RichText::new(format!("{} info", count("info"))).color(Color32::from_rgb(120, 190, 255)));
            if ui.link("Voir les alertes").clicked() {
                goto = Some(View::Alerts);
            }
        });

        if let Some(v) = goto {
            self.view = v;
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    // ───────────────────────── recherche globale ─────────────────────────

    fn search_view(&mut self, ui: &mut egui::Ui) {
        ui.heading(format!("Résultats pour « {} »", self.search.trim()));
        let mut open = None;
        if self.search_results.is_empty() {
            ui.label("Aucun résultat.");
        }
        for n in &self.search_results {
            ui.horizontal(|ui| {
                ui.label(RichText::new(n.kind.label()).color(kind_color(n.kind)));
                if ui.link(&n.title).clicked() {
                    open = Some(n.id);
                }
                if !n.summary.is_empty() {
                    ui.label(RichText::new(&n.summary).color(Color32::from_gray(140)));
                }
            });
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    // ───────────────────────── modules à fiches ─────────────────────────

    fn kind_view(&mut self, ui: &mut egui::Ui, kind: NodeKind) {
        ui.heading(kind.label_plural());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.new_title)
                    .hint_text(format!("Nouveau : {}", kind.label().to_lowercase())),
            );
            if ui.button("Créer").clicked() && !self.new_title.trim().is_empty() {
                let title = self.new_title.trim().to_string();
                let created = self.apply(|c| match kind {
                    NodeKind::Episode => {
                        let n = nodes::next_episode_number(c, 1)?;
                        nodes::create_episode(c, &title, 1, n)
                    }
                    NodeKind::Scene => Err(DbError::Invalid(
                        "une scène se crée depuis la fiche de son épisode".into(),
                    )),
                    _ => nodes::create(c, kind, &title),
                });
                if let Some(id) = created {
                    self.new_title.clear();
                    self.select(id);
                }
            }
        });
        ui.separator();
        ui.columns(2, |cols| {
            self.list_column(&mut cols[0], kind);
            self.detail_column(&mut cols[1]);
        });
    }

    fn list_column(&mut self, ui: &mut egui::Ui, kind: NodeKind) {
        let items: Vec<(i64, String)> = self
            .nodes
            .iter()
            .filter(|n| n.kind == kind)
            .map(|n| (n.id, n.title.clone()))
            .collect();
        let mut clicked = None;
        egui::ScrollArea::vertical().id_salt("list").show(ui, |ui| {
            if items.is_empty() {
                ui.label("Aucun élément.");
            }
            for (id, title) in &items {
                if ui.selectable_label(self.selected == Some(*id), title).clicked() {
                    clicked = Some(*id);
                }
            }
        });
        if let Some(id) = clicked {
            self.select(id);
        }
    }

    fn detail_column(&mut self, ui: &mut egui::Ui) {
        let Some(mut edit) = self.edit.take() else {
            ui.label("Sélectionne ou crée un élément.");
            return;
        };
        let id = edit.id;
        let kind = self.nodes.iter().find(|n| n.id == id).map(|n| n.kind);
        let (mut save, mut delete, mut add_scene, mut add_link) = (false, false, false, false);
        let (mut open, mut del_link) = (None, None);
        let (mut add_know, mut del_know) = (false, None);
        let defs: &[fields::FieldDef] = match kind {
            Some(k) => fields::defs(k),
            None => &[],
        };
        let know: Vec<KnowledgeRow> = if kind == Some(NodeKind::Character) {
            knowledge::for_character(&self.conn, id).unwrap_or_default()
        } else {
            Vec::new()
        };

        {
            let kform = &mut self.know_form;
            let nodes = &self.nodes;
            let all_links = &self.links;
            let form = &mut self.link_form;
            egui::ScrollArea::vertical().id_salt("detail").show(ui, |ui| {
                ui.add(egui::TextEdit::singleline(&mut edit.title).desired_width(f32::INFINITY));
                ui.label("Résumé");
                ui.add(
                    egui::TextEdit::multiline(&mut edit.summary)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.label("Notes");
                ui.add(
                    egui::TextEdit::multiline(&mut edit.notes)
                        .desired_rows(6)
                        .desired_width(f32::INFINITY),
                );
                for (i, def) in defs.iter().enumerate() {
                    ui.label(def.label);
                    let value = &mut edit.fields[i];
                    match def.kind {
                        FieldKind::Line => {
                            ui.add(egui::TextEdit::singleline(value).desired_width(f32::INFINITY));
                        }
                        FieldKind::Text => {
                            ui.add(
                                egui::TextEdit::multiline(value)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY),
                            );
                        }
                        FieldKind::Choice(options) => {
                            let shown = options
                                .iter()
                                .find(|(code, _)| *code == value.as_str())
                                .map_or("—", |(_, label)| *label);
                            egui::ComboBox::from_id_salt(def.column)
                                .selected_text(shown)
                                .show_ui(ui, |ui| {
                                    for (code, label) in options {
                                        ui.selectable_value(value, code.to_string(), *label);
                                    }
                                });
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button("Enregistrer").clicked() {
                        save = true;
                    }
                    if ui.button("Supprimer").clicked() {
                        delete = true;
                    }
                    if kind == Some(NodeKind::Episode) && ui.button("+ Scène").clicked() {
                        add_scene = true;
                    }
                });

                ui.separator();
                ui.heading("Connexions");
                for l in all_links.iter().filter(|l| l.src_id == id || l.dst_id == id) {
                    let (arrow, other) = if l.src_id == id {
                        ("→", l.dst_id)
                    } else {
                        ("←", l.src_id)
                    };
                    ui.horizontal(|ui| {
                        let what = l
                            .label
                            .clone()
                            .unwrap_or_else(|| link_type_label(&l.link_type).to_string());
                        ui.label(format!("{arrow} {what} :"));
                        if ui.link(title_in(nodes, other)).clicked() {
                            open = Some(other);
                        }
                        if ui.small_button("✕").clicked() {
                            del_link = Some(l.id);
                        }
                    });
                }

                ui.add_space(6.0);
                ui.label("Ajouter une connexion");
                let target_text = form
                    .target
                    .map_or_else(|| "Choisir un élément…".to_string(), |t| title_in(nodes, t).to_string());
                egui::ComboBox::from_id_salt("link_target")
                    .selected_text(target_text)
                    .show_ui(ui, |ui| {
                        for n in nodes.iter().filter(|n| n.id != id) {
                            ui.selectable_value(
                                &mut form.target,
                                Some(n.id),
                                format!("{} · {}", n.kind.label(), n.title),
                            );
                        }
                    });
                egui::ComboBox::from_id_salt("link_type")
                    .selected_text(LINK_TYPES[form.link_type].1)
                    .show_ui(ui, |ui| {
                        for (i, (_, label)) in LINK_TYPES.iter().enumerate() {
                            ui.selectable_value(&mut form.link_type, i, *label);
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut form.label)
                        .hint_text("précision (frère, rival…)"),
                );
                if ui.button("Relier").clicked() {
                    add_link = true;
                }

                if kind == Some(NodeKind::Character) {
                    ui.separator();
                    ui.heading("Connaissances");
                    ui.label(
                        RichText::new("Ce que ce personnage sait, et à quel événement il l'apprend (sert aux alertes de cohérence).")
                            .color(Color32::from_gray(130)),
                    );
                    for k in &know {
                        ui.horizontal(|ui| {
                            let when = k
                                .learned_event_title
                                .as_ref()
                                .map_or("moment non précisé".to_string(), |t| format!("appris à « {t} »"));
                            ui.label(format!("sait « {} » — {when}", k.subject_title));
                            if ui.small_button("✕").clicked() {
                                del_know = Some(k.id);
                            }
                        });
                    }
                    let subj = kform
                        .subject
                        .map_or_else(|| "Information / élément…".to_string(), |t| title_in(nodes, t).to_string());
                    egui::ComboBox::from_id_salt("know_subject").selected_text(subj).show_ui(ui, |ui| {
                        for n in nodes.iter().filter(|n| n.id != id) {
                            ui.selectable_value(&mut kform.subject, Some(n.id), format!("{} · {}", n.kind.label(), n.title));
                        }
                    });
                    let ev = kform
                        .event
                        .map_or_else(|| "Appris à l'événement… (facultatif)".to_string(), |t| title_in(nodes, t).to_string());
                    egui::ComboBox::from_id_salt("know_event").selected_text(ev).show_ui(ui, |ui| {
                        ui.selectable_value(&mut kform.event, None, "(moment non précisé)");
                        for n in nodes.iter().filter(|n| n.kind == NodeKind::Event) {
                            ui.selectable_value(&mut kform.event, Some(n.id), n.title.as_str());
                        }
                    });
                    if ui.button("Ajouter la connaissance").clicked() {
                        add_know = true;
                    }
                }
            });
        }

        if save {
            self.apply(|c| {
                nodes::update_text(c, id, &edit.title, &edit.summary, &edit.notes)?;
                match kind {
                    Some(k) => fields::save(c, k, id, &edit.fields),
                    None => Ok(()),
                }
            });
        }
        if let Some(k) = del_know {
            self.apply(|c| knowledge::delete(c, k));
        }
        if add_know {
            if let Some(subject) = self.know_form.subject {
                let event = self.know_form.event;
                if self.apply(|c| knowledge::add(c, id, subject, event)).is_some() {
                    self.know_form = KnowForm::default();
                }
            }
        }
        if let Some(l) = del_link {
            self.apply(|c| links::delete(c, l));
        }
        if add_link {
            if let Some(target) = self.link_form.target {
                let ty = LINK_TYPES[self.link_form.link_type].0;
                let label = self.link_form.label.trim().to_string();
                let ok = self.apply(|c| {
                    links::create(c, id, target, ty, (!label.is_empty()).then_some(label.as_str()), None)
                });
                if ok.is_some() {
                    self.link_form = LinkForm::default();
                }
            }
        }
        if add_scene {
            self.apply(|c| {
                let pos = nodes::next_scene_position(c, id)?;
                let scene = nodes::create_scene(c, id, &format!("Scène {pos}"), pos)?;
                links::create(c, id, scene, "contains", None, None)?;
                Ok(scene)
            });
        }
        if delete && self.apply(|c| nodes::delete(c, id)).is_some() {
            self.selected = None;
            return;
        }
        self.edit = Some(edit);
        if let Some(o) = open {
            self.go_to(o);
        }
    }
}

impl eframe::App for StudioApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::SidePanel::left("nav")
            .exact_width(210.0)
            .show(ctx, |ui| self.sidebar(ui));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.label(self.status.as_str());
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if !self.search.trim().is_empty() {
                self.search_view(ui);
                return;
            }
            match self.view {
                View::Dashboard => self.dashboard(ui),
                View::Kind(kind) => self.kind_view(ui, kind),
                View::Timeline => self.timeline_view(ui),
                View::Graph => self.graph_view(ui),
                View::Alerts => self.alerts_view(ui),
            }
        });
    }
}
