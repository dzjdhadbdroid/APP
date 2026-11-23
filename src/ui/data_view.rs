//! Données : sauvegardes, restauration, export JSON/Markdown, import.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText};

use super::StudioApp;
use crate::db::{self, transfer};

/// Ouvre un dossier dans l'explorateur de fichiers du système (sans effet si impossible).
fn open_folder(path: &Path) {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    let _ = std::process::Command::new(program).arg(path).spawn();
}

impl StudioApp {
    /// Écrit un export dans `<données>/exports/` et indique où il se trouve.
    pub(super) fn write_export(
        &mut self,
        stem: &str,
        extension: &str,
        content: db::DbResult<Vec<u8>>,
    ) {
        let result = content.and_then(|text| {
            let dir = self.data_dir.join("exports");
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{stem}_{}.{extension}", db::unix_secs()));
            std::fs::write(&path, text)?;
            Ok(path)
        });
        self.status = match result {
            Ok(p) => format!("Export créé : {}", p.display()),
            Err(e) => format!("Erreur d'export : {e}"),
        };
    }

    /// Un dossier complet : JSON, bible en Markdown et PDF, et un PDF de scénario par épisode.
    fn export_project(&mut self) {
        let result: db::DbResult<std::path::PathBuf> = (|| -> db::DbResult<std::path::PathBuf> {
            let dir = self
                .data_dir
                .join("exports")
                .join(format!("projet_{}", db::unix_secs()));
            std::fs::create_dir_all(dir.join("scenarios"))?;
            std::fs::write(dir.join("univers.json"), transfer::export_json(&self.conn)?)?;
            let md = transfer::export_markdown(&self.conn)?;
            std::fs::write(
                dir.join("bible.pdf"),
                crate::pdf::markdown_to_pdf("Bible de la sitcom", &md),
            )?;
            std::fs::write(dir.join("bible.md"), md)?;
            for e in db::timeline::list_air(&self.conn)? {
                let items = db::script::episode_items(&self.conn, e.id)?;
                if items.is_empty() {
                    continue;
                }
                let title = format!("S{}E{} - {}", e.season, e.number, e.title);
                let file: String = title
                    .chars()
                    .map(|c| {
                        if c.is_alphanumeric() || c == '-' || c == ' ' {
                            c
                        } else {
                            '_'
                        }
                    })
                    .collect();
                std::fs::write(
                    dir.join("scenarios").join(format!("{file}.pdf")),
                    crate::pdf::script_to_pdf(&title, &items),
                )?;
            }
            Ok(dir)
        })();
        self.status = match result {
            Ok(p) => format!("Projet exporté dans : {}", p.display()),
            Err(e) => format!("Erreur d'export : {e}"),
        };
    }

    pub(super) fn data_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Données et sauvegardes");
        ui.label(
            RichText::new(format!("Dossier des données : {}", self.data_dir.display()))
                .color(Color32::from_gray(140)),
        );
        ui.label(
            RichText::new("Tout est enregistré localement et immédiatement. Une sauvegarde automatique est faite à chaque lancement (10 conservées).")
                .color(Color32::from_gray(140)),
        );
        ui.add_space(8.0);

        ui.horizontal_wrapped(|ui| {
            if ui.button("Sauvegarder maintenant").clicked() {
                self.status = match db::manual_backup(&self.conn, &self.data_dir) {
                    Ok(p) => format!("Sauvegarde créée : {}", p.display()),
                    Err(e) => format!("Erreur de sauvegarde : {e}"),
                };
            }
            if ui.button("Exporter en JSON (complet)").clicked() {
                let content = transfer::export_json(&self.conn).map(String::into_bytes);
                self.write_export("univers", "json", content);
            }
            if ui.button("Exporter en Markdown (lisible)").clicked() {
                let content = transfer::export_markdown(&self.conn).map(String::into_bytes);
                self.write_export("univers", "md", content);
            }
            if ui.button("Exporter en PDF").clicked() {
                let content = transfer::export_markdown(&self.conn)
                    .map(|md| crate::pdf::markdown_to_pdf("Bible de la sitcom", &md));
                self.write_export("univers", "pdf", content);
            }
            if ui.button("Exporter tout le projet (dossier)").clicked() {
                self.export_project();
            }
            if ui.button("Ouvrir le dossier des données").clicked() {
                open_folder(&self.data_dir);
            }
        });

        ui.add_space(10.0);
        ui.heading("Importer un export JSON");
        ui.label("Le contenu du fichier est AJOUTÉ à l'existant (rien n'est modifié ni supprimé). En cas d'erreur, rien n'est importé.");
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.import_path)
                    .hint_text("Chemin du fichier .json")
                    .desired_width(420.0),
            );
            if ui.button("Importer").clicked() && !self.import_path.trim().is_empty() {
                // « Copier comme chemin » de Windows entoure le chemin de guillemets.
                let path = self.import_path.trim().trim_matches('"').to_string();
                match std::fs::read_to_string(&path) {
                    Ok(source) => {
                        let _ = db::backup_with_prefix(&self.conn, &self.data_dir, "avant_import");
                        if let Some(r) = self.apply(|c| transfer::import_json(c, &source)) {
                            self.status = format!(
                                "Import terminé : {} élément(s), {} lien(s), {} connaissance(s).",
                                r.nodes, r.links, r.knowledge
                            );
                        }
                    }
                    Err(e) => self.status = format!("Lecture impossible : {e}"),
                }
            }
        });

        ui.add_space(10.0);
        ui.heading("Restaurer une sauvegarde");
        ui.label(
            "Remplace tout le contenu actuel. Une sauvegarde de sécurité est créée juste avant.",
        );
        let backups: Vec<PathBuf> = db::list_backups(&self.data_dir)
            .into_iter()
            .take(30)
            .collect();
        if backups.is_empty() {
            ui.label("Aucune sauvegarde pour l'instant.");
        }
        let mut ask = None;
        let mut confirmed = None;
        let mut cancel = false;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for p in &backups {
                let name = p
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                ui.horizontal(|ui| {
                    ui.label(&name);
                    if self.confirm_restore.as_ref() == Some(p) {
                        ui.label(
                            RichText::new("Remplacer tout le contenu ?")
                                .color(Color32::from_rgb(255, 180, 80)),
                        );
                        if ui.button("Confirmer").clicked() {
                            confirmed = Some(p.clone());
                        }
                        if ui.button("Annuler").clicked() {
                            cancel = true;
                        }
                    } else if ui.small_button("Restaurer").clicked() {
                        ask = Some(p.clone());
                    }
                });
            }
        });
        if cancel {
            self.confirm_restore = None;
        }
        if let Some(p) = ask {
            self.confirm_restore = Some(p);
        }
        if let Some(p) = confirmed {
            self.confirm_restore = None;
            self.restore_backup(&p);
        }
    }

    fn restore_backup(&mut self, path: &Path) {
        if let Err(e) = db::backup_with_prefix(&self.conn, &self.data_dir, "avant_restauration") {
            self.status = format!("Restauration annulée (sauvegarde de sécurité impossible) : {e}");
            return;
        }
        match db::restore(&mut self.conn, path) {
            Ok(()) => {
                self.selected = None;
                self.edit = None;
                self.graph.focus = None;
                self.reload();
                self.status = "Sauvegarde restaurée.".to_string();
            }
            Err(e) => self.status = format!("Restauration impossible : {e}"),
        }
    }
}
