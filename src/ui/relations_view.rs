//! Module Relations : liens entre personnages (catégorie, proximité, description,
//! évolution dans le temps, informations connues de chacun).

use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText};

use super::{title_in, StudioApp};
use crate::db::{nodes::NodeKind, relations};

struct RelBuf {
    label: String,
    closeness: i64,
    description: String,
    a_knows: String,
    b_knows: String,
}

#[derive(Default)]
pub struct RelState {
    a: Option<i64>,
    b: Option<i64>,
    label: String,
    filter: Option<i64>,
    bufs: HashMap<i64, RelBuf>,
    notes: HashMap<i64, String>,
}

impl StudioApp {
    pub(super) fn relations_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Relations entre personnages");
        let characters: Vec<(i64, String)> = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Character)
            .map(|n| (n.id, n.title.clone()))
            .collect();
        let all = relations::list(&self.conn).unwrap_or_default();
        let name = |id: i64| title_in(&self.nodes, id).to_string();

        let mut create = false;
        {
            let st = &mut self.rel;
            ui.horizontal_wrapped(|ui| {
                for (slot, id_salt) in [(0, "rel_a"), (1, "rel_b")] {
                    let cur = if slot == 0 { st.a } else { st.b };
                    let shown = cur.map_or("Personnage…".to_string(), |c| name(c));
                    egui::ComboBox::from_id_salt(id_salt)
                        .selected_text(shown)
                        .show_ui(ui, |ui| {
                            for (id, n) in &characters {
                                let target = if slot == 0 { &mut st.a } else { &mut st.b };
                                ui.selectable_value(target, Some(*id), n);
                            }
                        });
                }
                ui.add(
                    egui::TextEdit::singleline(&mut st.label).hint_text("nature (frère, rival…)"),
                );
                if ui.button("Créer la relation").clicked() {
                    create = true;
                }
            });
            ui.horizontal(|ui| {
                ui.label("Filtrer sur :");
                let shown = st.filter.map_or("(tous)".to_string(), |c| name(c));
                egui::ComboBox::from_id_salt("rel_filter")
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut st.filter, None, "(tous)");
                        for (id, n) in &characters {
                            ui.selectable_value(&mut st.filter, Some(*id), n);
                        }
                    });
            });
        }
        ui.separator();

        let filter = self.rel.filter;
        let mut save: Option<i64> = None;
        let mut add_note: Option<i64> = None;
        let mut delete: Option<i64> = None;
        let mut open: Option<i64> = None;
        let shown: Vec<&relations::Relation> = all
            .iter()
            .filter(|r| filter.map_or(true, |f| r.a == f || r.b == f))
            .collect();
        if shown.is_empty() {
            ui.label("Aucune relation. Choisis deux personnages ci-dessus pour en créer une.");
        }
        egui::ScrollArea::vertical()
            .id_salt("relations")
            .show(ui, |ui| {
                for r in shown {
                    let buf = self.rel.bufs.entry(r.id).or_insert_with(|| RelBuf {
                        label: r.label.clone(),
                        closeness: r.closeness.unwrap_or(5),
                        description: r.description.clone(),
                        a_knows: r.a_knows.clone(),
                        b_knows: r.b_knows.clone(),
                    });
                    let (an, bn) = (name(r.a), name(r.b));
                    egui::CollapsingHeader::new(format!("{an} ↔ {bn} — {}", r.label))
                        .id_salt(("rel", r.id))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.link(&an).clicked() {
                                    open = Some(r.a);
                                }
                                if ui.link(&bn).clicked() {
                                    open = Some(r.b);
                                }
                            });
                            ui.add(egui::TextEdit::singleline(&mut buf.label).hint_text("nature"));
                            ui.add(egui::Slider::new(&mut buf.closeness, 0..=10).text("proximité"));
                            ui.label("Description");
                            ui.add(
                                egui::TextEdit::multiline(&mut buf.description)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.label(format!("Ce que {an} sait de {bn}"));
                            ui.add(
                                egui::TextEdit::multiline(&mut buf.a_knows)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.label(format!("Ce que {bn} sait de {an}"));
                            ui.add(
                                egui::TextEdit::multiline(&mut buf.b_knows)
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.horizontal(|ui| {
                                if ui.button("Enregistrer").clicked() {
                                    save = Some(r.id);
                                }
                                if ui.button("Supprimer la relation").clicked() {
                                    delete = Some(r.id);
                                }
                            });
                            ui.separator();
                            ui.label(RichText::new("Évolution de la relation").strong());
                            for h in relations::history(&self.conn, r.id).unwrap_or_default() {
                                let ev = h.event_id.map_or(String::new(), |e| {
                                    format!(" (à « {} »)", title_in(&self.nodes, e))
                                });
                                ui.label(format!("{} — {}{ev}", h.created_at, h.note));
                            }
                            let note = self.rel.notes.entry(r.id).or_default();
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(note)
                                        .hint_text("ex : se brouillent après l'épisode 3"),
                                );
                                if ui.button("Ajouter").clicked() {
                                    add_note = Some(r.id);
                                }
                            });
                        });
                    ui.add_space(2.0);
                }
            });
        ui.label(RichText::new(" ").color(Color32::GRAY));

        if create {
            if let (Some(a), Some(b)) = (self.rel.a, self.rel.b) {
                let label = self.rel.label.trim().to_string();
                if self
                    .apply(|c| relations::create(c, a, b, &label, Some(5)))
                    .is_some()
                {
                    self.rel.label.clear();
                    self.rel.a = None;
                    self.rel.b = None;
                }
            } else {
                self.status = "Choisis deux personnages.".into();
            }
        }
        if let Some(id) = save {
            if let Some(b) = self.rel.bufs.get(&id) {
                let (l, c, d, ak, bk) = (
                    b.label.clone(),
                    b.closeness,
                    b.description.clone(),
                    b.a_knows.clone(),
                    b.b_knows.clone(),
                );
                self.apply(|conn| relations::update(conn, id, &l, Some(c), &d, &ak, &bk));
            }
        }
        if let Some(id) = add_note {
            let text = self.rel.notes.get(&id).cloned().unwrap_or_default();
            if self
                .apply(|c| relations::add_history(c, id, &text, None))
                .is_some()
            {
                self.rel.notes.remove(&id);
            }
        }
        if let Some(id) = delete {
            self.rel.bufs.remove(&id);
            self.apply(|c| crate::db::links::delete(c, id));
        }
        if let Some(o) = open {
            self.go_to(o);
        }
    }
}
