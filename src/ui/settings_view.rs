//! Paramètres d'affichage (thème, taille) et dernières fiches consultées, mémorisés en base.

use eframe::egui::{self, Visuals};

use super::StudioApp;
use crate::db::settings;

pub struct Prefs {
    pub dark: bool,
    pub zoom: f32,
    pub applied: bool,
    pub recent: Vec<i64>,
}

impl Prefs {
    pub fn load(conn: &rusqlite::Connection) -> Self {
        let dark = settings::get(conn, "ui_theme").ok().flatten().is_none_or(|v| v != "light");
        let zoom = settings::get(conn, "ui_zoom")
            .ok()
            .flatten()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(1.0)
            .clamp(0.7, 2.0);
        let recent = settings::get(conn, "recent_viewed")
            .ok()
            .flatten()
            .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
            .unwrap_or_default();
        Self { dark, zoom, applied: false, recent }
    }

    pub fn apply(&mut self, ctx: &egui::Context) {
        if !self.applied {
            ctx.set_visuals(if self.dark { Visuals::dark() } else { Visuals::light() });
            ctx.set_zoom_factor(self.zoom);
            self.applied = true;
        }
    }
}

impl StudioApp {
    /// Mémorise une fiche ouverte (8 dernières, la plus récente en tête).
    pub(super) fn remember_viewed(&mut self, id: i64) {
        self.prefs.recent.retain(|x| *x != id);
        self.prefs.recent.insert(0, id);
        self.prefs.recent.truncate(8);
        let csv: Vec<String> = self.prefs.recent.iter().map(i64::to_string).collect();
        let _ = settings::set(&self.conn, "recent_viewed", &csv.join(","));
    }

    pub(super) fn settings_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Paramètres");
        ui.add_space(8.0);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Thème :");
            changed |= ui.selectable_value(&mut self.prefs.dark, true, "Sombre").changed();
            changed |= ui.selectable_value(&mut self.prefs.dark, false, "Clair").changed();
        });
        ui.horizontal(|ui| {
            ui.label("Taille de l'interface :");
            changed |= ui.add(egui::Slider::new(&mut self.prefs.zoom, 0.7..=2.0).step_by(0.1)).changed();
        });
        if changed {
            self.prefs.applied = false; // réappliqué à l'image suivante
            let _ = settings::set(&self.conn, "ui_theme", if self.prefs.dark { "dark" } else { "light" });
            let _ = settings::set(&self.conn, "ui_zoom", &format!("{:.1}", self.prefs.zoom));
        }
        ui.add_space(8.0);
        ui.label(format!("Dossier des données : {}", self.data_dir.display()));
    }
}
