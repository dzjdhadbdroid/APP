//! Éditeur de scénario : onglet « Scénario » des scènes (blocs) et des épisodes (ordre des scènes).

use std::collections::HashMap;

use eframe::egui::{self, Color32, RichText};

use super::{title_in, Confirm, StudioApp};
use crate::db::{
    links,
    nodes::{self, NodeKind},
    script::{self, BlockType},
    Atomic,
};

#[derive(Clone, Copy, PartialEq, Default)]
pub enum DetailTab {
    #[default]
    Sheet,
    Synopsis,
    Script,
}

/// Copie de travail d'un bloc : enregistrée seulement au clic sur « Enregistrer le scénario ».
struct BlockBuf {
    ty: BlockType,
    character: Option<i64>,
    text: String,
}

#[derive(Default)]
pub struct ScriptState {
    bufs: HashMap<i64, BlockBuf>,
    /// (scène, acte, séquence) en cours d'édition ; 0 = non renseigné.
    structure: Option<(i64, i64, i64)>,
}

impl StudioApp {
    /// Onglets « Fiche / Scénario » des scènes et des épisodes. Retourne `true` si l'onglet
    /// Scénario est affiché (la fiche classique ne doit alors pas être dessinée).
    pub(super) fn script_tabs(&mut self, ui: &mut egui::Ui) -> bool {
        let Some((id, kind)) = self
            .selected
            .and_then(|id| self.nodes.iter().find(|n| n.id == id))
            .map(|n| (n.id, n.kind))
        else {
            return false;
        };
        if !matches!(kind, NodeKind::Scene | NodeKind::Episode) {
            return false;
        }
        ui.horizontal(|ui| {
            for (tab, label) in [
                (DetailTab::Sheet, "Fiche"),
                (DetailTab::Synopsis, "Synopsis"),
                (DetailTab::Script, "Scénario"),
            ] {
                if super::theme::tab(ui, self.detail_tab == tab, label) {
                    self.detail_tab = tab;
                }
            }
        });
        ui.separator();
        if self.detail_tab == DetailTab::Sheet {
            return false;
        }
        if self.detail_tab == DetailTab::Synopsis {
            self.synopsis_tab(ui, id, kind);
            return true;
        }
        match kind {
            NodeKind::Episode => self.episode_script(ui, id),
            _ => self.scene_script(ui, id),
        }
        true
    }

    fn episode_script(&mut self, ui: &mut egui::Ui, episode: i64) {
        let scenes = script::scenes_of(&self.conn, episode).unwrap_or_default();
        let (mut swap, mut open, mut add_scene) = (None, None, false);
        let mut del_scene: Option<i64> = None;
        let mut export_pdf = false;
        let mut read = false;
        ui.label(
            RichText::new("Ordre de présentation des scènes de l'épisode. Ouvre une scène pour écrire son scénario.")
                .color(Color32::from_gray(135)),
        );
        ui.horizontal(|ui| {
            if ui.button("+ Scène").clicked() {
                add_scene = true;
            }
            if ui
                .button("Mode lecture")
                .on_hover_text("Lire l'épisode en plein écran")
                .clicked()
            {
                read = true;
            }
            if ui
                .button("Exporter en PDF (mise en page scénario)")
                .clicked()
            {
                export_pdf = true;
            }
        });
        self.pdf_section(ui, episode);
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("episode_scenes")
            .show(ui, |ui| {
                if scenes.is_empty() {
                    ui.label("Aucune scène.");
                }
                for (i, s) in scenes.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{}.", s.position));
                        if ui.link(&s.title).clicked() {
                            open = Some(s.id);
                        }
                        let structure = match (s.act, s.sequence) {
                            (Some(a), Some(q)) => format!("acte {a} · séquence {q}"),
                            (Some(a), None) => format!("acte {a}"),
                            (None, Some(q)) => format!("séquence {q}"),
                            (None, None) => String::new(),
                        };
                        ui.label(RichText::new(structure).color(Color32::from_gray(135)));
                        if i > 0 && ui.small_button("▲").clicked() {
                            swap = Some((s.id, scenes[i - 1].id));
                        }
                        if i + 1 < scenes.len() && ui.small_button("▼").clicked() {
                            swap = Some((s.id, scenes[i + 1].id));
                        }
                        if ui
                            .small_button("✕")
                            .on_hover_text("Supprimer la scène")
                            .clicked()
                        {
                            del_scene = Some(s.id);
                        }
                    });
                }
                ui.add_space(8.0);
                egui::CollapsingHeader::new("Lire l'épisode (scénario complet)")
                    .id_salt("episode_read")
                    .show(ui, |ui| match script::episode_text(&self.conn, episode) {
                        Ok(text) if text.trim().is_empty() => {
                            ui.label("Rien d'écrit pour l'instant.");
                        }
                        Ok(text) => {
                            ui.monospace(text);
                        }
                        Err(e) => {
                            ui.label(e.to_string());
                        }
                    });
            });
        if let Some((a, b)) = swap {
            self.apply(|c| script::swap_scenes(c, a, b));
        }
        if add_scene {
            self.apply(|c| {
                let pos = nodes::next_scene_position(c, episode)?;
                let scene = nodes::create_scene(c, episode, &format!("Scène {pos}"), pos)?;
                links::create(c, episode, scene, "contains", None, None)?;
                Ok(scene)
            });
        }
        if read {
            let ctx = ui.ctx().clone();
            self.start_reading(&ctx, episode);
        }
        if export_pdf {
            let title = title_in(&self.nodes, episode).to_string();
            let content = script::episode_items(&self.conn, episode)
                .map(|items| crate::pdf::script_to_pdf(&title, &items));
            self.write_export("scenario", "pdf", content);
        }
        if let Some(id) = del_scene {
            self.confirm_delete = Some(Confirm::Node(id));
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    fn scene_script(&mut self, ui: &mut egui::Ui, scene: i64) {
        let blocks = script::list(&self.conn, scene).unwrap_or_default();
        self.script
            .bufs
            .retain(|k, _| blocks.iter().any(|b| b.id == *k));
        if self.script.structure.map(|s| s.0) != Some(scene) {
            let (a, q) = script::structure(&self.conn, scene).unwrap_or((None, None));
            self.script.structure = Some((scene, a.unwrap_or(0), q.unwrap_or(0)));
        }
        let characters: Vec<(i64, String)> = self
            .nodes
            .iter()
            .filter(|n| n.kind == NodeKind::Character)
            .map(|n| (n.id, n.title.clone()))
            .collect();

        ui.label(RichText::new(title_in(&self.nodes, scene)).strong());
        let mut apply_structure = false;
        let mut delete_scene = false;
        let (mut focus, mut review) = (false, false);
        if let Some((_, act, seq)) = self.script.structure.as_mut() {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(act).range(0..=99).prefix("Acte "));
                ui.add(egui::DragValue::new(seq).range(0..=99).prefix("Séquence "));
                if ui.button("Appliquer").clicked() {
                    apply_structure = true;
                }
                ui.label(RichText::new("0 = non renseigné").color(Color32::from_gray(135)));
                ui.menu_button("⋯  Actions", |ui| {
                    if ui.button("Mode concentré (Échap pour sortir)").clicked() {
                        focus = true;
                        ui.close_menu();
                    }
                    if ui.button("→ Relire avec l'IA").clicked() {
                        review = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui
                        .button(
                            RichText::new("Supprimer la scène…")
                                .color(Color32::from_rgb(255, 110, 110)),
                        )
                        .clicked()
                    {
                        delete_scene = true;
                        ui.close_menu();
                    }
                });
            });
        }
        if focus {
            self.focus = true;
        }
        if review {
            self.ai.add_to_context(scene);
            self.ai
                .set_task(crate::ai::prompt::Task::EpisodeInconsistencies);
            self.switch_view(super::View::Ai);
        }
        if delete_scene {
            self.confirm_delete = Some(Confirm::Node(scene));
        }
        ui.separator();
        self.pdf_section(ui, scene);
        ui.separator();

        let (mut mv, mut del, mut add) = (None, None, None);
        egui::ScrollArea::vertical().id_salt("scene_blocks").show(ui, |ui| {
            if blocks.is_empty() {
                ui.label("Scène vide : ajoute une action, un dialogue ou une indication de mise en scène.");
            }
            for b in &blocks {
                let buf = self.script.bufs.entry(b.id).or_insert_with(|| BlockBuf {
                    ty: b.block_type,
                    character: b.character_id,
                    text: b.text.clone(),
                });
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("block_type", b.id)).selected_text(buf.ty.label()).show_ui(
                        ui,
                        |ui| {
                            for t in BlockType::ALL {
                                ui.selectable_value(&mut buf.ty, t, t.label());
                            }
                        },
                    );
                    if buf.ty == BlockType::Dialogue {
                        let who = buf
                            .character
                            .and_then(|c| characters.iter().find(|(id, _)| *id == c))
                            .map_or("(qui parle ?)".to_string(), |(_, n)| n.to_uppercase());
                        egui::ComboBox::from_id_salt(("block_who", b.id)).selected_text(who).show_ui(ui, |ui| {
                            ui.selectable_value(&mut buf.character, None, "(non précisé)");
                            for (id, name) in &characters {
                                ui.selectable_value(&mut buf.character, Some(*id), name);
                            }
                        });
                    }
                    if ui.small_button("▲").clicked() {
                        mv = Some((b.id, true));
                    }
                    if ui.small_button("▼").clicked() {
                        mv = Some((b.id, false));
                    }
                    if ui.small_button("✕").clicked() {
                        del = Some(b.id);
                    }
                });
                ui.add(egui::TextEdit::multiline(&mut buf.text).desired_rows(2).desired_width(f32::INFINITY));
                ui.add_space(4.0);
            }
        });

        let dirty: Vec<(i64, BlockType, Option<i64>, String)> = blocks
            .iter()
            .filter_map(|b| {
                let buf = self.script.bufs.get(&b.id)?;
                let changed =
                    buf.ty != b.block_type || buf.character != b.character_id || buf.text != b.text;
                changed.then(|| (b.id, buf.ty, buf.character, buf.text.clone()))
            })
            .collect();
        let mut save = false;
        ui.horizontal_wrapped(|ui| {
            for t in BlockType::ALL {
                if ui.button(format!("+ {}", t.label())).clicked() {
                    add = Some(t);
                }
            }
            if ui
                .add_enabled(
                    !dirty.is_empty(),
                    egui::Button::new("Enregistrer le scénario"),
                )
                .clicked()
            {
                save = true;
            }
            if !dirty.is_empty() {
                ui.label(
                    RichText::new(format!(
                        "{} bloc(s) modifié(s), non enregistré(s)",
                        dirty.len()
                    ))
                    .color(Color32::from_rgb(255, 180, 80)),
                );
            }
        });

        if apply_structure {
            if let Some((_, act, seq)) = self.script.structure {
                self.apply(|c| {
                    script::set_structure(
                        c,
                        scene,
                        (act > 0).then_some(act),
                        (seq > 0).then_some(seq),
                    )
                });
            }
        }
        if let Some((id, up)) = mv {
            self.apply(|c| script::move_block(c, id, up));
        }
        if let Some(id) = del {
            self.script.bufs.remove(&id);
            self.apply(|c| script::delete(c, id));
        }
        if let Some(t) = add {
            self.apply(|c| script::add(c, scene, t, None, ""));
        }
        if save {
            let saved = self.apply(|c| {
                let tx = Atomic::begin(c)?;
                for (id, ty, who, text) in &dirty {
                    script::update(&tx, *id, *ty, *who, text)?;
                }
                tx.commit()
            });
            if saved.is_some() {
                self.script.bufs.clear(); // recopiées depuis la base au prochain affichage
            }
        }
    }
}
