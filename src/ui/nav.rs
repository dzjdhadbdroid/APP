//! Navigation : barre principale horizontale (6 sections), navigation secondaire à gauche
//! (Personnages, Écriture) ou onglets de page (Tableau de bord, Lieux, Chronologie, Paramètres).

use std::collections::BTreeMap;

use eframe::egui::{self, Color32, RichText};

use super::{theme, Confirm, StudioApp, View};
use crate::db::{
    nodes::{self, NodeKind},
    timeline,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Dashboard,
    Characters,
    Places,
    Writing,
    Timeline,
    Settings,
}

pub const SECTIONS: [(Section, &str); 6] = [
    (Section::Dashboard, "Tableau de bord"),
    (Section::Characters, "Personnages"),
    (Section::Places, "Lieux"),
    (Section::Writing, "Écriture"),
    (Section::Timeline, "Chronologie"),
    (Section::Settings, "Paramètres"),
];

pub fn section_of(view: View) -> Section {
    match view {
        View::Dashboard | View::Graph | View::Alerts | View::Analysis | View::Ai => {
            Section::Dashboard
        }
        View::Relations => Section::Characters,
        View::Timeline => Section::Timeline,
        View::Settings | View::Data | View::Trash => Section::Settings,
        View::Kind(k) => match k {
            NodeKind::Character => Section::Characters,
            NodeKind::Place | NodeKind::Object => Section::Places,
            NodeKind::Episode | NodeKind::Scene | NodeKind::Plot => Section::Writing,
            NodeKind::Event => Section::Timeline,
        },
    }
}

pub fn section_index(s: Section) -> usize {
    SECTIONS.iter().position(|(x, _)| *x == s).unwrap_or(0)
}

/// Vue affichée en premier quand on ouvre une section.
pub fn default_views() -> [View; 6] {
    [
        View::Dashboard,
        View::Kind(NodeKind::Character),
        View::Kind(NodeKind::Place),
        View::Kind(NodeKind::Episode),
        View::Timeline,
        View::Settings,
    ]
}

impl StudioApp {
    /// Change de page en repartant d'une sélection vide.
    pub(super) fn switch_view(&mut self, v: View) {
        if self.view != v {
            self.view = v;
            self.selected = None;
            self.edit = None;
            self.search.clear();
            self.refresh_search();
        }
    }

    pub(super) fn has_sub_nav(&self) -> bool {
        matches!(
            section_of(self.view),
            Section::Characters | Section::Writing
        )
    }

    pub(super) fn top_bar(&mut self, ctx: &egui::Context) {
        let current = section_of(self.view);
        let mut go: Option<Section> = None;
        let mut undo: Option<bool> = None;
        egui::TopBottomPanel::top("top_nav")
            .frame(theme::bar_frame(ctx, egui::Margin::symmetric(16, 8)))
            .show(ctx, |ui| {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Sitcom Studio")
                            .strong()
                            .size(18.0)
                            .color(theme::ACCENT),
                    );
                    ui.add_space(18.0);
                    for (s, label) in SECTIONS {
                        let text = RichText::new(label).size(15.0);
                        if ui.selectable_label(current == s, text).clicked() {
                            go = Some(s);
                        }
                        ui.add_space(4.0);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut self.search)
                                .id(egui::Id::new("global_search"))
                                .hint_text("Recherche globale… (Ctrl+K)")
                                .desired_width(220.0),
                        );
                        if ui.button("?").on_hover_text("Aide et raccourcis").clicked() {
                            self.show_help = true;
                        }
                        if r.changed() {
                            self.refresh_search();
                        }
                        let (can_undo, can_redo) = (
                            crate::db::undo::can_undo(&self.conn),
                            crate::db::undo::can_redo(&self.conn),
                        );
                        if ui
                            .add_enabled(can_redo, egui::Button::new("↷"))
                            .on_hover_text("Rétablir (Ctrl+Y)")
                            .clicked()
                        {
                            undo = Some(true);
                        }
                        if ui
                            .add_enabled(can_undo, egui::Button::new("↶"))
                            .on_hover_text("Annuler la dernière action (Ctrl+Z)")
                            .clicked()
                        {
                            undo = Some(false);
                        }
                    });
                });
                ui.add_space(2.0);
            });
        if let Some(redo) = undo {
            self.undo_redo(redo);
        }
        if let Some(s) = go {
            let v = self.section_view[section_index(s)];
            self.search.clear();
            self.refresh_search();
            if v == self.view {
                self.selected = None;
                self.edit = None;
            }
            self.switch_view(v);
        }
    }

    pub(super) fn sub_nav(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        match section_of(self.view) {
            Section::Characters => self.characters_nav(ui),
            Section::Writing => self.writing_nav(ui),
            _ => {}
        }
    }

    fn characters_nav(&mut self, ui: &mut egui::Ui) {
        let entries: [(&str, View, Option<bool>); 3] = [
            ("Personnages", View::Kind(NodeKind::Character), Some(false)),
            ("Relations", View::Relations, None),
            (
                "Personnages secondaires",
                View::Kind(NodeKind::Character),
                Some(true),
            ),
        ];
        for (label, view, secondary) in entries {
            let active = self.view == view && secondary.map_or(true, |s| s == self.char_secondary);
            if ui
                .selectable_label(active, RichText::new(label).size(15.0))
                .clicked()
            {
                self.selected = None;
                self.edit = None;
                if let Some(s) = secondary {
                    self.char_secondary = s;
                }
                self.switch_view(view);
            }
            ui.add_space(2.0);
        }
    }

    /// Saisons repliables → épisodes ; un clic sur un épisode l'ouvre dans l'espace de travail.
    fn writing_nav(&mut self, ui: &mut egui::Ui) {
        let air = timeline::list_air(&self.conn).unwrap_or_default();
        let mut seasons: BTreeMap<i64, Vec<(i64, i64, String)>> = BTreeMap::new();
        for r in air {
            seasons
                .entry(r.season)
                .or_default()
                .push((r.number, r.id, r.title));
        }
        for eps in seasons.values_mut() {
            eps.sort();
        }
        let selected = self.selected;
        let (mut open_ep, mut new_ep, mut new_season) = (None, None, false);
        let mut plots: Option<bool> = None;
        let mut sheet: Option<i64> = None;
        let mut del_season: Option<i64> = None;
        ui.label(
            RichText::new("SAISONS")
                .small()
                .color(Color32::from_gray(140)),
        );
        egui::ScrollArea::vertical()
            .id_salt("writing_nav")
            .show(ui, |ui| {
                if seasons.is_empty() {
                    ui.label("Aucune saison.");
                }
                for (season, eps) in &seasons {
                    let has_selected = eps.iter().any(|(_, id, _)| Some(*id) == selected);
                    egui::CollapsingHeader::new(
                        RichText::new(format!("Saison {season}")).size(15.0),
                    )
                    .id_salt(("season", *season))
                    .default_open(has_selected)
                    .show(ui, |ui| {
                        for (number, id, title) in eps {
                            if ui
                                .selectable_label(
                                    selected == Some(*id),
                                    format!("E{number} · {title}"),
                                )
                                .clicked()
                            {
                                open_ep = Some(*id);
                            }
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("+ Épisode").clicked() {
                                new_ep = Some(*season);
                            }
                            ui.menu_button("⋯", |ui| {
                                if ui.button("Fiche de la saison").clicked() {
                                    sheet = Some(*season);
                                    ui.close_menu();
                                }
                                if ui.button("Supprimer la saison…").clicked() {
                                    del_season = Some(*season);
                                    ui.close_menu();
                                }
                            });
                        });
                    });
                }
                ui.add_space(6.0);
                if ui.button("+ Nouvelle saison").clicked() {
                    new_season = true;
                }
                ui.separator();
                let on_plots = self.view == View::Kind(NodeKind::Plot);
                if ui
                    .selectable_label(
                        on_plots && !self.gag_only,
                        RichText::new("Intrigues et arcs").size(15.0),
                    )
                    .clicked()
                {
                    plots = Some(false);
                }
                if ui
                    .selectable_label(
                        on_plots && self.gag_only,
                        RichText::new("Gags récurrents").size(15.0),
                    )
                    .clicked()
                {
                    plots = Some(true);
                }
            });

        let last_season = seasons.keys().next_back().copied().unwrap_or(0);
        let create = if let Some(s) = new_ep {
            Some(s)
        } else if new_season {
            Some(last_season + 1)
        } else {
            None
        };
        if let Some(season) = create {
            let made = self.apply(|c| {
                let n = nodes::next_episode_number(c, season)?;
                nodes::create_episode(c, &format!("Épisode {n}"), season, n)
            });
            open_ep = made.or(open_ep);
        }
        if let Some(id) = open_ep {
            self.season_sheet = None;
            self.view = View::Kind(NodeKind::Episode);
            self.search.clear();
            self.refresh_search();
            self.select(id);
            self.remember_viewed(id);
        }
        if let Some(s) = sheet {
            self.selected = None;
            self.edit = None;
            self.season_sheet = Some(s);
            self.view = View::Kind(NodeKind::Episode);
        }
        if let Some(s) = del_season {
            self.confirm_delete = Some(Confirm::Season(s));
        }
        if let Some(gags) = plots {
            self.selected = None;
            self.edit = None;
            self.gag_only = gags;
            self.season_sheet = None;
            self.switch_view(View::Kind(NodeKind::Plot));
        }
    }

    /// Onglets en haut de page pour les sections sans barre latérale.
    pub(super) fn page_tabs(&mut self, ui: &mut egui::Ui) {
        let open_alerts = self.alerts.iter().filter(|a| a.status == "open").count();
        let alerts_label = format!("Alertes ({open_alerts})");
        let tabs: Vec<(&str, View)> = match section_of(self.view) {
            Section::Dashboard => vec![
                ("Vue d'ensemble", View::Dashboard),
                ("Carte de l'univers", View::Graph),
                ("Analyse", View::Analysis),
                (alerts_label.as_str(), View::Alerts),
                ("Assistant IA", View::Ai),
            ],
            Section::Places => vec![
                ("Lieux", View::Kind(NodeKind::Place)),
                ("Objets", View::Kind(NodeKind::Object)),
            ],
            Section::Timeline => vec![
                ("Chronologie", View::Timeline),
                ("Événements", View::Kind(NodeKind::Event)),
            ],
            Section::Settings => vec![
                ("Affichage", View::Settings),
                ("Données et sauvegardes", View::Data),
                ("Corbeille", View::Trash),
            ],
            _ => return,
        };
        let mut go = None;
        ui.horizontal(|ui| {
            for (label, v) in &tabs {
                if theme::tab(ui, self.view == *v, label) {
                    go = Some(*v);
                }
            }
        });
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(6.0);
        if let Some(v) = go {
            self.switch_view(v);
        }
    }

    /// Espace de travail Écriture : la fiche de l'épisode (ou de la scène) en pleine largeur.
    pub(super) fn writing_workspace(&mut self, ui: &mut egui::Ui) {
        if let Some(season) = self.season_sheet {
            self.season_sheet_view(ui, season);
            return;
        }
        let ok = self
            .selected
            .and_then(|id| self.nodes.iter().find(|n| n.id == id))
            .is_some_and(|n| matches!(n.kind, NodeKind::Episode | NodeKind::Scene));
        if !ok {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.heading("Écriture");
                ui.label("Déplie une saison à gauche et choisis un épisode pour l'ouvrir.");
            });
            return;
        }
        if self.focus {
            if ui.button("← Quitter le mode concentré (Échap)").clicked() {
                self.focus = false;
            }
            ui.separator();
        }
        self.detail_column(ui);
    }

    /// Fiche d'une saison : arc narratif, épisodes avec leur avancement, intrigues et événements.
    fn season_sheet_view(&mut self, ui: &mut egui::Ui, season: i64) {
        use crate::db::{fields, plots, settings};
        let key = format!("season_arc_{season}");
        if self.season_arc.0 != season {
            self.season_arc = (
                season,
                settings::get(&self.conn, &key)
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            );
        }
        let episodes: Vec<_> = timeline::list_air(&self.conn)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.season == season)
            .collect();
        let season_plots = plots::of_season(&self.conn, season).unwrap_or_default();
        let events = timeline::events_of_season(&self.conn, season).map_or(0, |v| v.len());
        let (mut open, mut save) = (None, false);
        ui.heading(format!("Saison {season}"));
        ui.label(
            RichText::new(format!(
                "{} épisode(s) · {} intrigue(s) · {} événement(s) relié(s)",
                episodes.len(),
                season_plots.len(),
                events
            ))
            .color(theme::muted(ui)),
        );
        egui::ScrollArea::vertical().id_salt("season_sheet").show(ui, |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("Arc de la saison").strong());
            ui.add(
                egui::TextEdit::multiline(&mut self.season_arc.1)
                    .desired_rows(5)
                    .desired_width(f32::INFINITY)
                    .hint_text("Fil rouge, enjeux, évolution des personnages sur la saison…"),
            );
            if ui.button("Enregistrer l'arc").clicked() {
                save = true;
            }
            ui.add_space(10.0);
            ui.label(RichText::new("Épisodes").strong());
            egui::Grid::new("season_eps").striped(true).show(ui, |ui| {
                for e in &episodes {
                    let status = self
                        .conn
                        .query_row("SELECT status FROM episodes WHERE id = ?1", [e.id], |r| r.get::<_, String>(0))
                        .map(|c| fields::choice_label(NodeKind::Episode, "status", &c))
                        .unwrap_or_default();
                    ui.label(format!("E{}", e.number));
                    if ui.link(&e.title).clicked() {
                        open = Some(e.id);
                    }
                    ui.label(RichText::new(status).color(theme::muted(ui)));
                    ui.end_row();
                }
            });
            ui.add_space(10.0);
            ui.label(RichText::new("Intrigues présentes dans la saison").strong());
            if season_plots.is_empty() {
                ui.label("Aucune : relie des intrigues aux épisodes ou aux scènes (fiche → Connexions).");
            }
            for (id, title) in &season_plots {
                if ui.link(title).clicked() {
                    open = Some(*id);
                }
            }
        });
        if save {
            let text = self.season_arc.1.clone();
            if settings::set(&self.conn, &key, &text).is_ok() {
                self.status = "Arc de la saison enregistré.".into();
            }
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    /// Fenêtre d'aide : où trouver quoi, et les raccourcis.
    pub(super) fn help_window(&mut self, ctx: &egui::Context) {
        if !self.show_help {
            return;
        }
        let mut open = true;
        egui::Window::new("Aide")
            .open(&mut open)
            .collapsible(false)
            .default_width(460.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                let row = |ui: &mut egui::Ui, what: &str, text: &str| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(what).strong().color(theme::ACCENT));
                        ui.label(text);
                    });
                };
                theme::section_title(ui, "Où trouver quoi");
                row(ui, "Tableau de bord", "vue d'ensemble, carte de l'univers, alertes d'incohérence, assistant IA.");
                row(ui, "Personnages", "fiches, relations entre personnages, personnages secondaires.");
                row(ui, "Lieux", "lieux et sous-lieux, objets.");
                row(ui, "Écriture", "saisons → épisodes → scènes ; synopsis, scénario, PDF ; intrigues et gags.");
                row(ui, "Chronologie", "ordre réel, diffusion, scènes, par saison, comparaison de personnages.");
                row(ui, "Paramètres", "thème, sauvegardes, import/export, corbeille.");
                theme::section_title(ui, "Relier les éléments");
                ui.label("Dans une fiche, section « Connexions » : relie un élément à un autre. Ces liens alimentent la carte, les chronologies et les alertes d'incohérence.");
                theme::section_title(ui, "Carte de l'univers");
                ui.label("Molette pour zoomer, glisser le fond pour se déplacer, glisser un élément pour le repositionner. Clic = fiche à droite, double-clic = explorer ses connexions.");
                theme::section_title(ui, "Raccourcis");
                row(ui, "Ctrl+S", "enregistrer la fiche ouverte.");
                row(ui, "Ctrl+K", "aller à la recherche globale.");
                row(ui, "Ctrl+P", "palette de commandes : aller à une fiche, une page ou lancer une action.");
                row(ui, "Ctrl+Z / Ctrl+Y", "annuler / rétablir la dernière action (hors saisie de texte).");
                row(ui, "Échap", "quitter le mode concentré.");
            });
        self.show_help = open;
    }
}
