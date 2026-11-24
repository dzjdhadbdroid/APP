//! Visionneuse PDF intégrée : les pages sont rendues en arrière-plan puis affichées à la suite.

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use eframe::egui::{self, ColorImage, RichText, TextureHandle, TextureOptions};

use super::{theme, StudioApp};
use crate::db::attachments::{self, Attachment};
use crate::pdf_render::{self, PageImage};

const RENDER_WIDTH: i32 = 900;
const MAX_PAGES: usize = 60;

type Rendered = Result<(Vec<PageImage>, usize), String>;

enum PdfState {
    Loading(Receiver<Rendered>),
    Ready {
        pages: Vec<TextureHandle>,
        total: usize,
    },
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Default)]
enum Mode {
    #[default]
    Text,
    Pages,
}

pub struct PdfView {
    docs: HashMap<i64, PdfState>,
    texts: HashMap<i64, String>,
    mode: Mode,
    pub selected: Option<i64>,
    width: f32,
}

impl Default for PdfView {
    fn default() -> Self {
        Self {
            docs: HashMap::new(),
            texts: HashMap::new(),
            mode: Mode::Text,
            selected: None,
            width: 760.0,
        }
    }
}

impl PdfView {
    pub fn forget(&mut self, attachment: i64) {
        self.docs.remove(&attachment);
        self.texts.remove(&attachment);
        if self.selected == Some(attachment) {
            self.selected = None;
        }
    }
}

impl StudioApp {
    /// Affiche le PDF sélectionné (par défaut le premier) dans une zone défilante de `height` points.
    pub(super) fn pdf_viewer(&mut self, ui: &mut egui::Ui, files: &[Attachment], height: f32) {
        let Some(first) = files.first() else {
            return;
        };
        let id = self
            .pdf
            .selected
            .filter(|s| files.iter().any(|f| f.id == *s))
            .unwrap_or(first.id);
        if files.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for f in files {
                    if theme::tab(ui, f.id == id, &f.filename) {
                        self.pdf.selected = Some(f.id);
                    }
                }
            });
        }

        ui.horizontal(|ui| {
            if theme::tab(ui, self.pdf.mode == Mode::Text, "Texte") {
                self.pdf.mode = Mode::Text;
            }
            if theme::tab(ui, self.pdf.mode == Mode::Pages, "Pages (aperçu fidèle)") {
                self.pdf.mode = Mode::Pages;
            }
        });
        if self.pdf.mode == Mode::Text {
            self.pdf_text(ui, id, height);
            return;
        }

        if !self.pdf.docs.contains_key(&id) {
            let state = match attachments::data(&self.conn, id) {
                Ok((_, bytes)) => {
                    let (tx, rx) = channel();
                    let dir = self.data_dir.clone();
                    let ctx = ui.ctx().clone();
                    std::thread::spawn(move || {
                        let result =
                            pdf_render::render_pages(&dir, &bytes, RENDER_WIDTH, MAX_PAGES);
                        let _ = tx.send(result);
                        ctx.request_repaint();
                    });
                    PdfState::Loading(rx)
                }
                Err(e) => PdfState::Failed(e.to_string()),
            };
            self.pdf.docs.insert(id, state);
        }

        let ctx = ui.ctx().clone();
        let mut finished: Option<PdfState> = None;
        if let Some(PdfState::Loading(rx)) = self.pdf.docs.get(&id) {
            match rx.try_recv() {
                Ok(Ok((pages, total))) => {
                    let textures = pages
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            ctx.load_texture(
                                format!("pdf{id}_{i}"),
                                ColorImage::from_rgba_unmultiplied([p.width, p.height], &p.rgba),
                                TextureOptions::LINEAR,
                            )
                        })
                        .collect();
                    finished = Some(PdfState::Ready {
                        pages: textures,
                        total,
                    });
                }
                Ok(Err(e)) => finished = Some(PdfState::Failed(e)),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(120))
                }
                Err(TryRecvError::Disconnected) => {
                    finished = Some(PdfState::Failed("rendu interrompu".into()))
                }
            }
        }
        if let Some(s) = finished {
            self.pdf.docs.insert(id, s);
        }

        match self.pdf.docs.get(&id) {
            Some(PdfState::Loading(_)) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Chargement du PDF…");
                });
            }
            Some(PdfState::Failed(e)) => {
                ui.label(
                    RichText::new(format!("Affichage impossible : {e}"))
                        .color(egui::Color32::from_rgb(255, 150, 120)),
                );
                if ui.button("Réessayer").clicked() {
                    self.pdf.docs.remove(&id);
                }
            }
            Some(PdfState::Ready { pages, total }) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Largeur").small().color(theme::muted(ui)));
                    ui.add(
                        egui::Slider::new(&mut self.pdf.width, 400.0..=1400.0).show_value(false),
                    );
                    let shown = pages.len();
                    let note = if shown < *total {
                        format!("{shown} pages affichées sur {total} (« Ouvrir » pour le reste)")
                    } else {
                        format!("{total} page(s)")
                    };
                    ui.label(RichText::new(note).small().color(theme::muted(ui)));
                });
                let width = self.pdf.width;
                egui::ScrollArea::both()
                    .id_salt(("pdf_scroll", id))
                    .max_height(height.max(200.0))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for tex in pages {
                            let size = tex.size_vec2();
                            let scale = width / size.x;
                            ui.add(egui::Image::new((tex.id(), size * scale)));
                            ui.add_space(8.0);
                        }
                    });
            }
            None => {}
        }
    }

    /// Texte extrait du PDF, sélectionnable et copiable.
    fn pdf_text(&mut self, ui: &mut egui::Ui, id: i64, height: f32) {
        if !self.pdf.texts.contains_key(&id) {
            let text = attachments::text(&self.conn, id).unwrap_or_default();
            self.pdf.texts.insert(id, text);
        }
        let empty = self.pdf.texts.get(&id).is_none_or(|t| t.trim().is_empty());
        if empty {
            ui.label(
                RichText::new("Aucun texte enregistré pour ce PDF (PDF scanné, ou ajouté avant cette fonction).")
                    .color(theme::muted(ui)),
            );
            if ui.button("Extraire le texte").clicked() {
                let result = attachments::data(&self.conn, id)
                    .map_err(|e| e.to_string())
                    .and_then(|(_, bytes)| crate::pdf_render::extract_text(&self.data_dir, &bytes));
                match result {
                    Ok(t) if t.trim().is_empty() => {
                        self.status = "Ce PDF ne contient pas de texte (document scanné ?).".into();
                    }
                    Ok(t) => {
                        if self.apply(|c| attachments::set_text(c, id, &t)).is_some() {
                            self.pdf.texts.insert(id, t);
                        }
                    }
                    Err(e) => self.status = format!("Extraction impossible : {e}"),
                }
            }
            return;
        }
        let text = self.pdf.texts.get(&id).cloned().unwrap_or_default();
        ui.horizontal(|ui| {
            if ui.button("Copier le texte").clicked() {
                ui.ctx().copy_text(text.clone());
            }
            ui.label(
                RichText::new(format!("{} mots", text.split_whitespace().count()))
                    .small()
                    .color(theme::muted(ui)),
            );
        });
        egui::ScrollArea::vertical()
            .id_salt(("pdf_text", id))
            .max_height(height.max(200.0))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(text).size(15.0)).selectable(true));
            });
    }
}
