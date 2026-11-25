//! Mode lecture : un épisode en plein écran, sans l'interface, pour le relire comme un texte.

use eframe::egui::{self, Color32, RichText};

use super::{theme, StudioApp};
use crate::db::{pdf_analysis, script, timeline};
use crate::pdf::ScriptItem;

pub struct Reader {
    episode: i64,
    size: f32,
}

impl StudioApp {
    pub(super) fn start_reading(&mut self, ctx: &egui::Context, episode: i64) {
        self.reader = Some(Reader {
            episode,
            size: 17.0,
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
    }

    fn stop_reading(&mut self, ctx: &egui::Context) {
        self.reader = None;
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
    }

    /// Dessine le mode lecture s'il est actif. Retourne `true` quand il a pris tout l'écran.
    pub(super) fn reader_ui(&mut self, ctx: &egui::Context) -> bool {
        let Some(reader) = &self.reader else {
            return false;
        };
        let (episode, size) = (reader.episode, reader.size);
        let order: Vec<(i64, String)> = timeline::list_air(&self.conn)
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.id, a.title))
            .collect();
        let pos = order.iter().position(|e| e.0 == episode);
        let title = pos.map_or("Épisode".to_string(), |p| order[p].1.clone());

        let mut new_size = size;
        let mut goto: Option<i64> = None;
        let mut quit = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let can_edit = !ctx.wants_keyboard_input();
        if can_edit {
            let (left, right) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
                )
            });
            if let Some(p) = pos {
                if left && p > 0 {
                    goto = Some(order[p - 1].0);
                }
                if right && p + 1 < order.len() {
                    goto = Some(order[p + 1].0);
                }
            }
        }
        let items = script::episode_items(&self.conn, episode).unwrap_or_default();
        let pdf_text = if items.is_empty() {
            pdf_analysis::episode_text(&self.conn, episode, "script").unwrap_or_default()
        } else {
            String::new()
        };

        egui::TopBottomPanel::top("reader_bar")
            .frame(theme::bar_frame(ctx, egui::Margin::symmetric(16, 8)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("← Quitter (Échap)").clicked() {
                        quit = true;
                    }
                    ui.label(RichText::new(&title).strong().size(16.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("A+").on_hover_text("Texte plus grand").clicked() {
                            new_size = (size + 2.0).min(40.0);
                        }
                        if ui.button("A−").on_hover_text("Texte plus petit").clicked() {
                            new_size = (size - 2.0).max(11.0);
                        }
                        if let Some(p) = pos {
                            if ui
                                .add_enabled(p + 1 < order.len(), egui::Button::new("Suivant →"))
                                .on_hover_text("Flèche droite")
                                .clicked()
                            {
                                goto = Some(order[p + 1].0);
                            }
                            if ui
                                .add_enabled(p > 0, egui::Button::new("← Précédent"))
                                .on_hover_text("Flèche gauche")
                                .clicked()
                            {
                                goto = Some(order[p - 1].0);
                            }
                        }
                    });
                });
            });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .id_salt(("reader", episode))
                .show(ui, |ui| {
                    let width = ui.available_width().min(760.0);
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(width);
                        ui.add_space(16.0);
                        ui.label(RichText::new(&title).size(size * 1.8).strong());
                        ui.add_space(18.0);
                        if items.is_empty() && pdf_text.trim().is_empty() {
                            ui.label(
                                RichText::new(
                                    "Rien à lire : aucune scène écrite ni PDF de scénario.",
                                )
                                .color(Color32::from_gray(140)),
                            );
                        }
                    });
                    ui.vertical_centered(|ui| {
                        ui.set_max_width(width);
                        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                            ui.set_max_width(width);
                            render_items(ui, &items, size);
                            for line in pdf_text.lines() {
                                if line.trim().is_empty() {
                                    ui.add_space(size * 0.6);
                                } else {
                                    ui.label(RichText::new(line.trim()).size(size));
                                }
                            }
                            ui.add_space(60.0);
                        });
                    });
                });
        });

        if quit {
            self.stop_reading(ctx);
        } else if let Some(r) = &mut self.reader {
            r.size = new_size;
            if let Some(g) = goto {
                r.episode = g;
            }
        }
        true
    }
}

fn indent(left: i8) -> egui::Frame {
    egui::Frame::new().inner_margin(egui::Margin {
        left,
        right: 0,
        top: 0,
        bottom: 0,
    })
}

fn render_items(ui: &mut egui::Ui, items: &[ScriptItem], size: f32) {
    let gray = Color32::from_gray(150);
    for item in items {
        match item {
            ScriptItem::Heading(t) => {
                ui.add_space(size * 1.2);
                ui.label(
                    RichText::new(t)
                        .size(size * 1.15)
                        .strong()
                        .color(theme::ACCENT),
                );
                ui.add_space(size * 0.4);
            }
            ScriptItem::Action(t) => {
                ui.label(RichText::new(t).size(size).italics().color(gray));
                ui.add_space(size * 0.4);
            }
            ScriptItem::Speaker(t) => {
                ui.add_space(size * 0.3);
                indent(70).show(ui, |ui| {
                    ui.label(RichText::new(t.to_uppercase()).size(size * 0.9).strong());
                });
            }
            ScriptItem::Dialogue(t) => {
                indent(40).show(ui, |ui| {
                    ui.label(RichText::new(t).size(size));
                });
                ui.add_space(size * 0.4);
            }
            ScriptItem::Direction(t) => {
                indent(55).show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("({t})"))
                            .size(size * 0.9)
                            .italics()
                            .color(gray),
                    );
                });
            }
        }
    }
}
