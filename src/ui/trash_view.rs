//! Corbeille : restaurer ou supprimer définitivement les éléments supprimés.

use eframe::egui::{self, Color32, RichText};

use super::StudioApp;
use crate::db::{nodes::NodeKind, trash};

impl StudioApp {
    pub(super) fn trash_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Corbeille");
        ui.label(
            RichText::new("Les éléments supprimés sont conservés ici (100 derniers). Restaurer recrée l'élément, ses scènes, ses étiquettes et ses liens avec ce qui existe encore.")
                .color(super::theme::muted(ui)),
        );
        let rows = trash::list(&self.conn).unwrap_or_default();
        let (mut restore, mut purge, mut empty) = (None, None, false);
        if rows.is_empty() {
            ui.add_space(12.0);
            ui.label("La corbeille est vide.");
            return;
        }
        if ui.button("Vider la corbeille").clicked() {
            empty = true;
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("trash")
            .show(ui, |ui| {
                for r in &rows {
                    ui.horizontal(|ui| {
                        let label = NodeKind::parse(&r.kind).map_or("élément", |k| k.label());
                        ui.label(RichText::new(label).color(Color32::from_gray(150)));
                        ui.label(RichText::new(&r.title).strong());
                        ui.label(
                            RichText::new(
                                r.deleted_at
                                    .replace('T', " ")
                                    .chars()
                                    .take(16)
                                    .collect::<String>(),
                            )
                            .small(),
                        );
                        if ui.button("Restaurer").clicked() {
                            restore = Some(r.id);
                        }
                        if ui.button("Supprimer définitivement").clicked() {
                            purge = Some(r.id);
                        }
                    });
                }
            });
        if let Some(id) = restore {
            if self.apply(|c| trash::restore(c, id)).is_some() {
                self.status = "Élément restauré.".into();
            }
        }
        if let Some(id) = purge {
            self.apply(|c| trash::purge(c, id));
        }
        if empty {
            self.apply(|c| trash::empty(c));
        }
    }
}
