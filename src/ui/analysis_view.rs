//! Analyse de l'écriture : équilibre d'une saison, suivi des gags, frise de saison, statistiques.

use eframe::egui::{self, Color32, RichText, Sense, Vec2};

use super::{kind_color, theme, StudioApp};
use crate::db::{analysis, nodes::NodeKind, pdf_analysis, timeline};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Balance,
    Gags,
    Wall,
    Stats,
    Pdf,
}

#[derive(Default)]
pub struct AnalysisState {
    tab: Tab,
    season: Option<i64>,
}

/// Mots par page de scénario (estimation : une page de dialogue tient environ 180 mots).
const WORDS_PER_PAGE: f64 = 180.0;
/// Nombre d'épisodes consécutifs sans apparition à partir duquel on signale une absence.
const ABSENT_AFTER: usize = 2;

const PALETTE: [Color32; 8] = [
    Color32::from_rgb(235, 130, 130),
    Color32::from_rgb(240, 180, 90),
    Color32::from_rgb(220, 205, 100),
    Color32::from_rgb(120, 200, 130),
    Color32::from_rgb(100, 190, 200),
    Color32::from_rgb(120, 160, 240),
    Color32::from_rgb(180, 140, 235),
    Color32::from_rgb(225, 130, 190),
];

fn plot_color(id: i64) -> Color32 {
    PALETTE[id.rem_euclid(8) as usize]
}

fn status_label(code: &str) -> &str {
    match code {
        "idea" => "Idée",
        "outline" => "Plan",
        "drafting" => "En écriture",
        "written" => "Écrit",
        "final" => "Version finale",
        other => other,
    }
}

/// Épisodes consécutifs sans valeur, en partant de la fin.
fn silent_tail(values: &[i64]) -> usize {
    values.iter().rev().take_while(|v| **v == 0).count()
}

fn ep_label(c: &analysis::EpisodeCol) -> String {
    format!("S{}E{}", c.season, c.number)
}

/// Case colorée proportionnelle à `value / max`.
fn heat_cell(ui: &mut egui::Ui, value: i64, max: i64, color: Color32, tip: &str) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(46.0, 24.0), Sense::hover());
    let bg = ui.visuals().faint_bg_color;
    let fill = if value <= 0 {
        bg
    } else {
        let t = 0.25 + 0.75 * (value as f32 / max.max(1) as f32);
        let base = ui.visuals().extreme_bg_color;
        Color32::from_rgb(
            (base.r() as f32 + (color.r() as f32 - base.r() as f32) * t) as u8,
            (base.g() as f32 + (color.g() as f32 - base.g() as f32) * t) as u8,
            (base.b() as f32 + (color.b() as f32 - base.b() as f32) * t) as u8,
        )
    };
    ui.painter().rect_filled(rect.shrink(1.0), 3.0, fill);
    if value > 0 {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            value.to_string(),
            egui::FontId::proportional(11.0),
            Color32::from_gray(20),
        );
    }
    resp.on_hover_text(tip);
}

fn matrix_grid(ui: &mut egui::Ui, id: &str, m: &analysis::Matrix, color: Color32, unit: &str) {
    if m.rows.is_empty() {
        ui.label(RichText::new("Rien à afficher pour cette saison.").color(theme::muted(ui)));
        return;
    }
    let max = m
        .rows
        .iter()
        .flat_map(|r| r.values.iter().copied())
        .max()
        .unwrap_or(1);
    egui::ScrollArea::horizontal().id_salt(id).show(ui, |ui| {
        egui::Grid::new(id).spacing([2.0, 2.0]).show(ui, |ui| {
            ui.label("");
            for c in &m.columns {
                ui.label(RichText::new(ep_label(c)).small().color(theme::muted(ui)))
                    .on_hover_text(&c.title);
            }
            ui.label("");
            ui.end_row();
            for r in &m.rows {
                ui.label(&r.title);
                for (c, v) in m.columns.iter().zip(&r.values) {
                    heat_cell(
                        ui,
                        *v,
                        max,
                        color,
                        &format!("{} — {} : {v} {unit}", r.title, c.title),
                    );
                }
                let gap = silent_tail(&r.values);
                if gap >= ABSENT_AFTER && gap < r.values.len() {
                    ui.label(
                        RichText::new(format!("⚠ absent depuis {gap} épisodes"))
                            .small()
                            .color(Color32::from_rgb(255, 150, 90)),
                    );
                } else if gap == r.values.len() && !r.values.is_empty() {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    });
}

impl StudioApp {
    pub(super) fn analysis_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Analyse de l'écriture");
        ui.horizontal(|ui| {
            for (t, label) in [
                (Tab::Balance, "Équilibre de la saison"),
                (Tab::Gags, "Suivi des gags"),
                (Tab::Wall, "Frise de saison"),
                (Tab::Stats, "Statistiques"),
                (Tab::Pdf, "Recoupement PDF"),
            ] {
                if ui.selectable_label(self.analysis.tab == t, label).clicked() {
                    self.analysis.tab = t;
                }
            }
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .show(ui, |ui| match self.analysis.tab {
                Tab::Balance | Tab::Wall => self.season_tabs(ui),
                Tab::Gags => self.gags_tab(ui),
                Tab::Stats => self.stats_tab(ui),
                Tab::Pdf => self.pdf_check_tab(ui),
            });
    }

    /// Choix de la saison puis contenu (équilibre ou frise).
    fn season_tabs(&mut self, ui: &mut egui::Ui) {
        let seasons = timeline::seasons(&self.conn).unwrap_or_default();
        if seasons.is_empty() {
            ui.label("Crée d'abord des épisodes (Écriture) pour voir l'analyse de la saison.");
            return;
        }
        let mut season = self
            .analysis
            .season
            .filter(|s| seasons.contains(s))
            .unwrap_or(seasons[0]);
        ui.horizontal(|ui| {
            ui.label("Saison :");
            for s in &seasons {
                ui.selectable_value(&mut season, *s, format!("{s}"));
            }
        });
        self.analysis.season = Some(season);
        ui.add_space(6.0);
        if self.analysis.tab == Tab::Wall {
            self.wall_tab(ui, season);
            return;
        }
        let state = (
            analysis::speech_matrix(&self.conn, season),
            analysis::places_matrix(&self.conn, season),
            analysis::plots_matrix(&self.conn, season),
        );
        let (Ok(speech), Ok(places), Ok(plots)) = state else {
            ui.label("Impossible de calculer l'analyse.");
            return;
        };
        ui.label(
            RichText::new(
                "Plus la case est colorée, plus l'élément pèse dans l'épisode. Un ⚠ signale un élément \
                 qui n'apparaît plus depuis plusieurs épisodes.",
            )
            .color(theme::muted(ui)),
        );
        theme::section_title(ui, "Temps de parole (mots de dialogue par personnage)");
        matrix_grid(
            ui,
            "speech",
            &speech,
            kind_color(NodeKind::Character),
            "mots",
        );
        ui.add_space(10.0);
        theme::section_title(ui, "Lieux utilisés (scènes par lieu)");
        matrix_grid(
            ui,
            "places",
            &places,
            kind_color(NodeKind::Place),
            "scène(s)",
        );
        ui.add_space(10.0);
        theme::section_title(ui, "Intrigues présentes");
        matrix_grid(ui, "plots", &plots, kind_color(NodeKind::Plot), "présence");
    }

    fn gags_tab(&mut self, ui: &mut egui::Ui) {
        let Ok(report) = analysis::gag_report(&self.conn) else {
            ui.label("Impossible de calculer le suivi des gags.");
            return;
        };
        ui.label(
            RichText::new(
                "Les gags sont les intrigues de type « running gag ». Une case pleine = le gag est utilisé \
                 dans l'épisode (lien direct ou via une scène).",
            )
            .color(theme::muted(ui)),
        );
        if report.gags.is_empty() {
            ui.label(
                "Aucun running gag : passe une intrigue en type « running gag » dans sa fiche.",
            );
            return;
        }
        let mut open = None;
        egui::ScrollArea::horizontal()
            .id_salt("gags")
            .show(ui, |ui| {
                egui::Grid::new("gags_grid")
                    .spacing([2.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("");
                        for c in &report.columns {
                            ui.label(RichText::new(ep_label(c)).small().color(theme::muted(ui)))
                                .on_hover_text(&c.title);
                        }
                        ui.label("");
                        ui.end_row();
                        for g in &report.gags {
                            if ui.link(&g.title).clicked() {
                                open = Some(g.id);
                            }
                            for (c, used) in report.columns.iter().zip(&g.uses) {
                                heat_cell(
                                    ui,
                                    i64::from(*used),
                                    1,
                                    kind_color(NodeKind::Plot),
                                    &format!("{} — {}", g.title, c.title),
                                );
                            }
                            let (text, color) = match g.since {
                                None => ("jamais utilisé".to_string(), Color32::from_gray(140)),
                                Some(0) => (
                                    format!("dans le dernier épisode · {}×", g.count),
                                    Color32::from_rgb(120, 200, 130),
                                ),
                                Some(n) if n >= 3 => (
                                    format!("⚠ pas servi depuis {n} épisodes · {}×", g.count),
                                    Color32::from_rgb(255, 130, 110),
                                ),
                                Some(n) => (
                                    format!("pas servi depuis {n} épisode(s) · {}×", g.count),
                                    Color32::from_gray(170),
                                ),
                            };
                            ui.label(RichText::new(text).small().color(color));
                            ui.end_row();
                        }
                    });
            });
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    fn wall_tab(&mut self, ui: &mut egui::Ui, season: i64) {
        let Ok(cards) = analysis::season_cards(&self.conn, season) else {
            ui.label("Impossible de construire la frise.");
            return;
        };
        ui.label(
            RichText::new("Chaque épisode est une carte ; les pastilles colorées sont ses intrigues. Clique sur un titre pour l'ouvrir.")
                .color(theme::muted(ui)),
        );
        ui.add_space(6.0);
        let mut open = None;
        egui::ScrollArea::horizontal()
            .id_salt("wall")
            .show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    for c in &cards {
                        egui::Frame::group(ui.style())
                            .fill(kind_color(NodeKind::Episode).gamma_multiply(0.18))
                            .show(ui, |ui| {
                                ui.set_width(190.0);
                                ui.set_min_height(170.0);
                                ui.label(
                                    RichText::new(ep_label(&c.episode))
                                        .small()
                                        .color(theme::muted(ui)),
                                );
                                if ui.link(RichText::new(&c.episode.title).strong()).clicked() {
                                    open = Some(c.episode.id);
                                }
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {} scène(s) · {} mots",
                                        status_label(&c.status),
                                        c.scenes,
                                        c.words
                                    ))
                                    .small()
                                    .color(theme::muted(ui)),
                                );
                                ui.add_space(4.0);
                                if c.plots.is_empty() {
                                    ui.label(
                                        RichText::new("aucune intrigue")
                                            .small()
                                            .color(Color32::from_gray(130)),
                                    );
                                }
                                for (id, title) in &c.plots {
                                    egui::Frame::none()
                                        .fill(plot_color(*id))
                                        .rounding(4.0)
                                        .inner_margin(egui::Margin::symmetric(6, 2))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new(title)
                                                    .small()
                                                    .color(Color32::from_gray(20)),
                                            );
                                        });
                                }
                            });
                    }
                });
            });
        if let Some(id) = open {
            self.go_to(id);
        }
    }

    fn stats_tab(&mut self, ui: &mut egui::Ui) {
        let Ok(s) = analysis::stats(&self.conn) else {
            ui.label("Impossible de calculer les statistiques.");
            return;
        };
        let pages = (s.words as f64 / WORDS_PER_PAGE).ceil() as i64;
        let written = s
            .by_status
            .iter()
            .filter(|(st, _)| matches!(st.as_str(), "written" | "final"))
            .map(|(_, n)| n)
            .sum::<i64>();
        ui.horizontal_wrapped(|ui| {
            for (value, label) in [
                (s.episodes.to_string(), "épisodes"),
                (written.to_string(), "épisodes écrits"),
                (s.scenes.to_string(), "scènes"),
                (s.lines.to_string(), "répliques"),
                (s.words.to_string(), "mots"),
                (format!("≈ {pages}"), "pages (estimation)"),
                (s.characters.to_string(), "personnages"),
            ] {
                theme::card(ui).show(ui, |ui| {
                    ui.set_min_width(100.0);
                    ui.label(RichText::new(value).size(24.0).strong());
                    ui.label(RichText::new(label).small().color(theme::muted(ui)));
                });
            }
        });
        ui.label(
            RichText::new(format!(
                "Pages estimées à partir du nombre de mots (≈ {WORDS_PER_PAGE:.0} mots par page de scénario)."
            ))
            .small()
            .color(theme::muted(ui)),
        );
        if !s.by_status.is_empty() {
            ui.add_space(6.0);
            let parts: Vec<String> = s
                .by_status
                .iter()
                .map(|(st, n)| format!("{} : {n}", status_label(st)))
                .collect();
            ui.label(format!("Avancement — {}", parts.join(" · ")));
        }
        ui.add_space(12.0);
        theme::section_title(ui, "Mots écrits par épisode");
        let bars: Vec<(String, i64)> = s
            .words_per_episode
            .iter()
            .map(|(c, w)| (ep_label(c), *w))
            .collect();
        bar_chart(ui, &bars, kind_color(NodeKind::Episode));
        ui.add_space(12.0);
        theme::section_title(ui, "Évolution : éléments créés par mois");
        bar_chart(ui, &s.created_per_month, kind_color(NodeKind::Character));
    }
}

/// Histogramme simple dessiné à la main : une barre par entrée, valeur au-dessus, libellé dessous.
fn bar_chart(ui: &mut egui::Ui, data: &[(String, i64)], color: Color32) {
    if data.is_empty() || data.iter().all(|d| d.1 == 0) {
        ui.label(RichText::new("Pas encore de données.").color(theme::muted(ui)));
        return;
    }
    let max = data.iter().map(|d| d.1).max().unwrap_or(1).max(1) as f32;
    let slot = 54.0;
    let height = 150.0;
    egui::ScrollArea::horizontal()
        .id_salt(format!("chart_{}", data[0].0))
        .show(ui, |ui| {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(slot * data.len() as f32, height + 36.0),
                Sense::hover(),
            );
            let painter = ui.painter_at(rect);
            let text = ui.visuals().text_color();
            for (i, (label, v)) in data.iter().enumerate() {
                let x = rect.left() + slot * i as f32;
                let h = height * (*v as f32 / max);
                let bar = egui::Rect::from_min_max(
                    egui::pos2(x + 8.0, rect.top() + 16.0 + height - h),
                    egui::pos2(x + slot - 8.0, rect.top() + 16.0 + height),
                );
                painter.rect_filled(bar, 3.0, color);
                painter.text(
                    egui::pos2(bar.center().x, bar.top() - 2.0),
                    egui::Align2::CENTER_BOTTOM,
                    v.to_string(),
                    egui::FontId::proportional(11.0),
                    text,
                );
                painter.text(
                    egui::pos2(bar.center().x, rect.top() + 20.0 + height),
                    egui::Align2::CENTER_TOP,
                    label,
                    egui::FontId::proportional(11.0),
                    text,
                );
            }
        });
}

impl StudioApp {
    /// Recoupe le texte des PDF (scénario, synopsis) avec la bible.
    fn pdf_check_tab(&mut self, ui: &mut egui::Ui) {
        let Ok(checks) = pdf_analysis::cross_check(&self.conn) else {
            ui.label("Impossible d'analyser les PDF.");
            return;
        };
        ui.label(
            RichText::new(
                "Le texte des PDF rattachés aux épisodes est lu pour repérer qui parle (nom en majuscules \
                 avant la réplique, ou « NOM : réplique »), les lieux (intitulés INT./EXT.) et les personnages \
                 cités, puis comparé à ta bible. Lecture automatique : à vérifier, surtout si la mise en page est inhabituelle.",
            )
            .color(theme::muted(ui)),
        );
        if checks.is_empty() {
            ui.add_space(8.0);
            ui.label(
                "Aucun PDF avec du texte lisible. Importe un scénario ou un synopsis dans un épisode \
                 (un PDF scanné, sans texte, ne peut pas être analysé).",
            );
            return;
        }
        let mut create_char: Option<String> = None;
        let mut create_place: Option<String> = None;
        let mut open = None;
        for c in &checks {
            ui.add_space(8.0);
            theme::card(ui).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if ui
                    .link(RichText::new(&c.title).strong().size(16.0))
                    .clicked()
                {
                    open = Some(c.episode_id);
                }
                if c.script.speakers.is_empty() && c.script.locations.is_empty() {
                    ui.label(
                        RichText::new("Aucun intervenant ni lieu détecté dans le scénario.")
                            .small(),
                    );
                }
                if !c.script.speakers.is_empty() {
                    ui.label(RichText::new("Qui parle").small().color(theme::muted(ui)));
                    for (s, id) in c.script.speakers.iter().zip(&c.speaker_ids) {
                        ui.horizontal(|ui| match id {
                            Some(_) => {
                                ui.colored_label(Color32::from_rgb(120, 200, 130), "✓");
                                ui.label(format!(
                                    "{} — {} réplique(s), {} mots",
                                    s.name, s.lines, s.words
                                ));
                            }
                            None => {
                                ui.colored_label(Color32::from_rgb(255, 150, 90), "⚠");
                                ui.label(format!(
                                    "{} — {} réplique(s), {} mots · absent de la bible",
                                    s.name, s.lines, s.words
                                ));
                                if ui.small_button("Créer la fiche").clicked() {
                                    create_char = Some(s.name.clone());
                                }
                            }
                        });
                    }
                }
                if !c.script.locations.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Lieux").small().color(theme::muted(ui)));
                    for ((l, n), id) in c.script.locations.iter().zip(&c.place_ids) {
                        ui.horizontal(|ui| match id {
                            Some(_) => {
                                ui.colored_label(Color32::from_rgb(120, 200, 130), "✓");
                                ui.label(format!("{l} — {n} scène(s)"));
                            }
                            None => {
                                ui.colored_label(Color32::from_rgb(255, 150, 90), "⚠");
                                ui.label(format!("{l} — {n} scène(s) · absent de la bible"));
                                if ui.small_button("Créer le lieu").clicked() {
                                    create_place = Some(l.clone());
                                }
                            }
                        });
                    }
                }
                if !c.mentioned_only.is_empty() {
                    ui.add_space(4.0);
                    let names: Vec<&str> = c.mentioned_only.iter().map(|m| m.1.as_str()).collect();
                    ui.label(
                        RichText::new(format!("Cités sans parler : {}", names.join(", "))).small(),
                    );
                }
            });
        }
        if let Some(id) = open {
            self.go_to(id);
        }
        if let Some(name) = create_char {
            let title = capitalize(&name);
            self.apply(|conn| crate::db::nodes::create(conn, NodeKind::Character, &title));
        }
        if let Some(name) = create_place {
            let title = capitalize(&name);
            self.apply(|conn| crate::db::nodes::create(conn, NodeKind::Place, &title));
        }
    }
}

/// « ALICE MARTIN » → « Alice Martin ».
fn capitalize(s: &str) -> String {
    s.split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
