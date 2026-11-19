//! Assistant IA : l'utilisateur choisit le contexte, l'IA répond en citant ses sources,
//! et rien n'est écrit dans la base sans un clic explicite.

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Duration;

use eframe::egui::{self, Color32, RichText};
use rusqlite::Connection;

use super::{kind_color, title_in, StudioApp};
use crate::ai::{
    context,
    prompt::{self, Task},
    provider::{self, ProviderConfig, ProviderKind},
};
use crate::db::{history, settings};

/// ≈ 15 000 jetons : au-delà, les éléments suivants sont omis (et signalés).
const MAX_CONTEXT_CHARS: usize = 60_000;

pub struct AiState {
    kind: ProviderKind,
    base_url: String,
    model: String,
    api_key: String,
    ctx: Vec<i64>,
    neighbors: bool,
    task: Task,
    question: String,
    rx: Option<Receiver<Result<String, String>>>,
    allowed: HashSet<i64>,
    omitted: usize,
    answer: Option<String>,
    error: Option<String>,
    note_target: Option<i64>,
}

impl AiState {
    pub fn load(conn: &Connection) -> Self {
        let get = |k: &str| settings::get(conn, k).ok().flatten().unwrap_or_default();
        let kind = ProviderKind::parse(&get("ai.provider"));
        let or_default = |v: String, d: &str| if v.is_empty() { d.to_string() } else { v };
        Self {
            kind,
            base_url: or_default(get("ai.base_url"), kind.default_base_url()),
            model: or_default(get("ai.model"), kind.default_model()),
            api_key: get("ai.api_key"),
            ctx: Vec::new(),
            neighbors: false,
            task: Task::PersonalitySummary,
            question: String::new(),
            rx: None,
            allowed: HashSet::new(),
            omitted: 0,
            answer: None,
            error: None,
            note_target: None,
        }
    }

    /// Ajoute un élément au contexte (utilisé par le bouton « → Assistant IA » des fiches).
    pub fn add_to_context(&mut self, id: i64) {
        if !self.ctx.contains(&id) {
            self.ctx.push(id);
        }
    }

    fn config(&self) -> ProviderConfig {
        ProviderConfig {
            kind: self.kind,
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            api_key: self.api_key.clone(),
        }
    }
}

impl StudioApp {
    /// Récupère la réponse du fil d'exécution réseau, sans jamais bloquer l'interface.
    pub(super) fn poll_ai(&mut self, ctx: &egui::Context) {
        let polled = self.ai.rx.as_ref().map(|rx| rx.try_recv());
        match polled {
            Some(Ok(Ok(text))) => {
                self.ai.answer = Some(text);
                self.ai.error = None;
                self.ai.rx = None;
            }
            Some(Ok(Err(e))) => {
                self.ai.error = Some(e);
                self.ai.rx = None;
            }
            Some(Err(TryRecvError::Empty)) => ctx.request_repaint_after(Duration::from_millis(200)),
            Some(Err(TryRecvError::Disconnected)) => {
                self.ai.error = Some("Le traitement s'est interrompu.".to_string());
                self.ai.rx = None;
            }
            None => {}
        }
    }

    fn ai_save_settings(&mut self) {
        let a = &self.ai;
        let pairs = [
            ("ai.provider", a.kind.code()),
            ("ai.base_url", a.base_url.as_str()),
            ("ai.model", a.model.as_str()),
            ("ai.api_key", a.api_key.as_str()),
        ];
        for (k, v) in pairs {
            if let Err(e) = settings::set(&self.conn, k, v) {
                self.status = format!("Erreur : {e}");
                return;
            }
        }
        self.status = "Configuration IA enregistrée.".to_string();
    }

    fn ai_send(&mut self, egui_ctx: egui::Context) {
        if self.ai.ctx.is_empty() {
            self.ai.error = Some("Sélectionne au moins un élément de contexte.".to_string());
            return;
        }
        let items = match context::gather(&self.conn, &self.ai.ctx, self.ai.neighbors) {
            Ok(i) => i,
            Err(e) => {
                self.ai.error = Some(e.to_string());
                return;
            }
        };
        let (text, included) = prompt::render_context(&items, MAX_CONTEXT_CHARS);
        self.ai.allowed = prompt::allowed_ids(&items[..included]);
        self.ai.omitted = items.len() - included;
        let user = prompt::build_user_prompt(self.ai.task, &self.ai.question, &text);

        let mut cfg = self.ai.config();
        if cfg.api_key.trim().is_empty() {
            cfg.api_key = std::env::var(cfg.kind.env_key()).unwrap_or_default();
        }
        let (tx, rx) = mpsc::channel();
        self.ai.rx = Some(rx);
        self.ai.error = None;
        self.ai.answer = None;
        std::thread::spawn(move || {
            let result = provider::complete(&cfg, prompt::SYSTEM_PROMPT, &user);
            let _ = tx.send(result);
            egui_ctx.request_repaint();
        });
    }

    pub(super) fn ai_view(&mut self, ui: &mut egui::Ui) {
        let gray = Color32::from_gray(135);
        ui.heading("Assistant IA");
        ui.label(
            RichText::new(
                "L'IA ne modifie jamais tes données : elle propose du texte que tu valides toi-même, \
                 et cite les fiches sur lesquelles elle s'appuie ([#id]). Seuls les éléments choisis \
                 ci-dessous sont envoyés au fournisseur ; le reste de l'application fonctionne sans Internet.",
            )
            .color(gray),
        );

        // ── configuration ──
        let mut save_cfg = false;
        egui::CollapsingHeader::new("Configuration du fournisseur").id_salt("ai_cfg").show(ui, |ui| {
            let ai = &mut self.ai;
            let before = ai.kind;
            egui::ComboBox::from_id_salt("ai_kind").selected_text(ai.kind.label()).show_ui(ui, |ui| {
                for k in ProviderKind::ALL {
                    ui.selectable_value(&mut ai.kind, k, k.label());
                }
            });
            if ai.kind != before {
                ai.base_url = ai.kind.default_base_url().to_string();
                ai.model = ai.kind.default_model().to_string();
            }
            ui.label("Adresse de l'API (ex. http://localhost:11434/v1 pour un modèle local Ollama)");
            ui.add(egui::TextEdit::singleline(&mut ai.base_url).desired_width(f32::INFINITY));
            ui.label("Modèle");
            ui.add(egui::TextEdit::singleline(&mut ai.model).desired_width(f32::INFINITY));
            ui.label("Clé API (laisser vide pour utiliser la variable d'environnement, ou pour un modèle local)");
            ui.add(egui::TextEdit::singleline(&mut ai.api_key).password(true).desired_width(f32::INFINITY));
            ui.label(
                RichText::new(
                    "La clé enregistrée ici est stockée en clair dans ta base locale, donc aussi dans tes sauvegardes \
                     (mais jamais dans les exports JSON/Markdown). La variable d'environnement évite cela.",
                )
                .color(gray),
            );
            if ui.button("Enregistrer la configuration").clicked() {
                save_cfg = true;
            }
        });
        if save_cfg {
            self.ai_save_settings();
        }

        // ── contexte ──
        ui.separator();
        ui.heading("Contexte envoyé à l'IA");
        let mut remove = None;
        for id in &self.ai.ctx {
            let kind = self.nodes.iter().find(|n| n.id == *id).map(|n| n.kind);
            ui.horizontal(|ui| {
                if let Some(k) = kind {
                    ui.label(RichText::new(k.label()).color(kind_color(k)));
                }
                ui.label(title_in(&self.nodes, *id));
                if ui.small_button("✕").clicked() {
                    remove = Some(*id);
                }
            });
        }
        if let Some(id) = remove {
            self.ai.ctx.retain(|x| *x != id);
        }
        if self.ai.ctx.is_empty() {
            ui.label("Aucun élément. Ajoute-en ci-dessous, ou depuis une fiche avec « → Assistant IA ».");
        }
        let mut add = None;
        egui::ComboBox::from_id_salt("ai_add").selected_text("Ajouter un élément…").show_ui(ui, |ui| {
            for n in self.nodes.iter().filter(|n| !self.ai.ctx.contains(&n.id)) {
                if ui.selectable_label(false, format!("{} · {}", n.kind.label(), n.title)).clicked() {
                    add = Some(n.id);
                }
            }
        });
        if let Some(id) = add {
            self.ai.add_to_context(id);
        }
        ui.checkbox(&mut self.ai.neighbors, "Inclure aussi les éléments directement reliés");

        egui::CollapsingHeader::new("Aperçu exact de ce qui sera envoyé").id_salt("ai_preview").show(ui, |ui| {
            match context::gather(&self.conn, &self.ai.ctx, self.ai.neighbors) {
                Ok(items) => {
                    let (text, included) = prompt::render_context(&items, MAX_CONTEXT_CHARS);
                    ui.label(format!(
                        "{included} élément(s) sur {}, {} caractères.",
                        items.len(),
                        text.chars().count()
                    ));
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        ui.monospace(text);
                    });
                }
                Err(e) => {
                    ui.label(e.to_string());
                }
            }
        });

        // ── tâche ──
        ui.separator();
        ui.heading("Demande");
        egui::ComboBox::from_id_salt("ai_task").selected_text(self.ai.task.label()).show_ui(ui, |ui| {
            for t in Task::ALL {
                ui.selectable_value(&mut self.ai.task, t, t.label());
            }
        });
        ui.add(
            egui::TextEdit::multiline(&mut self.ai.question)
                .hint_text("Précisions ou question libre (facultatif sauf pour « Question libre »)")
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        let busy = self.ai.rx.is_some();
        let mut send = false;
        ui.horizontal(|ui| {
            if ui.add_enabled(!busy, egui::Button::new("Envoyer à l'IA")).clicked() {
                send = true;
            }
            if busy {
                ui.spinner();
                ui.label("En attente de la réponse…");
            }
        });
        if send {
            self.ai_send(ui.ctx().clone());
        }

        // ── réponse ──
        if let Some(err) = &self.ai.error {
            ui.colored_label(Color32::from_rgb(255, 110, 110), err);
        }
        let Some(answer) = self.ai.answer.clone() else { return };
        ui.separator();
        ui.heading("Réponse");
        if self.ai.omitted > 0 {
            ui.colored_label(
                Color32::from_rgb(255, 180, 80),
                format!("{} élément(s) n'ont pas été envoyés (limite de taille du contexte).", self.ai.omitted),
            );
        }
        egui::ScrollArea::vertical().id_salt("ai_answer").max_height(360.0).show(ui, |ui| {
            ui.add(egui::Label::new(answer.as_str()).selectable(true));
        });

        let (valid, unknown) = prompt::extract_citations(&answer, &self.ai.allowed);
        let mut open = None;
        if !valid.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.label("Sources citées :");
                for id in &valid {
                    if ui.link(format!("#{id} {}", title_in(&self.nodes, *id))).clicked() {
                        open = Some(*id);
                    }
                }
            });
        }
        if !unknown.is_empty() {
            let list: Vec<String> = unknown.iter().map(|i| format!("#{i}")).collect();
            ui.colored_label(
                Color32::from_rgb(255, 180, 80),
                format!("Citations non vérifiées (absentes du contexte envoyé) : {}. À ne pas croire sur parole.", list.join(", ")),
            );
        }
        if valid.is_empty() && unknown.is_empty() {
            ui.colored_label(Color32::from_rgb(255, 180, 80), "Aucune source citée : vérifie cette réponse avec soin.");
        }

        // ── actions explicites de l'utilisateur ──
        let mut add_note = None;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Copier la réponse").clicked() {
                ui.ctx().copy_text(answer.clone());
            }
            let target = self
                .ai
                .note_target
                .map_or_else(|| "Ajouter aux notes de…".to_string(), |t| title_in(&self.nodes, t).to_string());
            egui::ComboBox::from_id_salt("ai_note_target").selected_text(target).show_ui(ui, |ui| {
                for n in &self.nodes {
                    ui.selectable_value(&mut self.ai.note_target, Some(n.id), format!("{} · {}", n.kind.label(), n.title));
                }
            });
            if ui.add_enabled(self.ai.note_target.is_some(), egui::Button::new("Ajouter à ses notes")).clicked() {
                add_note = self.ai.note_target;
            }
        });
        if let Some(target) = add_note {
            let text = format!("[Suggestion IA — à relire]\n{answer}");
            if self.apply(|c| history::append_note(c, target, &text)).is_some() {
                self.status = "Ajouté aux notes (ancienne version conservée dans l'historique).".to_string();
            }
        }
        if let Some(id) = open {
            self.go_to(id);
        }
    }
}
