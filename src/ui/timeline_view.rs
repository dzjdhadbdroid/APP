//! Module 13 — Story Timeline : trois chronologies distinctes sur la même base.

use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText};

use super::{kind_color, StudioApp};
use crate::db::{
    nodes::{self, NodeKind},
    timeline::{self, STATUSES},
};

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Real,
    Air,
    Scenes,
    Season,
    Compare,
}

struct EvBuf {
    date: String,
    importance: i64,
    status: String,
}

#[derive(Default)]
pub struct TimelineState {
    tab: Tab,
    /// Restreint la chronologie à un élément : personnage, intrigue, lieu, saison (épisode)…
    filter: Option<i64>,
    new_title: String,
    from: String,
    to: String,
    season: i64,
    compare: Vec<i64>,
    bufs: HashMap<i64, EvBuf>,
}

impl StudioApp {
    pub(super) fn timeline_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Chronologie narrative");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.timeline.tab, Tab::Real, "Chronologie réelle (univers)");
            ui.selectable_value(&mut self.timeline.tab, Tab::Air, "Ordre de diffusion");
            ui.selectable_value(&mut self.timeline.tab, Tab::Scenes, "Ordre des scènes");
            ui.selectable_value(&mut self.timeline.tab, Tab::Season, "Par saison");
            ui.selectable_value(&mut self.timeline.tab, Tab::Compare, "Comparer des personnages");
        });
        ui.separator();
        match self.timeline.tab {
            Tab::Real => self.real_timeline(ui),
            Tab::Air => self.air_timeline(ui),
            Tab::Scenes => self.scene_timeline(ui),
            Tab::Season => self.season_timeline(ui),
            Tab::Compare => self.compare_timeline(ui),
        }
    }

    fn real_timeline(&mut self, ui: &mut egui::Ui) {
        let filter_choices: Vec<(i64, String)> = self
            .nodes
            .iter()
            .filter(|n| !matches!(n.kind, NodeKind::Event | NodeKind::Scene))
            .map(|n| (n.id, format!("{} · {}", n.kind.label(), n.title)))
            .collect();

        ui.horizontal(|ui| {
            ui.label("Filtrer sur :");
            let current = self
                .timeline
                .filter
                .and_then(|f| filter_choices.iter().find(|(id, _)| *id == f))
                .map_or_else(|| "Tout l'univers".to_string(), |(_, t)| t.clone());
            egui::ComboBox::from_id_salt("tl_filter")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.timeline.filter, None, "Tout l'univers");
                    for (id, text) in &filter_choices {
                        ui.selectable_value(&mut self.timeline.filter, Some(*id), text);
                    }
                });
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut self.timeline.new_title)
                    .hint_text("Nouvel événement"),
            );
            if ui.button("Créer").clicked() && !self.timeline.new_title.trim().is_empty() {
                let title = self.timeline.new_title.trim().to_string();
                if self.apply(|c| nodes::create(c, NodeKind::Event, &title)).is_some() {
                    self.timeline.new_title.clear();
                }
            }
        });
        ui.label(
            RichText::new("Dates : AAAA, AAAA-MM ou AAAA-MM-JJ. Un événement lié à un personnage ou une intrigue (fiche → Connexions) apparaît dans sa chronologie.")
                .color(Color32::from_gray(130)),
        );
        ui.horizontal(|ui| {
            ui.label("Période : de");
            ui.add(egui::TextEdit::singleline(&mut self.timeline.from).hint_text("AAAA-MM").desired_width(80.0));
            ui.label("à");
            ui.add(egui::TextEdit::singleline(&mut self.timeline.to).hint_text("AAAA-MM").desired_width(80.0));
        });
        ui.add_space(6.0);

        let rows = timeline::list_events(&self.conn, self.timeline.filter).unwrap_or_default();
        let rows = match timeline::in_period(rows, &self.timeline.from, &self.timeline.to) {
            Ok(r) => r,
            Err(e) => {
                ui.colored_label(Color32::from_rgb(255, 120, 120), e.to_string());
                Vec::new()
            }
        };
        let mut to_apply: Option<(i64, String, i64, String)> = None;
        let mut open = None;

        egui::ScrollArea::vertical().show(ui, |ui| {
            if rows.is_empty() {
                ui.label("Aucun événement.");
            }
            for (i, row) in rows.iter().enumerate() {
                let simultaneous = row.story_sort.is_some()
                    && (rows
                        .get(i.wrapping_sub(1))
                        .is_some_and(|p| p.story_sort == row.story_sort)
                        || rows.get(i + 1).is_some_and(|n| n.story_sort == row.story_sort));
                let buf = self.timeline.bufs.entry(row.id).or_insert_with(|| EvBuf {
                    date: row.story_start.clone().unwrap_or_default(),
                    importance: row.importance,
                    status: row.status.clone(),
                });
                let changed = buf.date.trim() != row.story_start.as_deref().unwrap_or("")
                    || buf.importance != row.importance
                    || buf.status != row.status;

                ui.horizontal(|ui| {
                    let tint = match row.importance {
                        5 => Color32::from_rgb(255, 90, 90),
                        4 => Color32::from_rgb(255, 160, 80),
                        _ => Color32::from_gray(150),
                    };
                    ui.label(RichText::new("●").color(tint));
                    if ui.link(&row.title).clicked() {
                        open = Some(row.id);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut buf.date)
                            .hint_text("AAAA-MM-JJ")
                            .desired_width(90.0),
                    );
                    ui.add(egui::DragValue::new(&mut buf.importance).range(1..=5).prefix("importance "));
                    egui::ComboBox::from_id_salt(("status", row.id))
                        .selected_text(timeline::status_label(&buf.status))
                        .show_ui(ui, |ui| {
                            for s in STATUSES {
                                ui.selectable_value(&mut buf.status, s.to_string(), timeline::status_label(s));
                            }
                        });
                    if changed && ui.button("Appliquer").clicked() {
                        to_apply = Some((row.id, buf.date.clone(), buf.importance, buf.status.clone()));
                    }
                    if simultaneous {
                        ui.label(RichText::new("⇄ simultané").color(Color32::from_rgb(120, 200, 255)));
                    }
                    if row.story_sort.is_none() {
                        ui.label(RichText::new("non daté").color(Color32::from_gray(120)));
                    }
                });
            }
        });

        if let Some((id, date, importance, status)) = to_apply {
            if self
                .apply(|c| timeline::set_event(c, id, &date, importance, &status))
                .is_some()
            {
                self.timeline.bufs.remove(&id);
            }
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    fn air_timeline(&mut self, ui: &mut egui::Ui) {
        ui.label("Ordre dans lequel les épisodes sont diffusés (indépendant de la chronologie de l'univers).");
        let rows = timeline::list_air(&self.conn).unwrap_or_default();
        let mut swap = None;
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            if rows.is_empty() {
                ui.label("Aucun épisode.");
            }
            for (i, r) in rows.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("S{}E{}", r.season, r.number))
                            .color(kind_color(NodeKind::Episode)),
                    );
                    if ui.link(&r.title).clicked() {
                        open = Some(r.id);
                    }
                    if i > 0 && ui.small_button("▲").clicked() {
                        swap = Some((r.id, rows[i - 1].id));
                    }
                    if i + 1 < rows.len() && ui.small_button("▼").clicked() {
                        swap = Some((r.id, rows[i + 1].id));
                    }
                });
            }
        });
        if let Some((a, b)) = swap {
            self.apply(|c| timeline::swap_air_order(c, a, b));
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    fn scene_timeline(&mut self, ui: &mut egui::Ui) {
        ui.label("Ordre de présentation des scènes, épisode par épisode (en ordre de diffusion).");
        let rows = timeline::list_scenes(&self.conn).unwrap_or_default();
        let mut open = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            if rows.is_empty() {
                ui.label("Aucune scène : ajoute-en depuis la fiche d'un épisode (« + Scène »).");
            }
            let mut last = "";
            for r in &rows {
                if r.episode_title != last {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&r.episode_title).strong().color(kind_color(NodeKind::Episode)));
                    last = &r.episode_title;
                }
                ui.horizontal(|ui| {
                    ui.label(format!("{}.", r.position));
                    if ui.link(&r.title).clicked() {
                        open = Some(r.id);
                    }
                });
            }
        });
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    /// Événements d'une saison : ceux reliés à l'un de ses épisodes.
    fn season_timeline(&mut self, ui: &mut egui::Ui) {
        let seasons = timeline::seasons(&self.conn).unwrap_or_default();
        if seasons.is_empty() {
            ui.label("Aucun épisode : crée-en pour voir une saison.");
            return;
        }
        if !seasons.contains(&self.timeline.season) {
            self.timeline.season = seasons[0];
        }
        ui.horizontal(|ui| {
            for s in &seasons {
                ui.selectable_value(&mut self.timeline.season, *s, format!("Saison {s}"));
            }
        });
        ui.label(
            RichText::new("Événements reliés (fiche → Connexions) à un épisode de la saison, dans l'ordre de l'univers.")
                .color(Color32::from_gray(130)),
        );
        let rows = timeline::events_of_season(&self.conn, self.timeline.season).unwrap_or_default();
        let mut open = None;
        egui::ScrollArea::vertical().id_salt("season_events").show(ui, |ui| {
            if rows.is_empty() {
                ui.label("Aucun événement relié à cette saison.");
            }
            for r in &rows {
                ui.horizontal(|ui| {
                    ui.label(r.story_start.clone().unwrap_or_else(|| "non daté".into()));
                    if ui.link(&r.title).clicked() {
                        open = Some(r.id);
                    }
                });
            }
        });
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    /// Une colonne par personnage choisi ; une ligne par événement. Les événements partageant
    /// une même date (simultanés) sont repérés dans la colonne « date ».
    fn compare_timeline(&mut self, ui: &mut egui::Ui) {
        let characters: Vec<(i64, String)> = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Character)
            .map(|n| (n.id, n.title.clone()))
            .collect();
        ui.horizontal_wrapped(|ui| {
            ui.label("Personnages :");
            for (id, name) in &characters {
                let mut on = self.timeline.compare.contains(id);
                if ui.checkbox(&mut on, name).changed() {
                    if on {
                        self.timeline.compare.push(*id);
                    } else {
                        self.timeline.compare.retain(|c| c != id);
                    }
                }
            }
        });
        self.timeline.compare.retain(|c| characters.iter().any(|(id, _)| id == c));
        if self.timeline.compare.len() < 2 {
            ui.label("Coche au moins deux personnages pour comparer leurs chronologies.");
            return;
        }
        let chosen = self.timeline.compare.clone();
        let per_char: Vec<Vec<i64>> = chosen
            .iter()
            .map(|c| {
                timeline::list_events(&self.conn, Some(*c))
                    .unwrap_or_default()
                    .into_iter()
                    .map(|e| e.id)
                    .collect()
            })
            .collect();
        let mut events = timeline::list_events(&self.conn, None).unwrap_or_default();
        events.retain(|e| per_char.iter().any(|l| l.contains(&e.id)));
        let mut open = None;
        egui::ScrollArea::both().id_salt("compare_events").show(ui, |ui| {
            egui::Grid::new("compare_grid").striped(true).show(ui, |ui| {
                ui.label(RichText::new("Date").strong());
                ui.label(RichText::new("Événement").strong());
                for c in &chosen {
                    ui.label(RichText::new(super::title_in(&self.nodes, *c)).strong());
                }
                ui.end_row();
                for (i, e) in events.iter().enumerate() {
                    let same = e.story_sort.is_some()
                        && (events.get(i.wrapping_sub(1)).is_some_and(|p| p.story_sort == e.story_sort)
                            || events.get(i + 1).is_some_and(|n| n.story_sort == e.story_sort));
                    let date = e.story_start.clone().unwrap_or_else(|| "non daté".into());
                    ui.label(if same { format!("{date} ⇄") } else { date });
                    if ui.link(&e.title).clicked() {
                        open = Some(e.id);
                    }
                    for list in &per_char {
                        ui.label(if list.contains(&e.id) { "●" } else { "" });
                    }
                    ui.end_row();
                }
            });
        });
        if let Some(id) = open {
            self.go_to(id);
        }
    }
}
