//! Palette de commandes (Ctrl+P) : aller n'importe où ou lancer une action en tapant quelques lettres.

use eframe::egui::{self, Color32, RichText};

use super::{kind_color, StudioApp, View};
use crate::db::{self, nodes::NodeKind, pdf_analysis::norm};

#[derive(Default)]
pub struct PaletteState {
    open: bool,
    query: String,
    selected: usize,
}

#[derive(Clone, Copy)]
enum Action {
    Go(View),
    Open(i64),
    Undo,
    Redo,
    Backup,
    Read,
}

struct Entry {
    label: String,
    hint: String,
    color: Color32,
    action: Action,
}

const COMMANDS: [(&str, View); 15] = [
    ("Tableau de bord", View::Dashboard),
    ("Carte de l'univers", View::Graph),
    ("Analyse de l'écriture", View::Analysis),
    ("Alertes de cohérence", View::Alerts),
    ("Assistant IA", View::Ai),
    ("Personnages", View::Kind(NodeKind::Character)),
    ("Relations", View::Relations),
    ("Lieux", View::Kind(NodeKind::Place)),
    ("Objets", View::Kind(NodeKind::Object)),
    ("Épisodes", View::Kind(NodeKind::Episode)),
    ("Intrigues et gags", View::Kind(NodeKind::Plot)),
    ("Chronologie", View::Timeline),
    ("Affichage et réglages", View::Settings),
    ("Données et sauvegardes", View::Data),
    ("Corbeille", View::Trash),
];

impl StudioApp {
    fn palette_entries(&self) -> Vec<Entry> {
        let words: Vec<String> = norm(&self.palette.query)
            .split(' ')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect();
        let matches = |text: &str| {
            let t = norm(text);
            words.iter().all(|w| t.contains(w.as_str()))
        };
        let muted = Color32::from_gray(140);
        let mut out = Vec::new();
        for (label, v) in COMMANDS {
            if matches(label) {
                out.push(Entry {
                    label: label.to_string(),
                    hint: "Aller à".into(),
                    color: muted,
                    action: Action::Go(v),
                });
            }
        }
        for (label, action) in [
            ("Annuler la dernière action", Action::Undo),
            ("Rétablir", Action::Redo),
            ("Sauvegarder maintenant", Action::Backup),
            ("Lire l'épisode ouvert en plein écran", Action::Read),
        ] {
            if matches(label) {
                out.push(Entry {
                    label: label.to_string(),
                    hint: "Action".into(),
                    color: muted,
                    action,
                });
            }
        }
        if !words.is_empty() {
            for n in self.nodes.iter().filter(|n| matches(&n.title)).take(40) {
                out.push(Entry {
                    label: n.title.clone(),
                    hint: n.kind.label().to_string(),
                    color: kind_color(n.kind),
                    action: Action::Open(n.id),
                });
            }
        }
        out.truncate(14);
        out
    }

    /// Épisode actuellement ouvert (ou celui de la scène ouverte).
    pub(super) fn current_episode(&self) -> Option<i64> {
        let n = self
            .selected
            .and_then(|id| self.nodes.iter().find(|n| n.id == id))?;
        match n.kind {
            NodeKind::Episode => Some(n.id),
            NodeKind::Scene => db::nodes::episode_of_scene(&self.conn, n.id),
            _ => None,
        }
    }

    pub(super) fn palette_ui(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::P)) {
            self.palette.open = !self.palette.open;
            self.palette.query.clear();
            self.palette.selected = 0;
        }
        if !self.palette.open {
            return;
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.palette.open = false;
            return;
        }
        let entries = self.palette_entries();
        let last = entries.len().saturating_sub(1);
        let (down, up, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            )
        });
        if down {
            self.palette.selected = (self.palette.selected + 1).min(last);
        }
        if up {
            self.palette.selected = self.palette.selected.saturating_sub(1);
        }
        self.palette.selected = self.palette.selected.min(last);

        let mut chosen: Option<Action> = enter
            .then(|| entries.get(self.palette.selected).map(|e| e.action))
            .flatten();
        egui::Window::new("palette")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 70.0])
            .fixed_size([520.0, 0.0])
            .show(ctx, |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.palette.query)
                        .hint_text("Aller à… (personnage, épisode, lieu, réglage, action)")
                        .desired_width(f32::INFINITY),
                );
                r.request_focus();
                if r.changed() {
                    self.palette.selected = 0;
                }
                ui.add_space(4.0);
                if entries.is_empty() {
                    ui.label(RichText::new("Aucun résultat.").color(Color32::from_gray(140)));
                }
                for (i, e) in entries.iter().enumerate() {
                    let sel = i == self.palette.selected;
                    let resp = ui.horizontal(|ui| {
                        let l = ui.selectable_label(sel, &e.label);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(&e.hint).small().color(e.color));
                        });
                        l
                    });
                    if resp.inner.clicked() {
                        chosen = Some(e.action);
                    }
                }
                ui.add_space(2.0);
                ui.label(
                    RichText::new("↑↓ choisir · Entrée valider · Échap fermer")
                        .small()
                        .color(Color32::from_gray(120)),
                );
            });
        let Some(action) = chosen else {
            return;
        };
        self.palette.open = false;
        match action {
            Action::Go(v) => {
                self.season_sheet = None;
                self.switch_view(v);
            }
            Action::Open(id) => self.go_to(id),
            Action::Undo => self.undo_redo(false),
            Action::Redo => self.undo_redo(true),
            Action::Backup => {
                self.status = match db::manual_backup(&self.conn, &self.data_dir) {
                    Ok(p) => format!("Sauvegarde créée : {}", p.display()),
                    Err(e) => format!("Erreur de sauvegarde : {e}"),
                };
            }
            Action::Read => match self.current_episode() {
                Some(ep) => self.start_reading(ctx, ep),
                None => self.status = "Ouvre d'abord un épisode (ou une de ses scènes).".into(),
            },
        }
    }
}
