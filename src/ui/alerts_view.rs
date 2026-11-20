//! Alertes de cohérence scénaristique : classées par gravité, ignorables.

use eframe::egui::{self, Color32, RichText};

use super::StudioApp;
use crate::db::alerts;

fn severity_style(severity: &str) -> (&'static str, Color32) {
    match severity {
        "error" => ("ERREUR", Color32::from_rgb(255, 90, 90)),
        "warning" => ("ATTENTION", Color32::from_rgb(255, 180, 80)),
        _ => ("INFO", Color32::from_rgb(120, 190, 255)),
    }
}

impl StudioApp {
    pub(super) fn alerts_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Alertes de cohérence");
        ui.label(
            RichText::new(
                "Recalculées à chaque modification, d'après les liens que tu crées dans les fiches :\n\
                 • connaissance préalable : événement « nécessite » X, « implique » un personnage, dont la fiche indique quand il apprend X ;\n\
                 • lieu interdit : scène « se déroule dans » un lieu, « implique » un personnage qui « ne doit pas être à » ce lieu (ou son lieu parent) ;\n\
                 • mention anticipée : un événement daté « mentionne » un événement daté plus tard ;\n\
                 • objet absent : l'objet « apparaît dans » des épisodes d'une intrigue (épisodes qui « font avancer » l'intrigue) avant et après un épisode où il manque ;\n\
                 • relations contradictoires : deux libellés différents entre les mêmes personnages.",
            )
            .color(Color32::from_gray(130)),
        );
        ui.checkbox(&mut self.show_ignored, "Afficher les alertes ignorées");
        ui.separator();

        let rows: Vec<_> = self
            .alerts
            .iter()
            .filter(|a| self.show_ignored || a.status == "open")
            .cloned()
            .collect();
        if rows.is_empty() {
            ui.label("Aucune alerte.");
        }

        let (mut open, mut toggle) = (None, None);
        egui::ScrollArea::vertical().show(ui, |ui| {
            for a in &rows {
                let (label, color) = severity_style(&a.severity);
                let ignored = a.status == "ignored";
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(label).strong().color(color));
                    let text = RichText::new(&a.message);
                    ui.label(if ignored { text.weak().strikethrough() } else { text });
                    if let Some(n) = a.node_id {
                        if ui.small_button("Ouvrir").clicked() {
                            open = Some(n);
                        }
                    }
                    let btn = if ignored { "Réactiver" } else { "Ignorer" };
                    if ui.small_button(btn).clicked() {
                        toggle = Some((a.id, if ignored { "open" } else { "ignored" }));
                    }
                });
            }
        });

        if let Some((id, status)) = toggle {
            self.apply(|c| alerts::set_status(c, id, status));
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }
}
