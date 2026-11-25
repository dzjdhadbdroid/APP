//! Module 12 — Carte de l'univers : graphe interactif construit depuis la base.
//!
//! Interactions : molette = zoom, glisser le fond = déplacer, glisser un élément = le repositionner,
//! clic = sélectionner (la fiche s'affiche à droite), double-clic = explorer ses connexions.

use std::collections::{HashMap, HashSet};

use eframe::egui::{
    self, vec2, Align2, Color32, CursorIcon, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2,
};

use super::{kind_color, link_type_label, theme, StudioApp};
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
    /// Afficher toutes les étiquettes de liens (sinon seulement celles de l'élément sélectionné).
    pub show_labels: bool,
    pub node_radius: f32,
    pub highlight: String,
    /// Itérations de placement restantes (relancé à chaque modification des données).
    pub settle: u32,
    /// Ajuster la vue à l'écran dès que le placement est terminé.
    pub fit_pending: bool,
    drag_node: Option<i64>,
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
            show_labels: false,
            node_radius: 12.0,
            highlight: String::new(),
            settle: 300,
            fit_pending: true,
            drag_node: None,
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

const EDGE_LEGEND: [(&str, &str); 5] = [
    ("relation", "relation entre personnages"),
    ("lives_in", "lieu"),
    ("contains", "contenu / apparition"),
    ("advances", "intrigue"),
    ("owns", "possession"),
];

fn short(text: &str, max: usize) -> String {
    let t: String = text.trim().chars().take(max).collect();
    if text.trim().chars().count() > max {
        format!("{t}…")
    } else {
        t
    }
}

impl StudioApp {
    pub(super) fn graph_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Carte de l'univers");
        ui.label(
            RichText::new("Molette : zoom · Glisser le fond : déplacer · Glisser un élément : le repositionner · Clic : fiche à droite · Double-clic : explorer ses connexions")
                .small()
                .color(theme::muted(ui)),
        );
        ui.add_space(4.0);

        // ── barre d'outils ──
        let key_before = (
            self.graph.focus,
            self.graph.depth,
            self.graph.only_isolated,
            self.graph.hidden.len(),
        );
        let mut fit = false;
        let mut leave_focus = false;
        let mut zoom_by = 1.0f32;
        let mut jump: Option<String> = None;
        {
            let g = &mut self.graph;
            ui.horizontal_wrapped(|ui| {
                if ui
                    .button("Ajuster à l'écran")
                    .on_hover_text("Cadre tous les éléments affichés")
                    .clicked()
                {
                    fit = true;
                }
                if ui.button("−").on_hover_text("Dézoomer").clicked() {
                    zoom_by = 0.8;
                }
                if ui.button("+").on_hover_text("Zoomer").clicked() {
                    zoom_by = 1.25;
                }
                let r = ui.add(
                    egui::TextEdit::singleline(&mut g.highlight)
                        .hint_text("Trouver un élément…")
                        .desired_width(180.0),
                );
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    jump = Some(g.highlight.trim().to_lowercase());
                }
                if let Some(f) = g.focus {
                    let _ = f;
                    ui.label(RichText::new("Exploration").strong().color(theme::ACCENT));
                    ui.add(egui::Slider::new(&mut g.depth, 1..=4).text("sauts"));
                    if ui.button("Quitter l'exploration").clicked() {
                        leave_focus = true;
                    }
                }
            });
            // légende = filtres : une case par type d'élément, dans sa couleur
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Afficher :").small().color(theme::muted(ui)));
                for k in NodeKind::ALL {
                    let mut on = !g.hidden.contains(&k);
                    let text =
                        RichText::new(format!("● {}", k.label_plural())).color(kind_color(k));
                    if ui.checkbox(&mut on, text).changed() {
                        if on {
                            g.hidden.remove(&k);
                        } else {
                            g.hidden.insert(k);
                        }
                    }
                }
            });
            egui::CollapsingHeader::new("Options d'affichage")
                .id_salt("graph_options")
                .default_open(false)
                .show(ui, |ui| {
                    ui.checkbox(
                        &mut g.only_isolated,
                        "Seulement les éléments sans lien (cerclés de rouge)",
                    );
                    ui.checkbox(&mut g.show_labels, "Toujours afficher le nom des liens");
                    ui.add(
                        egui::Slider::new(&mut g.node_radius, 6.0..=24.0)
                            .text("Taille des éléments"),
                    );
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Liens :").small().color(theme::muted(ui)));
                        for (ty, label) in EDGE_LEGEND {
                            ui.label(
                                RichText::new(format!("━ {label}"))
                                    .small()
                                    .color(edge_color(ty)),
                            );
                        }
                    });
                });
            if leave_focus {
                g.focus = None;
                g.settle = 300;
                g.fit_pending = true;
            }
            if key_before != (g.focus, g.depth, g.only_isolated, g.hidden.len()) {
                g.settle = 300;
                g.fit_pending = true;
            }
        }

        // ── éléments visibles ──
        let linked: HashSet<i64> = self
            .links
            .iter()
            .flat_map(|l| [l.src_id, l.dst_id])
            .collect();
        let kinds: HashMap<i64, NodeKind> = self.nodes.iter().map(|n| (n.id, n.kind)).collect();
        let degree: HashMap<i64, usize> = {
            let mut d = HashMap::new();
            for l in &self.links {
                *d.entry(l.src_id).or_insert(0) += 1;
                *d.entry(l.dst_id).or_insert(0) += 1;
            }
            d
        };
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
        let edges: Vec<links::Link> = self
            .links
            .iter()
            .filter(|l| visible.contains(&l.src_id) && visible.contains(&l.dst_id))
            .cloned()
            .collect();
        let pairs: Vec<(i64, i64)> = edges.iter().map(|l| (l.src_id, l.dst_id)).collect();

        self.graph.layout.sync(&ids);
        if self.graph.settle > 0 && self.graph.drag_node.is_none() {
            self.graph.layout.step(&pairs);
            self.graph.settle -= 1;
            ui.ctx().request_repaint();
        }

        // ── panneau de droite : fiche de l'élément sélectionné ──
        let (mut open, mut explore, mut center_on) = (None, None, None);
        let selected_node = self
            .selected
            .and_then(|id| self.nodes.iter().find(|n| n.id == id))
            .filter(|n| visible.contains(&n.id) || self.selected == Some(n.id));
        egui::SidePanel::right("graph_side")
            .exact_width(270.0)
            .frame(theme::bar_frame(ui.ctx(), egui::Margin::symmetric(14, 12)))
            .show_inside(ui, |ui| match selected_node {
                Some(n) => {
                    ui.label(RichText::new(n.kind.label().to_uppercase()).small().strong().color(kind_color(n.kind)));
                    ui.label(RichText::new(&n.title).heading());
                    if !n.summary.trim().is_empty() {
                        ui.label(RichText::new(short(&n.summary, 220)).color(theme::muted(ui)));
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add(theme::primary_button("Ouvrir la fiche")).clicked() {
                            open = Some(n.id);
                        }
                        if ui.button("Explorer").on_hover_text("Ne garder que cet élément et ses connexions").clicked() {
                            explore = Some(n.id);
                        }
                    });
                    theme::section_title(ui, "Connexions");
                    let mine: Vec<&links::Link> =
                        self.links.iter().filter(|l| l.src_id == n.id || l.dst_id == n.id).collect();
                    if mine.is_empty() {
                        ui.label(RichText::new("Aucune connexion : cet élément est isolé.").color(theme::muted(ui)));
                    }
                    egui::ScrollArea::vertical().id_salt("graph_side_links").show(ui, |ui| {
                        for l in mine {
                            let other = if l.src_id == n.id { l.dst_id } else { l.src_id };
                            let Some(o) = self.nodes.iter().find(|x| x.id == other) else {
                                continue;
                            };
                            let what = l.label.clone().unwrap_or_else(|| link_type_label(&l.link_type).to_string());
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("●").color(kind_color(o.kind)));
                                if ui.link(&o.title).clicked() {
                                    center_on = Some(o.id);
                                }
                                ui.label(RichText::new(what).small().color(edge_color(&l.link_type)));
                            });
                        }
                    });
                }
                None => {
                    ui.label(RichText::new("Aucun élément sélectionné").strong());
                    ui.label(
                        RichText::new("Clique un élément de la carte pour voir sa fiche et ses connexions ici.")
                            .color(theme::muted(ui)),
                    );
                    theme::section_title(ui, "Aperçu");
                    ui.label(format!("{} éléments affichés", ids.len()));
                    ui.label(format!("{} liens", edges.len()));
                    let isolated = ids.iter().filter(|i| !linked.contains(i)).count();
                    if isolated > 0 {
                        ui.label(
                            RichText::new(format!("{isolated} élément(s) sans lien (cerclés de rouge)"))
                                .color(Color32::from_rgb(230, 110, 110)),
                        );
                    }
                }
            });

        // ── canevas ──
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect: Rect = resp.rect;
        let center = rect.center();

        // ajustement à l'écran (bouton, ou automatique une fois le placement terminé)
        if fit || (self.graph.fit_pending && self.graph.settle == 0) {
            self.graph.fit_pending = false;
            let pts: Vec<(f32, f32)> = ids
                .iter()
                .filter_map(|i| self.graph.layout.pos.get(i).copied())
                .collect();
            if !pts.is_empty() {
                let (mut minx, mut maxx, mut miny, mut maxy) =
                    (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
                for (x, y) in &pts {
                    minx = minx.min(*x);
                    maxx = maxx.max(*x);
                    miny = miny.min(*y);
                    maxy = maxy.max(*y);
                }
                let (w, h) = ((maxx - minx).max(60.0), (maxy - miny).max(60.0));
                let zoom = ((rect.width() - 120.0) / w)
                    .min((rect.height() - 120.0) / h)
                    .clamp(0.2, 2.5);
                self.graph.zoom = zoom;
                self.graph.offset = -vec2((minx + maxx) / 2.0, (miny + maxy) / 2.0) * zoom;
            }
        }
        if zoom_by != 1.0 {
            self.graph.zoom = (self.graph.zoom * zoom_by).clamp(0.2, 4.0);
            self.graph.offset *= zoom_by;
        }
        if let Some(id) = center_on {
            self.select(id);
            if let Some(p) = self.graph.layout.pos.get(&id) {
                self.graph.offset = -vec2(p.0, p.1) * self.graph.zoom;
            }
        }
        if let Some(needle) = jump.filter(|n| !n.is_empty()) {
            let found = self
                .nodes
                .iter()
                .find(|n| visible.contains(&n.id) && n.title.to_lowercase().contains(&needle))
                .map(|n| n.id);
            if let Some(id) = found {
                self.select(id);
                if let Some(p) = self.graph.layout.pos.get(&id) {
                    self.graph.offset = -vec2(p.0, p.1) * self.graph.zoom;
                }
            }
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
        let base = self.graph.node_radius * zoom.clamp(0.6, 2.0);
        let radius_of = |id: &i64| -> f32 {
            let d = degree.get(id).copied().unwrap_or(0).min(12) as f32;
            base * (1.0 + 0.06 * d) // les éléments très connectés sont plus gros
        };

        // survol et glisser-déposer d'un élément
        let hit = |at: Pos2, pos: &HashMap<i64, (f32, f32)>| -> Option<i64> {
            ids.iter()
                .filter_map(|id| {
                    let d = to_screen(*pos.get(id)?).distance(at);
                    (d <= radius_of(id) + 4.0).then_some((*id, d))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id)
        };
        let hovered = resp
            .hover_pos()
            .and_then(|p| hit(p, &self.graph.layout.pos));
        if resp.drag_started() {
            if let Some(at) = resp.interact_pointer_pos() {
                self.graph.drag_node = hit(at, &self.graph.layout.pos);
            }
        }
        if resp.dragged() {
            match self.graph.drag_node {
                Some(id) => {
                    let d = resp.drag_delta() / zoom;
                    if let Some(p) = self.graph.layout.pos.get_mut(&id) {
                        p.0 += d.x;
                        p.1 += d.y;
                    }
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                }
                None => {
                    self.graph.offset += resp.drag_delta();
                    ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
                }
            }
        } else if hovered.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        if resp.drag_stopped() {
            self.graph.drag_node = None;
        }

        // élément « actif » : survolé, sinon sélectionné → son voisinage est mis en avant
        let active = hovered.or(self.selected.filter(|s| visible.contains(s)));
        let neighbors: HashSet<i64> = match active {
            Some(a) => edges
                .iter()
                .filter(|l| l.src_id == a || l.dst_id == a)
                .flat_map(|l| [l.src_id, l.dst_id])
                .collect(),
            None => HashSet::new(),
        };
        let dim = |c: Color32, on: bool| {
            if active.is_some() && !on {
                c.gamma_multiply(0.25)
            } else {
                c
            }
        };

        let image_ctx = ui.ctx().clone();
        let textures: std::collections::HashMap<i64, eframe::egui::TextureHandle> = ids
            .iter()
            .filter_map(|id| self.node_texture(&image_ctx, *id).map(|t| (*id, t)))
            .collect();
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
            let mine = active.is_some_and(|x| l.src_id == x || l.dst_id == x);
            let width = if mine { 2.8 } else { 1.3 };
            painter.line_segment(
                [pa, pb],
                Stroke::new(
                    width,
                    dim(edge_color(&l.link_type), mine || active.is_none()),
                ),
            );
            if (mine || self.graph.show_labels) && zoom >= 0.5 {
                let text = l
                    .label
                    .clone()
                    .unwrap_or_else(|| link_type_label(&l.link_type).to_string());
                let at = pa + (pb - pa) * 0.5;
                let galley_w = text.chars().count() as f32 * 5.4 + 8.0;
                painter.rect_filled(
                    Rect::from_center_size(at, vec2(galley_w, 14.0)),
                    4.0,
                    ui.visuals().panel_fill.gamma_multiply(0.85),
                );
                painter.text(
                    at,
                    Align2::CENTER_CENTER,
                    text,
                    FontId::proportional(11.0),
                    Color32::from_gray(200),
                );
            }
        }

        let needle = self.graph.highlight.trim().to_lowercase();
        for id in &ids {
            let (Some(p), Some(kind)) = (pos.get(id), kinds.get(id)) else {
                continue;
            };
            let sp = to_screen(*p);
            let r = radius_of(id);
            let title = self
                .nodes
                .iter()
                .find(|n| n.id == *id)
                .map_or("", |n| n.title.as_str());
            let in_focus = active == Some(*id) || neighbors.contains(id);
            painter.circle_filled(sp, r, dim(kind_color(*kind), in_focus));
            if let Some(t) = textures.get(id) {
                let half = r * 1.25;
                let img_rect = Rect::from_center_size(sp, vec2(half * 2.0, half * 2.0));
                painter.image(
                    t.id(),
                    img_rect,
                    Rect::from_min_max(eframe::egui::pos2(0.0, 0.0), eframe::egui::pos2(1.0, 1.0)),
                    dim(Color32::WHITE, in_focus),
                );
                painter.rect_stroke(
                    img_rect,
                    0.0,
                    Stroke::new(2.5, dim(kind_color(*kind), in_focus)),
                    eframe::egui::StrokeKind::Outside,
                );
            }
            if self.selected == Some(*id) {
                painter.circle_stroke(sp, r + 3.0, Stroke::new(3.0, Color32::WHITE));
            } else if !linked.contains(id) {
                painter.circle_stroke(
                    sp,
                    r + 2.0,
                    Stroke::new(2.0, Color32::from_rgb(220, 70, 70)),
                );
            }
            if !needle.is_empty() && title.to_lowercase().contains(&needle) {
                painter.circle_stroke(sp, r + 7.0, Stroke::new(2.5, Color32::YELLOW));
            }
            // noms : toujours pour l'élément actif et ses voisins ; sinon seulement si assez zoomé
            if in_focus || (active.is_none() && zoom >= 0.45) {
                painter.text(
                    sp + vec2(0.0, r + 10.0),
                    Align2::CENTER_CENTER,
                    title,
                    FontId::proportional(if active == Some(*id) { 14.0 } else { 12.0 }),
                    dim(Color32::from_gray(230), in_focus),
                );
            }
        }

        // bulle d'aide au survol
        if let Some(h) = hovered {
            if let Some(n) = self.nodes.iter().find(|n| n.id == h) {
                let count = degree.get(&h).copied().unwrap_or(0);
                let tip = format!(
                    "{} — {}\n{} connexion(s){}",
                    n.title,
                    n.kind.label(),
                    count,
                    if n.summary.trim().is_empty() {
                        String::new()
                    } else {
                        format!("\n{}", short(&n.summary, 140))
                    }
                );
                resp.clone().on_hover_text(tip);
            }
        }

        // ── clics ──
        let (mut clicked, mut dbl) = (None, None);
        if let Some(at) = resp.interact_pointer_pos() {
            if resp.double_clicked() {
                dbl = hit(at, &self.graph.layout.pos);
            } else if resp.clicked() {
                clicked = hit(at, &self.graph.layout.pos);
            }
        }
        if resp.clicked() && clicked.is_none() && dbl.is_none() {
            // clic dans le vide : désélectionne
            self.selected = None;
            self.edit = None;
        }
        if let Some(id) = clicked {
            self.select(id);
        }
        if let Some(id) = dbl {
            self.select(id);
            self.graph.focus = Some(id);
            self.graph.settle = 300;
            self.graph.fit_pending = true;
        }
        if let Some(id) = explore {
            self.graph.focus = Some(id);
            self.graph.settle = 300;
            self.graph.fit_pending = true;
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }
}
