//! Onglet Synopsis et panneau des PDF déjà rédigés (synopsis ou scénario) d'un épisode ou d'une scène.

use eframe::egui::{self, Color32, RichText};

use super::{theme, title_in, StudioApp};
use crate::db::{attachments, fields, nodes::NodeKind};

fn open_with_system(path: &std::path::Path) -> std::io::Result<()> {
    use std::process::Command;
    if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()
            .map(|_| ())
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(path).spawn().map(|_| ())
    } else {
        Command::new("xdg-open").arg(path).spawn().map(|_| ())
    }
}

/// Champ texte qui tient lieu de synopsis : le synopsis d'un épisode, les notes d'une scène.
fn synopsis_column(kind: NodeKind) -> Option<&'static str> {
    match kind {
        NodeKind::Episode => Some("synopsis"),
        NodeKind::Scene => Some("content"),
        _ => None,
    }
}

impl StudioApp {
    /// Onglet Synopsis : texte du synopsis + PDF déjà rédigés.
    pub(super) fn synopsis_tab(&mut self, ui: &mut egui::Ui, id: i64, kind: NodeKind) {
        ui.label(RichText::new(title_in(&self.nodes, id)).strong());
        if let Some(col) = synopsis_column(kind) {
            let defs = fields::defs(kind);
            let idx = defs.iter().position(|d| d.column == col);
            if let (Some(idx), true) = (idx, self.synopsis_buf.0 != id) {
                let text = fields::load(&self.conn, kind, id)
                    .unwrap_or_default()
                    .get(idx)
                    .cloned()
                    .unwrap_or_default();
                self.synopsis_buf = (id, text);
            }
            ui.label(if kind == NodeKind::Episode {
                "Synopsis"
            } else {
                "Notes de scène"
            });
            ui.add(
                egui::TextEdit::multiline(&mut self.synopsis_buf.1)
                    .desired_rows(8)
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Enregistrer le texte").clicked() {
                if let Some(idx) = idx {
                    let text = self.synopsis_buf.1.clone();
                    self.apply(|c| {
                        let mut values = fields::load(c, kind, id)?;
                        values[idx] = text;
                        fields::save(c, kind, id, &values)
                    });
                }
            }
            ui.separator();
        }
        self.attachments_panel(ui, id, "synopsis", "PDF de synopsis");
    }

    /// PDF de scénario déjà rédigés : section repliée tant qu'on n'en a pas besoin.
    pub(super) fn pdf_section(&mut self, ui: &mut egui::Ui, node: i64) {
        let n = attachments::list(&self.conn, node, "script").map_or(0, |v| v.len());
        egui::CollapsingHeader::new(format!("PDF de scénario déjà rédigé ({n})"))
            .id_salt(("pdf_section", node))
            .default_open(n > 0)
            .show(ui, |ui| self.attachments_panel(ui, node, "script", ""));
    }

    /// Liste des PDF d'un créneau, avec ajout, ouverture, enregistrement et suppression.
    pub(super) fn attachments_panel(
        &mut self,
        ui: &mut egui::Ui,
        node: i64,
        slot: &str,
        title: &str,
    ) {
        let files = attachments::list(&self.conn, node, slot).unwrap_or_default();
        let (mut add, mut open, mut save_as, mut delete) = (false, None, None, None);
        ui.horizontal(|ui| {
            if !title.is_empty() {
                ui.label(RichText::new(title).strong());
            }
            if ui.button("+ Ajouter un PDF…").clicked() {
                add = true;
            }
        });
        if files.is_empty() {
            ui.label(
                RichText::new("Aucun PDF. Ajoute un document déjà rédigé : il est conservé dans la base et sauvegardé avec elle.")
                    .small()
                    .color(theme::muted(ui)),
            );
        }
        for f in &files {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("📄 {}", f.filename)).strong());
                ui.label(
                    RichText::new(format!(
                        "{} Ko · {}",
                        (f.size + 1023) / 1024,
                        f.added_at.chars().take(10).collect::<String>()
                    ))
                    .small()
                    .color(theme::muted(ui)),
                );
                if ui.button("Ouvrir").clicked() {
                    open = Some(f.id);
                }
                if ui.button("Enregistrer sous…").clicked() {
                    save_as = Some(f.id);
                }
                if self.attach_confirm == Some(f.id) {
                    if ui
                        .button(
                            RichText::new("Confirmer ?").color(Color32::from_rgb(255, 110, 110)),
                        )
                        .clicked()
                    {
                        delete = Some(f.id);
                    }
                } else if ui.button("Retirer").clicked() {
                    self.attach_confirm = Some(f.id);
                }
            });
        }

        if add {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("PDF", &["pdf"])
                .pick_file()
            {
                match std::fs::read(&path) {
                    Ok(bytes) => {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if self
                            .apply(|c| attachments::add(c, node, slot, &name, &bytes))
                            .is_some()
                        {
                            self.status = format!("PDF ajouté : {name}");
                        }
                    }
                    Err(e) => self.status = format!("Lecture du fichier impossible : {e}"),
                }
            }
        }
        if let Some(id) = open {
            let result = attachments::data(&self.conn, id)
                .map_err(|e| e.to_string())
                .and_then(|(name, bytes)| {
                    let dir = self.data_dir.join("pdf_ouverts");
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let path = dir.join(format!("{id}_{name}"));
                    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
                    open_with_system(&path).map_err(|e| e.to_string())
                });
            if let Err(e) = result {
                self.status = format!("Ouverture impossible : {e}");
            }
        }
        if let Some(id) = save_as {
            if let Ok((name, bytes)) = attachments::data(&self.conn, id) {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(name.as_str())
                    .add_filter("PDF", &["pdf"])
                    .save_file()
                {
                    self.status = match std::fs::write(&path, bytes) {
                        Ok(()) => format!("Enregistré : {}", path.display()),
                        Err(e) => format!("Écriture impossible : {e}"),
                    };
                }
            }
        }
        if let Some(id) = delete {
            self.attach_confirm = None;
            self.apply(|c| attachments::delete(c, id));
        }
    }
}
