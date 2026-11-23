//! Module 12 — Universe Map : graphe interactif construit depuis la base.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, vec2, Align2, Color32, FontId, Pos2, RichText, Sense, Stroke, Vec2};

use super::{kind_color, link_type_label, StudioApp};
use crate::db::{links, nodes::NodeKind};
use crate::graph::layout::Layout;

pub struct GraphState {
    pub layout: Layout,
    pub zoom: f32,
    pub offset: Vec2,
    pub hidden: HashSet<NodeKind>,
    /// Mode exploration : n'affiche que l'élément et ses connexions jusqu'à `depth` sauts.
    pub focus: Option<i64>,
    pub depth: i64,
    pub only_isolated: bool,
    pub show_labels: bool,
    pub node_radius: f32,
    pub highlight: String,
    /// Itérations de placement restantes (relancé à chaque modification des données).
    pub settle: u32,
}

impl GraphState {
    pub fn new() -> Self {
        Self {
            layout: Layout::new(),
            zoom: 1.0,
            offset: Vec2::ZERO,
            hidden: HashSet::new(),
            focus: None,
            depth: 2,
            only_isolated: false,
            show_labels: true,
            node_radius: 12.0,
            highlight: String::new(),
            settle: 300,
        }
    }
}

fn edge_color(link_type: &str) -> Color32 {
    match link_type {
        "relation" => Color32::from_rgb(230, 120, 160),
        "lives_in" | "located_at" | "set_in" => Color32::from_rgb(110, 190, 130),
        "appears_in" | "contains" | "involves" => Color32::from_rgb(120, 150, 200),
        "advances" => Color32::from_rgb(240, 160, 80),
        "owns" => Color32::from_rgb(220, 210, 100),
        _ => Color32::from_gray(120),
    }
}

impl StudioApp {
    pub(super) fn graph_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Graphe de l'univers");

        // ── contrôles ──
        let key_before = (
            self.graph.focus,
            self.graph.depth,
            self.graph.only_isolated,
            self.graph.hidden.len(),
        );
        {
            let g = &mut self.graph;
            let mut reset = false;
            ui.horizontal_wrapped(|ui| {
                if ui.button("Vue globale").clicked() {
                    reset = true;
                }
                for k in NodeKind::ALL {
                    let mut on = !g.hidden.contains(&k);
                    let text = RichText::new(k.label_plural()).color(kind_color(k));
                    if ui.checkbox(&mut on, text).changed() {
                        if on {
                            g.hidden.remove(&k);
                        } else {
                            g.hidden.insert(k);
                        }
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.checkbox(&mut g.only_isolated, "Éléments isolés seulement");
                ui.checkbox(&mut g.show_labels, "Étiquettes des liens");
                ui.add(egui::Slider::new(&mut g.node_radius, 6.0..=24.0).text("Taille"));
                if g.focus.is_some() {
                    ui.add(egui::Slider::new(&mut g.depth, 1..=4).text("Profondeur"));
                }
                ui.label("Surligner :");
                ui.add(egui::TextEdit::singleline(&mut g.highlight).desired_width(120.0));
            });
            if reset {
                g.focus = None;
                g.only_isolated = false;
                g.zoom = 1.0;
                g.offset = Vec2::ZERO;
                g.settle = 300;
            }
            if key_before != (g.focus, g.depth, g.only_isolated, g.hidden.len()) {
                g.settle = 300;
            }
        }

        // ── éléments visibles ──
        let linked: HashSet<i64> = self
            .links
            .iter()
            .flat_map(|l| [l.src_id, l.dst_id])
            .collect();
        let kinds: HashMap<i64, NodeKind> = self.nodes.iter().map(|n| (n.id, n.kind)).collect();
        let focus = self.graph.focus;
        let mut ids: Vec<i64> = match focus {
            Some(f) => {
                let mut v = vec![f];
                let reach = links::reachable(&self.conn, f, self.graph.depth).unwrap_or_default();
                v.extend(reach.into_iter().map(|(id, _)| id));
                v
            }
            None => self.nodes.iter().map(|n| n.id).collect(),
        };
        ids.retain(|id| {
            Some(*id) == focus
                || (kinds
                    .get(id)
                    .is_some_and(|k| !self.graph.hidden.contains(k))
                    && (!self.graph.only_isolated || !linked.contains(id)))
        });
        let visible: HashSet<i64> = ids.iter().copied().collect();
        let edges: Vec<&links::Link> = self
            .links
            .iter()
            .filter(|l| visible.contains(&l.src_id) && visible.contains(&l.dst_id))
            .collect();
        let pairs: Vec<(i64, i64)> = edges.iter().map(|l| (l.src_id, l.dst_id)).collect();

        self.graph.layout.sync(&ids);
        if self.graph.settle > 0 {
            self.graph.layout.step(&pairs);
            self.graph.settle -= 1;
            ui.ctx().request_repaint();
        }

        // ── panneau d'information (bas) ──
        let (mut open, mut explore) = (None, None);
        egui::TopBottomPanel::bottom("graph_info")
            .resizable(false)
            .show_inside(ui, |ui| {
                match self.selected.and_then(|id| self.nodes.iter().find(|n| n.id == id)) {
                    Some(n) => {
                        let count = self
                            .links
                            .iter()
                            .filter(|l| l.src_id == n.id || l.dst_id == n.id)
                            .count();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&n.title).strong().color(kind_color(n.kind)));
                            ui.label(format!("{} · {count} connexion(s)", n.kind.label()));
                            if ui.button("Ouvrir la fiche").clicked() {
                                open = Some(n.id);
                            }
                            if ui.button("Explorer ses connexions").clicked() {
                                explore = Some(n.id);
                            }
                        });
                    }
                    None => {
                        ui.label("Clique un nœud pour le sélectionner ; double-clic pour explorer ses connexions.");
                    }
                }
            });

        // ── canevas ──
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let center = resp.rect.center();

        if resp.dragged() {
            self.graph.offset += resp.drag_delta();
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(p) = resp.hover_pos() {
                    let old = self.graph.zoom;
                    let new = (old * (1.0 + scroll * 0.002)).clamp(0.2, 4.0);
                    let ratio = new / old;
                    let rel = p - center;
                    // zoom centré sur le curseur
                    self.graph.offset = rel - (rel - self.graph.offset) * ratio;
                    self.graph.zoom = new;
                }
            }
        }

        let zoom = self.graph.zoom;
        let offset = self.graph.offset;
        let to_screen = |p: (f32, f32)| -> Pos2 { center + offset + vec2(p.0, p.1) * zoom };
        let radius = self.graph.node_radius * zoom.clamp(0.6, 2.0);
        let pos = &self.graph.layout.pos;

        if ids.is_empty() {
            painter.text(
                center,
                Align2::CENTER_CENTER,
                "Aucun élément à afficher.",
                FontId::proportional(16.0),
                Color32::from_gray(140),
            );
        }

        for l in &edges {
            let (Some(a), Some(b)) = (pos.get(&l.src_id), pos.get(&l.dst_id)) else {
                continue;
            };
            let (pa, pb) = (to_screen(*a), to_screen(*b));
            painter.line_segment([pa, pb], Stroke::new(1.5, edge_color(&l.link_type)));
            if self.graph.show_labels && zoom >= 0.6 {
                let text = l
                    .label
                    .clone()
                    .unwrap_or_else(|| link_type_label(&l.link_type).to_string());
                painter.text(
                    pa + (pb - pa) * 0.5,
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(10.0),
                    Color32::from_gray(150),
                );
            }
        }

        let needle = self.graph.highlight.trim().to_lowercase();
        for id in &ids {
            let (Some(p), Some(kind)) = (pos.get(id), kinds.get(id)) else {
                continue;
            };
            let sp = to_screen(*p);
            let title = self
                .nodes
                .iter()
                .find(|n| n.id == *id)
                .map_or("", |n| n.title.as_str());
            painter.circle_filled(sp, radius, kind_color(*kind));
            if self.selected == Some(*id) {
                painter.circle_stroke(sp, radius + 2.0, Stroke::new(2.5, Color32::WHITE));
            } else if !linked.contains(id) {
                painter.circle_stroke(
                    sp,
                    radius + 2.0,
                    Stroke::new(2.0, Color32::from_rgb(220, 70, 70)),
                );
            }
            if !needle.is_empty() && title.to_lowercase().contains(&needle) {
                painter.circle_stroke(sp, radius + 6.0, Stroke::new(2.0, Color32::YELLOW));
            }
            painter.text(
                sp + vec2(0.0, radius + 9.0),
                Align2::CENTER_CENTER,
                title,
                FontId::proportional(12.0),
                Color32::from_gray(225),
            );
        }

        // ── sélection ──
        let hit = |at: Pos2| -> Option<i64> {
            ids.iter()
                .filter_map(|id| {
                    let d = to_screen(*pos.get(id)?).distance(at);
                    (d <= radius + 4.0).then_some((*id, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id)
        };
        let (mut clicked, mut dbl) = (None, None);
        if let Some(at) = resp.interact_pointer_pos() {
            if resp.double_clicked() {
                dbl = hit(at);
            } else if resp.clicked() {
                clicked = hit(at);
            }
        }

        if let Some(id) = clicked {
            self.select(id);
        }
        if let Some(id) = dbl {
            self.select(id);
            self.graph.focus = Some(id);
            self.graph.settle = 300;
        }
        if let Some(id) = explore {
            self.graph.focus = Some(id);
            self.graph.settle = 300;
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }
}
