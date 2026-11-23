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
        View::Dashboard | View::Graph | View::Alerts | View::Ai => Section::Dashboard,
        View::Relations => Section::Characters,
        View::Timeline => Section::Timeline,
        View::Settings | View::Data => Section::Settings,
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
                                .hint_text("Recherche globale…")
                                .desired_width(220.0),
                        );
                        if r.changed() {
                            self.refresh_search();
                        }
                    });
                });
                ui.add_space(2.0);
            });
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
        let (mut open_ep, mut new_ep, mut new_season, mut plots) = (None, None, false, false);
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
                            if ui.small_button("Supprimer la saison").clicked() {
                                del_season = Some(*season);
                            }
                        });
                    });
                }
                ui.add_space(6.0);
                if ui.button("+ Nouvelle saison").clicked() {
                    new_season = true;
                }
                ui.separator();
                let active = self.view == View::Kind(NodeKind::Plot);
                if ui
                    .selectable_label(active, RichText::new("Intrigues et arcs").size(15.0))
                    .clicked()
                {
                    plots = true;
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
            self.view = View::Kind(NodeKind::Episode);
            self.search.clear();
            self.refresh_search();
            self.select(id);
            self.remember_viewed(id);
        }
        if let Some(s) = del_season {
            self.confirm_delete = Some(Confirm::Season(s));
        }
        if plots {
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
            ],
            _ => return,
        };
        let mut go = None;
        ui.horizontal(|ui| {
            for (label, v) in &tabs {
                if ui
                    .selectable_label(self.view == *v, RichText::new(*label).size(14.0))
                    .clicked()
                {
                    go = Some(*v);
                }
            }
        });
        ui.separator();
        if let Some(v) = go {
            self.switch_view(v);
        }
    }

    /// Espace de travail Écriture : la fiche de l'épisode (ou de la scène) en pleine largeur.
    pub(super) fn writing_workspace(&mut self, ui: &mut egui::Ui) {
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
}
